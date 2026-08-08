# tower-rate-limiter

Keyed HTTP rate limiting middleware for Tower.

- [Website](https://nivek-ph.github.io/tower-rate-limiter/)
- [crates.io](https://crates.io/crates/tower-rate-limiter)
- [docs.rs](https://docs.rs/tower-rate-limiter)

Each `RateLimitLayer` extracts a key from the request, resolves a quota, increments a Store, and
lets the rate-limit algorithm decide whether to call the ready inner service or return an immediate
HTTP response.

The crate is Tower-first. Axum and Redis are optional adapters.

## Features

```toml
[dependencies]
tower-rate-limiter = "0.1"

# Optional adapters
# tower-rate-limiter = { version = "0.1", features = ["axum", "redis"] }
```


| Feature            | Provides                                                               |
| ------------------ | ---------------------------------------------------------------------- |
| `memory` (default) | Runtime-independent, process-local `MemoryStore`                       |
| `redis`            | `RedisStore` backed by an existing Redis multiplexed connection        |
| `axum`             | Support for reading Axum `ConnectInfo<SocketAddr>` in `IpKeyExtractor` |


With `--no-default-features`, applications can provide their own `Store`, `KeyExtractor`, and
`ResponseFactory` without pulling in Axum, Redis, or Tokio.

## Quick start

Start with [`tower_memory`](examples/tower_memory.rs). It demonstrates a custom `KeyExtractor`,
an explicit `MemoryStore`, a fixed quota, and Tower Layer composition. For request-derived quotas
and downstream `RateLimitContext`, see [`tower_dynamic`](examples/tower_dynamic.rs).

Builder defaults:


| Setting          | Default          |
| ---------------- | ---------------- |
| Limit            | `1`              |
| Window           | 60 seconds       |
| Policy name      | `default-policy` |
| Store errors     | Reject           |
| RateLimit fields | Draft 11         |


The Store is always explicit via `.with_store(...)`.

`MemoryStore` configures each cached entry to expire with its fixed window, so inactive keys are
eventually removed without a background task.

## Request flow

```text
Request
  → KeyExtractor
  → LimitProvider
  → Store::increment(key, window)
  → Usage { used, reset_after }
  → Allowed: call the inner service
  → RateLimited: return a response without calling the inner service
```

The core interfaces have narrow responsibilities:

- `KeyExtractor` synchronously returns an application-owned key from the request.
- `LimitProvider` asynchronously resolves the request's quota. `.limit(n)` uses a fixed `u64`.
- `Store` atomically increments a scoped key and returns `Usage`.
- `ResponseFactory` turns middleware outcomes into the application's response body and status.

`LimitProvider` finishes before the Store is called, so a provider failure consumes no quota. Once
both key and limit are available, the request is charged before the inner service is called and is
not refunded based on the inner response.

## Fixed-window semantics

The first `limit` requests are allowed. Request `limit + 1` is rate limited. Rejected requests
continue increasing `Usage::used`, but they do not extend the active window.

The first increment starts the window. `Usage::reset_after` is the remaining duration until that
window expires. Returning `used == 0` violates the Store interface and follows the Store error
path.

The core scopes a key with the policy name before passing it to the Store. The window is passed
separately. Layers that share a Store, policy name, and extracted key intentionally share usage;
use different policy names for different policies or windows.

## Errors and responses

Middleware failures use one closed `RateLimitError` type with three tuple variants:

- `Key(code, message)`
- `Quota(code, message)`
- `Store(code, message)`

Each variant contains a stable machine-readable code followed by a diagnostic message. The
default `ResponseFactory` produces an empty response body with these statuses:


| Outcome            | Status                      |
| ------------------ | --------------------------- |
| `RateLimited`      | `429 Too Many Requests`     |
| `Key`               | `500 Internal Server Error` |
| `Quota`             | `500 Internal Server Error` |
| `Store`             | `503 Service Unavailable`   |


Applications can implement `ResponseFactory` to choose their own body, status, headers, and
logging. Store failures reject by default. Select `StoreFailureMode::Allow` through
`RateLimitBuilder::store_failure_mode` to call the inner service without claiming quota metadata when
the Store fails.

Key and limit failures never fail open.

### Request bypass

Use `RateLimitBuilder::skip` to exempt requests using application-trusted request headers or
extensions. The predicate runs before client-key extraction and receives the request head with a
unit body:

```rust
use std::{collections::HashSet, net::SocketAddr, sync::Arc};

let allowlist = Arc::new(HashSet::from(["192.168.0.56".parse().unwrap()]));

let limiter = RateLimitLayer::builder(IpKeyExtractor::new())
    .skip(move |request| {
        request
            .extensions()
            .get::<SocketAddr>()
            .is_some_and(|peer| allowlist.contains(&peer.ip()))
    })
    .with_store(store)
    .build()?;
```

Bypassed requests call the inner service without extracting a client key, resolving a limit,
charging the Store, or receiving rate-limit context or response fields. Keep proxy trust,
authentication, and credential validation in the application; prefer checking a validated identity
extension instead of matching raw credentials in this predicate.

## RateLimit fields and context

By default, allowed and rate-limited responses include fields following the draft-11 definitions
of [`RateLimit-Policy`](https://datatracker.ietf.org/doc/html/draft-ietf-httpapi-ratelimit-headers-11#name-ratelimit-policy-field)
and [`RateLimit`](https://datatracker.ietf.org/doc/html/draft-ietf-httpapi-ratelimit-headers-11#name-ratelimit-field):

```text
RateLimit-Policy: "<policy>";q=<limit>;w=<window-seconds>
RateLimit: "<policy>";r=<remaining>;t=<effective-window-seconds>
```

`RateLimit-Policy` advertises the configured fixed-window quota: `q` is the request limit and `w`
is the window in seconds. `RateLimit` reports the current service limit: `r` is the remaining
quota after the current request and `t` is the effective window in seconds, derived from the
Store's `reset_after`. The optional draft-11 `qu` (quota unit) and `pk` (partition key) parameters
are not emitted; omitting `qu` means the default quota unit is requests.

Select the older [draft 7](https://datatracker.ietf.org/doc/html/draft-ietf-httpapi-ratelimit-headers-07#name-ratelimit-header-field-def)
format with `.rate_limit_fields(RateLimitFields::Draft7)`:

```text
RateLimit-Policy: <limit>;w=<window-seconds>
RateLimit: limit=<limit>, remaining=<remaining>, reset=<reset-seconds>
```

Draft 7 does not include the configured policy name in either field. Omit both fields with
`.rate_limit_fields(RateLimitFields::Disabled)`; rate-limited responses still include
`Retry-After`. Active durations are rounded up to whole seconds for both revisions.

Allowed requests receive `RateLimitContext` in their request extensions. Its policies expose:

```text
policy_name, limit, used, remaining, reset_after
```

Nested Layers append policies instead of overwriting existing context or response fields.

## Axum

See [`axum_memory`](examples/axum_memory.rs) for `ConnectInfo` setup and nested policy scopes. For
a deployment-owned forwarding-header policy, see
[`axum_x_forwarded_for`](examples/axum_x_forwarded_for.rs).

`IpKeyExtractor` reads a peer `SocketAddr` request extension and returns its `IpAddr`. With the
`axum` feature, it also reads `ConnectInfo<SocketAddr>`. It does not interpret forwarding headers
or define a trusted-proxy policy; applications own that policy.

## Redis

`RedisStore` accepts an established `redis::aio::MultiplexedConnection`. It does not parse URLs,
open connections, or own connection shutdown.

See [`axum_redis`](examples/axum_redis.rs) for connection setup, namespacing, a shared Store, and
custom error responses.

One Lua operation performs `INCR`, sets `PEXPIRE` only on the first increment, and returns the
current usage plus `PTTL`. A missing or non-positive TTL is a Store error instead of an implicit
repair. Redis adds the `rl:` marker and the optional namespace to the key it receives.

Applications that need hashing or another representation can use
`RateLimitBuilder::with_key_encoder` to transform the scoped key before it reaches any Store. The
encoder must be deterministic, collision-resistant for the application's key space, non-blocking,
and free of I/O.

## Examples


| Example                                                    | Shows                                            |
| ---------------------------------------------------------- | ------------------------------------------------ |
| [`tower_memory`](examples/tower_memory.rs)                 | Basic Tower service with `MemoryStore`           |
| [`tower_dynamic`](examples/tower_dynamic.rs)               | Request-derived quota with `LimitProvider`       |
| [`axum_memory`](examples/axum_memory.rs)                   | Application and route-scoped Axum policies       |
| [`axum_x_forwarded_for`](examples/axum_x_forwarded_for.rs) | Application-owned forwarding-header trust policy |
| [`axum_redis`](examples/axum_redis.rs)                     | Shared Redis Store and custom error responses    |


```text
cargo run --example tower_memory --features memory
cargo run --example tower_dynamic --features memory
cargo run --example axum_memory --features axum,memory
cargo run --example axum_x_forwarded_for --features axum,memory
cargo run --example axum_redis --features axum,redis
```



## Verification

```text
cargo fmt --all -- --check
cargo test --no-default-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo check --examples --all-features
cargo doc --all-features --no-deps
cargo package --allow-dirty --offline
```

Redis adapter unit tests live with `RedisStore` and cover transport-key formatting and Lua
result parsing. The CI test job starts Redis and verifies atomic fixed-window behavior through
the public `Store` interface. Local `cargo test --all-features` requires `REDIS_URL` to point to a
reachable test Redis server.

## Scope

Version 0.1 intentionally focuses on fixed-window request limiting. Sliding windows, token buckets,
weighted requests, refunds, Redis Cluster, Store lifecycle methods, and built-in
forwarding-header trust are outside the current interface.
