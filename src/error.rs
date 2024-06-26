use std::error::Error as _;

use http::Uri;
#[cfg(feature = "rustls-base")]
use tokio_rustls::rustls::pki_types::InvalidDnsNameError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the data for key `{0}` is not available")]
    Io(#[from] std::io::Error),

    #[error("unexpected EOF while tunnel reading")]
    UnexpectedEOF,

    #[error("unsuccessful tunnel ({0})")]
    UnsuccessfulTunnel(String),

    #[error("Proxy Authentication Required, please set the credentials and retry")]
    ProxyAuthenticationRequired,

    #[error("Proxy {proxy_uri} is redirecting to {location} (status {status_code})")]
    ProxyRedirect {
        status_code: u16,
        location: Uri,
        proxy_uri: Uri,
    },

    #[error("Proxy is redirecting ({code}), but no location provided")]
    MissingProxyRedirectLocation { code: u16 },

    #[error("proxy uri missing scheme: {0}")]
    MissingUriHost(Uri),

    #[error("proxy uri missing host: {0}")]
    MissingUriScheme(Uri),

    #[error("{0}")]
    Http(#[from] http::Error),

    // TODO: not feasable until hyper_util::client::legacy::connect::http::ConnectError is made public
    // #[error("{0}")]
    // Connect(#[from] hyper_util::client::legacy::connect::http::ConnectError),
    #[cfg(feature = "openssl-tls")]
    #[error("{0}")]
    Openssl(#[from] openssl::error::ErrorStack),

    #[cfg(feature = "rustls-base")]
    #[error("{0}")]
    InvalidDnsNameError(#[from] InvalidDnsNameError),

    #[error("other error ({0})")]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl Error {
    /// When receiving a [`hyper_util::client::legacy::Error`] higher up in the stack, this function can be used to
    /// get a reference to the underlying [`crate::Error`] that caused it.
    pub fn as_source_of(hyper_error: &hyper_util::client::legacy::Error) -> Option<&Error> {
        hyper_error.source().and_then(|c| c.downcast_ref::<Error>())
    }
}
