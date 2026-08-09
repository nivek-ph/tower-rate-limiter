# Examples

The examples progress from a framework-independent Tower service to shared Redis counters. Every
page below embeds the complete source file used by the repository, so the documentation and the
compiled example cannot drift independently.

| Example | What it demonstrates | Required features |
| --- | --- | --- |
| [Tower with MemoryStore](examples/tower-memory.md) | Minimal Tower Layer composition | `memory` |
| [Request-derived quotas](examples/tower-dynamic.md) | Custom key extraction, `LimitProvider`, downstream context | `memory` |
| [Axum with nested policies](examples/axum-memory.md) | `ConnectInfo`, global and route-scoped policies | `axum,memory` |
| [Trusted forwarded addresses](examples/axum-forwarded.md) | Deployment-owned proxy trust policy | `axum,memory` |
| [Axum with Redis](examples/axum-redis.md) | Shared Store, namespace, custom error responses | `axum,redis,runtime-tokio` |

Run an example from the repository root with the command shown on its page. The Axum examples start
an HTTP server and keep running until interrupted. The Redis example additionally requires
`REDIS_URL` to reference a reachable Redis server.

These programs are intentionally small. They demonstrate the limiter boundary, not production
authentication, proxy configuration, connection supervision, observability, or graceful shutdown.
Review the [Production guide](production.md) before adapting them to a deployed service.
