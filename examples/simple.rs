use bytes::Bytes;
use headers::Authorization;
use http::Uri;
use http_body_util::{BodyExt as _, Empty};
use hyper::Request;
use hyper_proxy::{BoxConnector, Intercept, Proxy, ProxyConnector};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::TokioExecutor,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proxy: ProxyConnector<_> = {
        let proxy_uri = "http://localhost:8100".parse().unwrap();
        let mut proxy = Proxy::new(Intercept::All, proxy_uri);
        proxy.set_authorization(Authorization::basic("John Doe", "Agent1234"));
        let connector = BoxConnector(HttpConnector::new());
        #[cfg(not(any(feature = "tls", feature = "rustls-base", feature = "openssl-tls")))]
        let proxy_connector = ProxyConnector::from_proxy_unsecured(connector, proxy);
        #[cfg(any(feature = "tls", feature = "rustls-base", feature = "openssl"))]
        let proxy_connector = ProxyConnector::from_proxy(connector, proxy).unwrap();
        proxy_connector
    };

    // Connecting to http will trigger regular GETs and POSTs.
    // We need to manually append the relevant headers to the request
    let uri: Uri = "http://http.badssl.com/".parse().unwrap();
    let mut req = Request::get(uri.clone()).body(Empty::<Bytes>::new())?;

    if let Some(headers) = proxy.http_headers(&uri) {
        req.headers_mut().extend(headers.clone().into_iter());
    }

    let client = Client::builder(TokioExecutor::new()).build(proxy);
    let resp = client.request(req).await?;
    println!("Response: {}", resp.status());
    let full_body = resp.into_body().collect().await?.to_bytes();

    println!("Body from http: {:?}", full_body);

    // Connecting to an https uri is straightforward (uses 'CONNECT' method underneath)
    let uri = "https://mozilla-modern.badssl.com/".parse().unwrap();
    let resp = client.get(uri).await?;
    println!("Response: {}", resp.status());
    let full_body = resp.into_body().collect().await?.to_bytes();

    println!("Body from https: {:?}", full_body);

    Ok(())
}
