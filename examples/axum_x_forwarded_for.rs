use std::{
    error::Error,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use axum::{Router, routing::get};
use http::Request;
use tower_rate_limiter::{IpKeyExtractor, KeyExtractor, MemoryStore, RateLimitError, RateLimitLayer};

/// The deployment's trusted Nginx proxy strips client-provided forwarding headers and writes
/// this value. The crate deliberately does not provide this trust policy itself.
#[derive(Clone, Copy)]
struct TrustedForwardedAddress;

impl KeyExtractor for TrustedForwardedAddress {
    type Key = IpAddr;

    fn extract<B>(&self, request: &Request<B>) -> Result<Self::Key, RateLimitError> {
        if let Some(header) = request.headers().get("x-forwarded-for")
            && let Ok(value) = header.to_str()
        {
            for candidate in value.split(',') {
                if let Ok(address) = candidate.trim().parse::<IpAddr>() {
                    return Ok(address);
                }
            }
        }

        IpKeyExtractor::new().extract(request)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let key_extractor = TrustedForwardedAddress;
    let limiter = RateLimitLayer::builder(key_extractor)
        .policy_name("forwarded-limit")
        .limit(100)
        .window(Duration::from_secs(60))
        .with_store(MemoryStore::new())
        .build()?;
    let app = Router::new().route("/health", get(|| async { "ok" })).layer(limiter);

    let address: SocketAddr = "127.0.0.1:3001".parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("listening on http://{address}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
