# Quick start

This guide builds a process-local limiter around a plain Tower service. It is the shortest path to
the first working layer; the same layer can later be applied to an Axum router.

## 1. Add the dependency

The default feature enables the in-memory Store:

```toml
[dependencies]
tower-rate-limiter = "0.1"
```

The crate requires Rust 1.96 or newer.

## 2. Choose a client key

Define how a request becomes an application-owned client key and compose the Layer:

```rust,ignore
{{#include ../examples/tower_memory.rs}}
```

Run the complete example from the repository root:

```sh
cargo run --example tower_memory --features memory
```

The example uses one static key so its construction is easy to see. In a real service, extract a
validated account ID, API-client ID, peer address, or another stable identity. Different callers
must produce different keys; repeated requests from one caller must produce the same key.

## 3. Configure the policy deliberately

The minimal production-shaped builder is:

```rust,ignore
use std::time::Duration;
use tower_rate_limiter::{IpKeyExtractor, MemoryStore, RateLimitLayer};

let layer = RateLimitLayer::builder(IpKeyExtractor::new())
    .policy_name("public-api")
    .limit(100)
    .window(Duration::from_secs(60))
    .with_store(MemoryStore::new())
    .build()?;
# Ok::<(), tower_rate_limiter::ConfigError>(())
```

`build()` validates configuration. A policy name cannot be empty and a window must be at least one
millisecond. The typed builder also prevents `build()` until a Store has been supplied.

## Builder defaults

| Setting | Default |
| --- | --- |
| Limit | `1` request |
| Window | 60 seconds |
| Policy name | `default-policy` |
| Store errors | Reject with `503 Service Unavailable` |
| Response fields | IETF draft 11 |

A real application should set a stable policy name, a quota, and a window deliberately. Layers that
share a Store, policy name, and extracted key intentionally share usage.

## What happens on each request

The middleware first extracts a key and resolves the quota. Only after both steps succeed does it
increment the Store. A provider failure therefore consumes no quota, while a charged request is not
refunded based on the downstream response.

The first `limit` requests are allowed. Request `limit + 1` is rate limited, and rejected requests
continue to increment usage without extending the active fixed window.

For a limit of `2`, the sequence is:

| Request | `Usage::used` | Remaining | Result |
| --- | ---: | ---: | --- |
| 1 | 1 | 1 | inner service called |
| 2 | 2 | 0 | inner service called |
| 3 | 3 | 0 | `429 Too Many Requests` |

The response from the second request has zero remaining quota but is still allowed. Exhaustion is
enforced only when `used > limit`.

## Next steps

- Read [How it works](concepts.md) for the public extension points.
- Browse the [complete examples](examples.md) for Tower, Axum, dynamic quotas, proxy handling, and Redis.
- Review every builder option in [Configuration](configuration.md).
- Use [Axum and Redis](adapters.md) when the service needs framework or shared-store integration.
- Browse the repository's [`examples/`](https://github.com/nivek-ph/tower-rate-limiter/tree/main/examples)
  for dynamic quotas, nested policies, proxy handling, and custom error responses.
