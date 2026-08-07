# tower-rate-limiter

Keyed, fixed-window HTTP rate limiting middleware for Tower.

`tower-rate-limiter` lets an application decide **who** a request belongs to, **what** quota applies,
and **where** usage is stored. The middleware owns the charging and HTTP response flow without
coupling the core to Axum, Tokio, Redis, or a built-in identity policy.

```text
Request
  -> optional bypass
  -> KeyExtractor
  -> LimitProvider
  -> Store::increment
  -> allow the ready inner service or return a response
```

## When to use it

Use this crate when you need per-caller HTTP enforcement in a Tower service, for example:

- one quota per authenticated account or API client;
- an IP-based limit at an application boundary;
- different quotas for free and paid plans;
- route-specific policies composed as Tower Layers;
- counters shared across processes through Redis.

This is not a global concurrency limiter or a backpressure mechanism. It charges requests to an
application-defined client key and evaluates them against a fixed-window quota.

## Design at a glance

| Concern | Application-facing seam | Included option |
| --- | --- | --- |
| Caller identity | `KeyExtractor` | `IpKeyExtractor` |
| Quota selection | `LimitProvider` | fixed `u64` via `.limit(...)` |
| Atomic usage | `Store` | `MemoryStore`, optional `RedisStore` |
| Error and rejection responses | `ResponseFactory` | `DefaultResponseFactory` |

The Store is always explicit. This makes the counter's ownership and sharing boundary visible at
layer construction instead of hiding process-local state behind a default singleton.

## Cargo features

| Feature | Default | Adds |
| --- | --- | --- |
| `memory` | yes | Runtime-independent, process-local `MemoryStore` |
| `axum` | no | Axum `ConnectInfo<SocketAddr>` support in `IpKeyExtractor` |
| `redis` | no | `RedisStore` backed by an existing multiplexed connection |

With `default-features = false`, the core remains usable with application-provided implementations
and does not pull in Axum, Redis, or Tokio.

## Start here

1. Follow the [Quick start](getting-started.md) to construct a Tower layer.
2. Browse the [complete examples](examples.md) for progressively richer integrations.
3. Review [Configuration](configuration.md) before choosing policy names, windows, and failure mode.
4. Read [How it works](concepts.md) for exact charging semantics.
5. Use [Axum and Redis](adapters.md) or implement [Custom components](custom-components.md).
6. Check the [Production guide](production.md) before deploying behind a proxy or across replicas.

The [API documentation](https://docs.rs/tower-rate-limiter) is the complete type-level reference.
The [GitHub repository](https://github.com/nivek-ph/tower-rate-limiter) contains runnable examples.
