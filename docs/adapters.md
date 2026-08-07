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

```rust,ignore
{{#include ../examples/axum_memory.rs:serve}}
```

See [Axum with nested policies](examples/axum-memory.md) for the complete source.

If `ConnectInfo` is missing, `IpKeyExtractor` returns a key error with code
`peer_ip_unavailable`. Enabling the feature alone is not enough; the server construction shown
above is what inserts the peer address.

Forwarding headers are untrusted input. If the application sits behind a proxy, establish and test
its proxy trust policy before producing a forwarded client address; the crate does not do this
implicitly.

The [trusted forwarded addresses example](examples/axum-forwarded.md) shows application-owned
parsing for a deployment where Nginx strips client-provided forwarding headers and writes a trusted
value. Copying the parser without the matching proxy configuration would allow clients to choose
their own rate-limit identity.

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
| Runtime dependency in the adapter | none | Tokio-compatible Redis connection |

Cloning `MemoryStore` shares its in-process state. Creating separate `MemoryStore::new()` values
creates separate counter sets. With multiple application replicas, each in-memory Store enforces
its own quota, so the effective aggregate allowance can grow with replica count.
