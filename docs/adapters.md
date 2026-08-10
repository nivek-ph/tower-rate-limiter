# Axum and Redis

The core stays independent of a web framework and async runtime. Optional features provide focused
adapters without taking ownership of application lifecycle or trust policy.

## Axum

Enable Axum integration together with the default in-memory Store:

```toml
[dependencies]
tower-rate-limiter = { version = "0.1", features = ["axum", "memory"] }
```

`IpKeyExtractor` reads Axum's `ConnectInfo<SocketAddr>` as the peer address.
`ClientIpKeyExtractor` uses the same value as its fallback when no supported client-IP header is
present. The server must supply connection information when serving the router:

```rust,ignore
{{#include ../examples/axum_memory.rs:serve}}
```

See [Axum with nested policies](examples/axum-memory.md) for the complete source.

If `ConnectInfo` is missing, `IpKeyExtractor` returns `socket_ip_unavailable`. When both a client-IP
header and `ConnectInfo` are missing, `ClientIpKeyExtractor` returns `client_ip_unavailable`.
Enabling the feature alone is not enough; the server construction shown above inserts the peer
address.

`ClientIpKeyExtractor` checks `Forwarded`, `X-Forwarded-For`, `X-Real-IP`, and `CF-Connecting-IP` in
that order. These are untrusted inputs: parsing does not authenticate their sender. Only select
this adapter when a trusted proxy removes or overwrites every accepted header. A malformed
first-present source returns `invalid_client_ip` instead of falling back to another header or the
peer address. Use `IpKeyExtractor` when only the socket peer should be trusted.

The [trusted proxy client IP example](examples/trusted-proxy-client-ip.md) shows the built-in
extractor in an Axum application. Copying it without a proxy that sanitizes every supported header
would allow clients to choose their own rate-limit identity.

## Redis

Enable Redis when multiple processes need to share usage:

```toml
[dependencies]
tower-rate-limiter = { version = "0.1", default-features = false, features = ["redis", "runtime-tokio"] }
```

`RedisStore` accepts an already established `redis::aio::MultiplexedConnection`. The application
continues to own URL parsing, connection setup, reconnection strategy, and shutdown.

The `redis` feature uses one `MULTI`/`EXEC` transaction to initialize the counter, increment it, and
read its TTL. Use `redis-lua` in place of `redis` to perform the same fixed-window operation with
Lua. Either implementation must be combined with `runtime-tokio` or `runtime-smol`. A missing or
non-positive TTL is surfaced as a Store error rather than repaired implicitly.

Redis adds an `rl:` transport marker and the optional namespace after it receives the scoped key.
Use a namespace to separate deployments or applications sharing one Redis database. Namespace is a
transport concern; use distinct policy names for distinct rate-limit policies.

See [Axum with Redis](examples/axum-redis.md) for complete connection setup, namespacing, a shared
Store, and custom error responses.

### Choosing a Store

| Requirement | `MemoryStore` | `RedisStore` |
| --- | --- | --- |
| Single process | yes | yes |
| Counters shared across replicas | no | yes |
| External service required | no | yes |
| Survives process restart | no | usually, subject to Redis persistence |
| Runtime dependency in the adapter | none | Tokio- or Smol-compatible Redis connection |

Cloning `MemoryStore` shares its in-process state. Creating separate `MemoryStore::new()` values
creates separate counter sets. With multiple application replicas, each in-memory Store enforces
its own quota, so the effective aggregate allowance can grow with replica count.

Each cached entry expires with its fixed window. Moka treats the entry as absent after that point
and eventually removes it through cache maintenance without a background task.
