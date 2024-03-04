use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use hyper::rt;
use hyper_util::rt::TokioIo;
#[cfg(feature = "rustls-base")]
use tokio_rustls::client::TlsStream as RustlsStream;

#[cfg(all(feature = "tls", feature = "rustls"))]
compile_error!("cannot combine tls and rustls");

#[cfg(feature = "tls")]
use tokio_native_tls::TlsStream;

#[cfg(feature = "openssl-tls")]
use tokio_openssl::SslStream as OpenSslStream;

use hyper_util::client::legacy::connect::{Connected, Connection};

#[cfg(feature = "rustls-base")]
pub type TlsStream<R> = RustlsStream<R>;

#[cfg(feature = "openssl-tls")]
pub type TlsStream<R> = OpenSslStream<R>;

/// A Proxy Stream wrapper
pub enum ProxyStream<R> {
    NoProxy(R),
    Regular(R),
    #[cfg(any(feature = "tls", feature = "rustls-base", feature = "openssl-tls"))]
    Secured(Box<TokioIo<TlsStream<TokioIo<R>>>>),
}

macro_rules! match_fn_pinned {
    ($self:expr, $fn:ident, $ctx:expr, $buf:expr) => {
        match $self.get_mut() {
            ProxyStream::NoProxy(s) => Pin::new(s).$fn($ctx, $buf),
            ProxyStream::Regular(s) => Pin::new(s).$fn($ctx, $buf),
            #[cfg(any(feature = "tls", feature = "rustls-base", feature = "openssl-tls"))]
            ProxyStream::Secured(s) => Pin::new(s).$fn($ctx, $buf),
        }
    };

    ($self:expr, $fn:ident, $ctx:expr) => {
        match $self.get_mut() {
            ProxyStream::NoProxy(s) => Pin::new(s).$fn($ctx),
            ProxyStream::Regular(s) => Pin::new(s).$fn($ctx),
            #[cfg(any(feature = "tls", feature = "rustls-base", feature = "openssl-tls"))]
            ProxyStream::Secured(s) => Pin::new(s).$fn($ctx),
        }
    };
}

impl<R: rt::Read + rt::Write + Unpin> rt::Read for ProxyStream<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: rt::ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        match_fn_pinned!(self, poll_read, cx, buf)
    }
}

impl<R: rt::Read + rt::Write + Unpin> rt::Write for ProxyStream<R> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match_fn_pinned!(self, poll_write, cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        match_fn_pinned!(self, poll_write_vectored, cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            ProxyStream::NoProxy(s) => s.is_write_vectored(),
            ProxyStream::Regular(s) => s.is_write_vectored(),
            #[cfg(any(feature = "tls", feature = "rustls-base", feature = "openssl-tls"))]
            ProxyStream::Secured(s) => s.is_write_vectored(),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match_fn_pinned!(self, poll_flush, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match_fn_pinned!(self, poll_shutdown, cx)
    }
}

impl<R: rt::Read + rt::Write + Connection + Unpin> Connection for ProxyStream<R> {
    fn connected(&self) -> Connected {
        match self {
            ProxyStream::NoProxy(s) => s.connected(),

            ProxyStream::Regular(s) => s.connected().proxy(true),
            #[cfg(feature = "tls")]
            ProxyStream::Secured(s) => s
                .inner()
                .get_ref()
                .get_ref()
                .get_ref()
                .inner()
                .connected()
                .proxy(true),

            #[cfg(feature = "rustls-base")]
            ProxyStream::Secured(s) => s.inner().get_ref().0.inner().connected().proxy(true),

            #[cfg(feature = "openssl-tls")]
            ProxyStream::Secured(s) => s.inner().get_ref().inner().connected().proxy(true),
        }
    }
}
