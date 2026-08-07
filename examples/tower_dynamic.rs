use std::{
    convert::Infallible,
    error::Error,
    future::{Ready, ready},
    time::Duration,
};

use http::{Request, Response};
use tower::{Layer, Service, ServiceExt, service_fn};
use tower_rate_limiter::{KeyExtractor, LimitProvider, MemoryStore, RateLimitContext, RateLimitError, RateLimitLayer};

/// Extract the application-owned user identity used as the Client Key.
#[derive(Clone, Copy)]
struct UserIdKeyExtractor;

impl KeyExtractor for UserIdKeyExtractor {
    type Key = String;

    fn extract<B>(&self, request: &Request<B>) -> Result<Self::Key, RateLimitError> {
        request
            .headers()
            .get("x-user-id")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                RateLimitError::Key(
                    String::from("invalid_user_id"),
                    String::from("x-user-id is missing, empty, or invalid UTF-8"),
                )
            })
    }
}

/// Resolve a different quota from the request's application-owned plan.
#[derive(Clone, Copy)]
struct PlanLimitProvider;

impl LimitProvider for PlanLimitProvider {
    // This example does a local lookup, so a ready future is sufficient. A database or
    // remote configuration lookup can return its own asynchronous future here.
    type Future = Ready<Result<u64, RateLimitError>>;

    fn limit<B>(&self, request: &Request<B>) -> Self::Future {
        let limit = match request.headers().get("x-plan").and_then(|value| value.to_str().ok()) {
            Some("premium") => 5,
            _ => 2,
        };

        ready(Ok(limit))
    }
}

async fn call<S>(service: &mut S, user_id: &str, plan: &str) -> Result<Response<()>, Infallible>
where
    S: Service<Request<()>, Response = Response<()>, Error = Infallible>,
{
    let request = Request::builder()
        .header("x-user-id", user_id)
        .header("x-plan", plan)
        .body(())
        .expect("valid demo request");

    service.ready().await?.call(request).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let limiter = RateLimitLayer::builder(UserIdKeyExtractor)
        .policy_name("plan-limit")
        .limit_provider(PlanLimitProvider)
        .window(Duration::from_secs(60))
        .with_store(MemoryStore::new())
        .build()?;

    let inner = service_fn(|request: Request<()>| async move {
        if let Some(context) = request.extensions().get::<RateLimitContext>() {
            for entry in context.policies() {
                println!(
                    "  downstream context: policy={} limit={} remaining={}",
                    entry.policy_name, entry.limit, entry.remaining
                );
            }
        }

        Ok::<Response<()>, Infallible>(Response::new(()))
    });
    let mut service = limiter.layer(inner);

    println!("free plan: 2 requests allowed");
    for attempt in 1..=3 {
        let response = call(&mut service, "user-free", "free").await?;
        println!(
            "  request {attempt}: {} ({})",
            response.status(),
            response
                .headers()
                .get("RateLimit")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("no RateLimit header")
        );
    }

    println!("premium plan: 5 requests allowed");
    for attempt in 1..=6 {
        let response = call(&mut service, "user-premium", "premium").await?;
        println!(
            "  request {attempt}: {} ({})",
            response.status(),
            response
                .headers()
                .get("RateLimit")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("no RateLimit header")
        );
    }

    Ok(())
}
