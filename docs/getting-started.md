# Quick start

The Store is always explicit. Add the crate with its default in-memory implementation:

```toml
[dependencies]
tower-rate-limiter = "0.1"
```

Then define how a request becomes an application-owned client key and compose the Layer:

```rust
{{#include ../examples/tower_memory.rs}}
```

Run the complete example from the repository root:

```sh
cargo run --example tower_memory --features memory
```

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

## Next steps

- Read [How it works](concepts.md) for the public extension points.
- Use [Axum and Redis](adapters.md) when the service needs framework or shared-store integration.
- Browse the repository's [`examples/`](https://github.com/nivek-ph/tower-rate-limiter/tree/main/examples)
  for dynamic quotas, nested policies, proxy handling, and custom error responses.
