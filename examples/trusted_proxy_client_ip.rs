use std::{error::Error, net::SocketAddr, time::Duration};

use axum::{Router, routing::get};
use tower_rate_limiter::{ClientIpKeyExtractor, MemoryStore, RateLimitLayer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Only use this extractor when a trusted proxy removes or overwrites every supported
    // client-IP header before forwarding the request to this application.
    let key_extractor = ClientIpKeyExtractor::new();
    let limiter = RateLimitLayer::builder(key_extractor)
        .policy_name("client-ip-limit")
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
