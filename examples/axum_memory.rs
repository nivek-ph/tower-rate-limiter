use std::{error::Error, net::SocketAddr, time::Duration};

use axum::{Router, routing::get};
use tower_rate_limiter::{IpKeyExtractor, MemoryStore, RateLimitLayer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let key_extractor = IpKeyExtractor::new();
    let global_limiter = RateLimitLayer::builder(key_extractor)
        .policy_name("global-limit")
        .limit(10)
        .window(Duration::from_secs(60))
        .with_store(MemoryStore::new())
        .build()?;

    let auth_limiter = RateLimitLayer::builder(key_extractor)
        .policy_name("auth-limit")
        .limit(3)
        .window(Duration::from_secs(60))
        .with_store(MemoryStore::new())
        .build()?;

    let auth_routes = Router::new()
        .route("/login", get(|| async { "login" }))
        .layer(auth_limiter);
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest("/auth", auth_routes)
        .layer(global_limiter);

    let address: SocketAddr = "127.0.0.1:3000".parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("listening on http://{address}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
