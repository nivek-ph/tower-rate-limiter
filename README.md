# tower-rate-limiter

[Crates.io](https://crates.io/crates/tower-rate-limiter) ·
[Documentation](https://docs.rs/tower-rate-limiter)

Keyed, fixed-window HTTP rate limiting middleware for [Tower](https://github.com/tower-rs/tower).
Limit repeated requests by account, API client, peer address, or any other application-defined
identity.

- Tower-first core with optional Axum integration
- Fixed or request-derived quotas
- Process-local memory and shared Redis Stores
- Custom keys, responses, bypass rules, and Store failure behavior
- Optional structured tracing for Store failures
- IETF `RateLimit` and `RateLimit-Policy` response fields

[Guide](https://nivek-ph.github.io/tower-rate-limiter/) ·
[API documentation](https://docs.rs/tower-rate-limiter) ·
[Examples](https://github.com/nivek-ph/tower-rate-limiter/tree/main/examples)

## Quick start

The default feature includes the process-local `MemoryStore`. The crate requires Rust 1.96 or
newer.

```toml
[dependencies]
tower-rate-limiter = "0.1"
```

```rust
use std::time::Duration;
use tower::Layer;
use tower_rate_limiter::{IpKeyExtractor, MemoryStore, RateLimitLayer};

let limiter = RateLimitLayer::builder(IpKeyExtractor::new())
    .policy_name("public-api")
    .limit(100)
    .window(Duration::from_secs(60))
    .with_store(MemoryStore::new())
    .build()
    .expect("valid rate-limit policy");

let service = limiter.layer(inner_service);
```

The Store is always explicit through `.with_store(...)`. `IpKeyExtractor` reads only the peer
`SocketAddr` from request extensions; Axum applications must provide `ConnectInfo`. Behind a trusted
proxy, `ClientIpKeyExtractor` can check supported client-IP headers first and fall back to that peer.
Only use its header-derived identity when the proxy removes or overwrites every accepted header. See the
[quick-start guide](https://nivek-ph.github.io/tower-rate-limiter/getting-started.html) for a
complete runnable example.

## Stores

| Store | Use when |
| --- | --- |
| `MemoryStore` | One process owns the quota, or for local development |
| `RedisStore` | Multiple processes must share one quota |
| Custom `Store` | The application needs another counter backend |

For Redis with Tokio and the default transaction implementation:

```toml
[dependencies]
tower-rate-limiter = {
    version = "0.1",
    default-features = false,
    features = ["redis", "runtime-tokio"],
}
```

Use `redis-lua` instead of `redis` for the Lua implementation, or `runtime-smol` instead of
`runtime-tokio` for Smol. The application provides an established Redis connection and owns its
lifecycle.

## Documentation

| Topic | Guide |
| --- | --- |
| Installation and first Layer | [Quick start](https://nivek-ph.github.io/tower-rate-limiter/getting-started.html) |
| Charging and fixed-window semantics | [How it works](https://nivek-ph.github.io/tower-rate-limiter/concepts.html) |
| Builder options and failure behavior | [Configuration](https://nivek-ph.github.io/tower-rate-limiter/configuration.html) |
| Axum, Redis, and proxy considerations | [Adapters](https://nivek-ph.github.io/tower-rate-limiter/adapters.html) |
| Custom keys, quotas, Stores, and responses | [Custom components](https://nivek-ph.github.io/tower-rate-limiter/custom-components.html) |
| Response field formats | [Rate Limit fields](https://nivek-ph.github.io/tower-rate-limiter/rate-limit-fields.html) |
| Deployment checklist | [Production guide](https://nivek-ph.github.io/tower-rate-limiter/production.html) |

Runnable Tower, Axum, dynamic-quota, proxy, and Redis integrations are collected in
[`examples/`](examples).

## Contributing

Bug reports, feature requests, and contributions are welcome through
[GitHub Issues](https://github.com/nivek-ph/tower-rate-limiter/issues).

## License

Licensed under either

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)); or
- MIT License ([LICENSE-MIT](LICENSE-MIT)).
