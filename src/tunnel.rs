use bytes::{buf::Buf, BytesMut};
use http::{HeaderMap, Uri};
use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::Error;

macro_rules! try_ready {
    ($x:expr) => {
        match $x {
            core::task::Poll::Ready(Ok(x)) => x,
            core::task::Poll::Ready(Err(e)) => return core::task::Poll::Ready(Err(e.into())),
            core::task::Poll::Pending => return core::task::Poll::Pending,
        }
    };
}

pub(crate) struct TunnelConnect {
    proxy_uri: Uri,
    buf: BytesMut,
}

impl TunnelConnect {
    /// Change stream
    pub fn with_stream<S>(self, stream: S) -> Tunnel<S> {
        Tunnel {
            proxy_uri: self.proxy_uri,
            buf: self.buf,
            stream: Some(stream),
            state: TunnelState::Writing,
        }
    }
}

pub(crate) struct Tunnel<S> {
    proxy_uri: Uri,
    buf: BytesMut,
    stream: Option<S>,
    state: TunnelState,
}

#[derive(Debug)]
enum TunnelState {
    Writing,
    Reading,
}

struct HeadersDisplay<'a>(&'a HeaderMap);

impl<'a> Display for HeadersDisplay<'a> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        for (key, value) in self.0 {
            let value_str = value.to_str().map_err(|_| fmt::Error)?;
            write!(f, "{}: {}\r\n", key.as_str(), value_str)?;
        }

        Ok(())
    }
}

/// Creates a new tunnel through proxy
pub(crate) fn new(
    proxy_uri: &Uri,
    target_host: &str,
    port: u16,
    headers: &HeaderMap,
) -> TunnelConnect {
    let buf = format!(
        "CONNECT {0}:{1} HTTP/1.1\r\n\
         Host: {0}:{1}\r\n\
         {2}\
         \r\n",
        target_host,
        port,
        HeadersDisplay(headers)
    )
    .into_bytes();

    TunnelConnect {
        proxy_uri: proxy_uri.to_owned(),
        buf: buf.as_slice().into(),
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Future for Tunnel<S> {
    type Output = Result<S, Error>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.stream.is_none() {
            panic!("must not poll after future is complete")
        }

        let this = self.get_mut();

        loop {
            if let TunnelState::Writing = &this.state {
                let fut = this.stream.as_mut().unwrap().write_buf(&mut this.buf);
                futures_util::pin_mut!(fut);
                let n = try_ready!(fut.poll(ctx));

                if !this.buf.has_remaining() {
                    this.state = TunnelState::Reading;
                    this.buf.truncate(0);
                } else if n == 0 {
                    return Poll::Ready(Err(Error::UnexpectedEOF));
                }
            } else {
                let fut = this.stream.as_mut().unwrap().read_buf(&mut this.buf);
                futures_util::pin_mut!(fut);
                let n = try_ready!(fut.poll(ctx));

                let mut headers = [httparse::EMPTY_HEADER; 16];
                let mut response = httparse::Response::new(&mut headers);

                if n == 0 {
                    return Poll::Ready(Err(Error::UnexpectedEOF));
                } else {
                    let read = &this.buf[..];

                    let parsed = match response.parse(read) {
                        Ok(parsed) => parsed,
                        Err(err) => {
                            return Poll::Ready(Err(Error::UnsuccessfulTunnel(format!(
                                "failed to parse proxy http response ({err})"
                            ))))
                        }
                    };

                    if parsed.is_complete() {
                        match response.code {
                            Some(200) => {
                                // connect tunnel on 200
                                if read.ends_with(b"\r\n\r\n") {
                                    return Poll::Ready(Ok(this.stream.take().unwrap()));
                                }
                            }
                            Some(301..=399) => {
                                // error with location on 301
                                let location = response
                                    .headers
                                    .iter()
                                    .find(|h| h.name.to_lowercase() == "location");
                                if let Some(location) = location
                                    .and_then(|l| std::convert::TryInto::try_into(l.value).ok())
                                {
                                    return Poll::Ready(Err(Error::ProxyRedirect {
                                        status_code: response.code.unwrap(),
                                        proxy_uri: this.proxy_uri.to_owned(),
                                        location,
                                    }));
                                } else if let Some(code) = response.code {
                                    return Poll::Ready(Err(Error::MissingProxyRedirectLocation {
                                        code,
                                    }));
                                } else {
                                    return Poll::Ready(Err(Error::UnsuccessfulTunnel(
                                        String::from_utf8_lossy(read).to_string(),
                                    )));
                                }
                            }
                            // error with authentication required
                            Some(407) => {
                                return Poll::Ready(Err(Error::ProxyAuthenticationRequired));
                            }
                            _ => {
                                // let len = read.len().min(16);
                                return Poll::Ready(Err(Error::UnsuccessfulTunnel(
                                    String::from_utf8_lossy(read).to_string(),
                                )));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::Error;

    use super::{HeaderMap, Tunnel};
    use futures_util::future::TryFutureExt;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use tokio::net::TcpStream;
    use tokio::runtime::Runtime;

    fn tunnel<S>(conn: S, host: String, port: u16) -> Tunnel<S> {
        let fake_uri = http::Uri::from_static("http://example.com");
        super::new(&fake_uri, &host, port, &HeaderMap::new()).with_stream(conn)
    }

    macro_rules! mock_tunnel {
        () => {{
            mock_tunnel!(
                b"\
                HTTP/1.1 200 OK\r\n\
                \r\n\
                "
            )
        }};
        ($write:expr) => {{
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let connect_expected = format!(
                "\
                 CONNECT {0}:{1} HTTP/1.1\r\n\
                 Host: {0}:{1}\r\n\
                 \r\n\
                 ",
                addr.ip(),
                addr.port()
            )
            .into_bytes();

            thread::spawn(move || {
                let (mut sock, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).unwrap();
                assert_eq!(&buf[..n], &connect_expected[..]);

                sock.write_all($write).unwrap();
            });
            addr
        }};
    }

    #[test]
    fn test_tunnel() {
        let addr = mock_tunnel!();

        let core = Runtime::new().unwrap();
        let work = TcpStream::connect(&addr);
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work
            .map_err(Error::from)
            .and_then(|tcp| tunnel(tcp, host, port));

        core.block_on(work).unwrap();
    }

    #[test]
    fn test_tunnel_eof() {
        let addr = mock_tunnel!(b"HTTP/1.1 200 OK");

        let core = Runtime::new().unwrap();
        let work = TcpStream::connect(&addr);
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work
            .map_err(Error::from)
            .and_then(|tcp| tunnel(tcp, host, port));

        core.block_on(work).unwrap_err();
    }

    #[test]
    fn test_tunnel_bad_response() {
        let addr = mock_tunnel!(b"foo bar baz hallo");

        let core = Runtime::new().unwrap();
        let work = TcpStream::connect(&addr);
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work
            .map_err(Error::from)
            .and_then(|tcp| tunnel(tcp, host, port));

        core.block_on(work).unwrap_err();
    }
}
