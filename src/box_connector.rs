use std::{
    error::Error,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use http::Uri;
use hyper::rt;
use hyper_util::client::legacy::connect::Connection;
use tower_service::Service;

/// Workaround wrapper for Connectors that expose don't all their bounded types.
///
/// This is a workaround for the fact that [`hyper_util::client::legacy::connect::HttpConnector`] implements [`Service`]
/// without exposing its `Error` type (`hyper_util::client::legacy::connect::http::ConnectError` is not public).
/// We can only wrap Services that use an `Error` type that can be converted into `hyper_proxy::Error`.
/// If `ConnectError` is ever made public this can be removed.
#[derive(Clone)]
pub struct BoxConnector<T: Clone>(pub T);

pub type BoxError = Box<dyn Error + Send + Sync>;

impl<T> Service<Uri> for BoxConnector<T>
where
    T: Clone,
    T: Service<Uri>,
    T::Response: Connection + rt::Read + rt::Write + Send + Unpin + 'static,
    T::Future: Send + 'static,
    T::Error: Into<BoxError>,
{
    type Response = T::Response;
    type Error = BoxError;

    #[allow(clippy::type_complexity)]
    type Future = Pin<Box<dyn Future<Output = Result<T::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.0.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let resp_future = self.0.call(dst);
        Box::pin(async move { resp_future.await.map_err(Into::into) })
    }
}
