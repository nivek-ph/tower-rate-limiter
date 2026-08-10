# Trusted proxy client IP

This example uses `ClientIpKeyExtractor` behind a trusted proxy. The proxy must remove or overwrite
every supported client-IP header before forwarding the request to the application.

```sh
cargo run --example trusted_proxy_client_ip --features axum,memory
```

The server listens on `http://127.0.0.1:3001`.

## Complete source

```rust,ignore
{{#include ../../examples/trusted_proxy_client_ip.rs}}
```

Parsing does not establish trust. `ClientIpKeyExtractor` accepts `Forwarded`, `X-Forwarded-For`,
`X-Real-IP`, and `CF-Connecting-IP`, then falls back to the socket IP when all four are absent. On
an internet-facing server that accepts arbitrary values for any of those headers, a client could
select or rotate its own rate-limit key.
