# Axum and Redis

The core stays independent of a web framework and async runtime. Optional features provide focused
adapters without taking ownership of application lifecycle or trust policy.

## Axum

Enable Axum integration together with the default in-memory Store:

```toml
[dependencies]
tower-rate-limiter = { version = "0.1", features = ["axum", "memory"] }
```

`IpKeyExtractor` can then read Axum's `ConnectInfo<SocketAddr>`. The server must supply connection
information when serving the router:

```rust
{{#include ../examples/axum_memory.rs:serve}}
```

See the complete [`axum_memory` example](https://github.com/nivek-ph/tower-rate-limiter/blob/main/examples/axum_memory.rs)
for application-wide and route-scoped policies.

Forwarding headers are untrusted input. If the application sits behind a proxy, establish and test
its proxy trust policy before producing a forwarded client address; the crate does not do this
implicitly.

## Redis

Enable Redis when multiple processes need to share usage:

```toml
[dependencies]
tower-rate-limiter = { version = "0.1", features = ["redis"] }
```

`RedisStore` accepts an already established `redis::aio::MultiplexedConnection`. The application
continues to own URL parsing, connection setup, reconnection strategy, and shutdown.

One Lua operation increments usage and starts the TTL only on the first increment. A missing or
non-positive TTL is surfaced as a Store error rather than repaired implicitly.

See [`axum_redis`](https://github.com/nivek-ph/tower-rate-limiter/blob/main/examples/axum_redis.rs)
for connection setup, namespacing, a shared Store, and custom error responses.

## Build and publish this site

Preview the site locally while editing:

```sh
mdbook serve --open
```

Generate static files for any web server:

```sh
mdbook build
```

The output is written to `book/`. It can be deployed to GitHub Pages or any static hosting service;
no Rust process is needed in production.
