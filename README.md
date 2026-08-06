# tower-rate-limiter

Keyed HTTP rate limiting middleware for Tower.

Each `RateLimitLayer` extracts a key from the request, resolves a quota, increments a Store, and
lets the rate-limit algorithm decide whether to call the ready inner service or return an immediate
HTTP response.

The crate is Tower-first. Axum and Redis are optional adapters.

## Features

```toml
[dependencies]
tower-rate-limiter = "0.1.0-alpha.0"

# Optional adapters
# tower-rate-limiter = { version = "0.1.0-alpha.0", features = ["axum", "redis"] }
```


| Feature            | Provides                                                               |
| ------------------ | ---------------------------------------------------------------------- |
| `memory` (default) | Runtime-independent, process-local `MemoryStore`                       |
| `redis`            | `RedisStore` backed by an existing Redis multiplexed connection        |
| `axum`             | Support for reading Axum `ConnectInfo<SocketAddr>` in `IpKeyExtractor` |


With `--no-default-features`, applications can provide their own `Store`, `KeyExtractor`, and
`ResponseFactory` without pulling in Axum, Redis, or Tokio.

## Quick start

Start with `[tower_memory](examples/tower_memory.rs)`. It demonstrates a custom `KeyExtractor`,
an explicit `MemoryStore`, a fixed quota, and Tower Layer composition. For request-derived quotas
and downstream `RateLimitContext`, see `[tower_dynamic](examples/tower_dynamic.rs)`.

Builder defaults:


| Setting          | Default          |
| ---------------- | ---------------- |
| Limit            | `1`              |
| Window           | 60 seconds       |
| Policy name      | `default-policy` |
| Store errors     | Reject           |
| RateLimit fields | Enabled          |


The Store is always explicit via `.with_store(...)`.

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

- `KeyUnavailable(code, message)`
- `LimitUnavailable(code, message)`
- `StoreUnavailable(code, message)`

Each variant contains a stable machine-readable code followed by a diagnostic message. The
default `ResponseFactory` produces an empty response body with these statuses:


| Outcome            | Status                      |
| ------------------ | --------------------------- |
| `RateLimited`      | `429 Too Many Requests`     |
| `KeyUnavailable`   | `500 Internal Server Error` |
| `LimitUnavailable` | `500 Internal Server Error` |
| `StoreUnavailable` | `503 Service Unavailable`   |


Applications can implement `ResponseFactory` to choose their own body, status, headers, and
logging. Store failures reject by default. Select `StoreErrorAction::Allow` through
`RateLimitBuilder::on_store_error` to call the inner service without claiming quota metadata when
the Store fails.

Key and limit failures never fail open.

## RateLimit fields and context

When enabled, allowed and rate-limited responses include the current draft-11 fields:

```text
RateLimit-Policy: "<policy>";q=<limit>;w=<window-seconds>
RateLimit: "<policy>";r=<remaining>;t=<reset-seconds>
```

Rate-limited responses also include `Retry-After`, even when `.emit_headers(false)` disables the
two RateLimit fields. Active durations are rounded up to whole seconds.

Allowed requests receive `RateLimitContext` in their request extensions. Its policies expose:

```text
policy_name, limit, used, remaining, reset_after
```

Nested Layers append policies instead of overwriting existing context or response fields.

## Axum

See `[axum_memory](examples/axum_memory.rs)` for `ConnectInfo` setup and nested policy scopes. For
a deployment-owned forwarding-header policy, see
`[axum_x_forwarded_for](examples/axum_x_forwarded_for.rs)`.

`IpKeyExtractor` reads a peer `SocketAddr` request extension and returns its `IpAddr`. With the
`axum` feature, it also reads `ConnectInfo<SocketAddr>`. It does not interpret forwarding headers
or define a trusted-proxy policy; applications own that policy.

## Redis

`RedisStore` accepts an established `redis::aio::MultiplexedConnection`. It does not parse URLs,
open connections, or own connection shutdown.

See `[axum_redis](examples/axum_redis.rs)` for connection setup, namespacing, a shared Store, and
custom error responses.

One Lua operation performs `INCR`, sets `PEXPIRE` only on the first increment, and returns the
current usage plus `PTTL`. A missing or non-positive TTL is a Store error instead of an implicit
repair. Redis adds the `rl:` marker and the optional namespace to the key it receives.

Applications that need hashing or another representation can use
`RateLimitBuilder::with_key_encoding` to transform the scoped key before it reaches any Store. The
encoder must be deterministic, collision-resistant for the application's key space, non-blocking,
and free of I/O.

## Examples


| Example                                                    | Shows                                            |
| ---------------------------------------------------------- | ------------------------------------------------ |
| `[tower_memory](examples/tower_memory.rs)`                 | Basic Tower service with `MemoryStore`           |
| `[tower_dynamic](examples/tower_dynamic.rs)`               | Request-derived quota with `LimitProvider`       |
| `[axum_memory](examples/axum_memory.rs)`                   | Application and route-scoped Axum policies       |
| `[axum_x_forwarded_for](examples/axum_x_forwarded_for.rs)` | Application-owned forwarding-header trust policy |
| `[axum_redis](examples/axum_redis.rs)`                     | Shared Redis Store and custom error responses    |


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
weighted requests, refunds, request skipping, Redis Cluster, Store lifecycle methods, and built-in
forwarding-header trust are outside the current interface.
