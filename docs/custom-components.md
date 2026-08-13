# Custom components

The core has four application-facing seams. Implement only the ones whose policy belongs to your
application; the included fixed provider, default response factory, and Stores cover common cases.

## Custom client identity

`KeyExtractor` is synchronous and body-generic:

```rust,ignore
use http::Request;
use tower_rate_limiter::{KeyExtractor, RateLimitError};

#[derive(Clone)]
struct AuthenticatedAccount {
    id: String,
}

#[derive(Clone, Copy)]
struct AccountKey;

impl KeyExtractor for AccountKey {
    type Key = String;

    fn extract<B>(&self, request: &Request<B>) -> Result<Self::Key, RateLimitError> {
        request
            .extensions()
            .get::<AuthenticatedAccount>()
            .map(|account| account.id.clone())
            .ok_or_else(|| RateLimitError::Key(
                "account_missing".into(),
                "authenticated account extension is missing".into(),
            ))
    }
}
```

The key only needs to implement `Display`. Prefer a stable, non-secret identifier. Authentication
and credential validation should happen in an earlier application layer. Normalize identity once
at this boundary rather than composing several extractors.

## Request-derived quota

`LimitProvider` returns a concrete future so it can perform local or asynchronous resolution:

```rust,ignore
use std::future::{Ready, ready};
use http::Request;
use tower_rate_limiter::{LimitProvider, RateLimitError};

struct AccountPlan {
    requests_per_window: u64,
}

#[derive(Clone, Copy)]
struct PlanQuota;

impl LimitProvider for PlanQuota {
    type Future = Ready<Result<u64, RateLimitError>>;

    fn limit<B>(&self, request: &Request<B>) -> Self::Future {
        let limit = request
            .extensions()
            .get::<AccountPlan>()
            .map_or(10, |plan| plan.requests_per_window);
        ready(Ok(limit))
    }
}
```

A provider error must use `RateLimitError::Quota(code, message)`. It rejects before Store usage and
never follows the Store fail-open setting.

## Custom Store

A Store must atomically increment the complete opaque key and preserve fixed-window semantics. The
public interface is:

```rust,ignore
use std::{future::Future, time::Duration};
use tower_rate_limiter::{RateLimitError, Usage};

trait StoreShape: Clone {
    type Future: Future<Output = Result<Usage, RateLimitError>>;
    fn increment(&self, key: &str, window: Duration) -> Self::Future;
}
```

The illustrative `StoreShape` mirrors `tower_rate_limiter::Store`. An implementation must:

- make increment and first-window creation one atomic operation;
- start expiry only on the first increment;
- avoid extending expiry on later or rejected requests;
- return usage including the current increment;
- return `used >= 1` and the remaining `reset_after` duration;
- make clones of one Store value observe the same counters;
- map backend failures to `RateLimitError::Store(code, message)`.

The Store receives a policy-scoped key. It must treat that string as opaque and must not reconstruct
client or policy identity from its format.

`Store` requires `Clone` because `RateLimitLayer` clones it into each produced Service and
`RateLimit::call` clones it into each request's `ResponseFuture`. It deliberately has no
unconditional `Send + Sync + 'static` supertraits, and its Future is not universally required to be
`Send + 'static`. This keeps the core usable by local Tower executors. Add framework-specific bounds
where the Store enters that framework.

When a concrete Store is passed directly to Axum, no extra annotations are normally needed. Rust
derives whether the resulting `ResponseFuture` is `Send + 'static` from its concrete fields and
futures, so built-in Stores and custom Stores whose futures already satisfy those properties compile
directly:

```rust,ignore
let limiter = RateLimitLayer::builder(IpKeyExtractor::new())
    .with_store(MyStore::new())
    .build()?;

let app = Router::new().layer(limiter);
```

Explicit bounds become necessary when the integration is hidden behind a generic function. Axum
requires the final middleware Future to be `Send + 'static`, but `S: Store` alone intentionally does
not promise that. State the framework requirement on the generic integration point:

```rust,ignore
fn add_rate_limit<S>(router: Router, store: S) -> Result<Router, ConfigError>
where
    S: Store + Send + Sync + 'static,
    S::Future: Send + 'static,
{
    let limiter = RateLimitLayer::builder(IpKeyExtractor::new())
        .with_store(store)
        .build()?;

    Ok(router.layer(limiter))
}
```

The same rule applies to other generic injected components: if a custom `LimitProvider` is used in
an Axum helper, its future normally also needs `P::Future: Send + 'static`. The compiler reports the
specific future or captured value that prevents the composed `ResponseFuture` from being `Send`.

Earlier releases also placed `Send + Sync + 'static` and `Future: Send + 'static` on `Store`, so
`S: Store` implied them automatically. After upgrading, generic framework helpers must state their
actual runtime requirements explicitly. Concrete `RedisStore` and `MemoryStore` call sites usually
need no additional annotations because the compiler can verify their implementations directly.

## Custom responses

`ResponseFactory` receives the original request and a structured reason. `ReqBody` and `ResBody`
are independent, matching Tower services whose request and response body types differ:

```rust,ignore
use http::{Request, Response};
use tower_rate_limiter::{ResponseFactory, ResponseReason};

#[derive(Clone, Copy)]
struct ApiResponseFactory;

impl<ReqBody, ResBody: Default> ResponseFactory<ReqBody, ResBody> for ApiResponseFactory {
    fn build(&self, _request: Request<ReqBody>, reason: ResponseReason) -> Response<ResBody> {
        let mut response = Response::new(ResBody::default());
        *response.status_mut() = reason.status_code();
        response
    }
}
```

Match `ResponseReason::RateLimited(policy)` to describe quota exhaustion. `policy` contains the
resolved limit, configured window, current usage, reset duration, and `remaining()` quota. Match
`ResponseReason::Error(...)` to map key, quota, and Store failures. The middleware adds
`RateLimit`, `RateLimit-Policy`, and `Retry-After` after the factory returns where applicable.

Avoid placing secrets, raw credentials, or connection details in stable error codes, diagnostic
messages, response bodies, or logs.
