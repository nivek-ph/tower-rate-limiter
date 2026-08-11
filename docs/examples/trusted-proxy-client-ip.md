# Trusted proxy client IP

This example uses `TrustedProxyClientIpKeyExtractor` with an application-supplied peer policy. A
trusted proxy must remove or overwrite every supported client-IP header before forwarding the
request to the application.

```sh
cargo run --example trusted_proxy_client_ip --features axum,memory
```

The server listens on `http://127.0.0.1:3001`.

## Complete source

```rust,ignore
{{#include ../../examples/trusted_proxy_client_ip.rs}}
```

The example's documentation address is a placeholder; replace it with application configuration for
the actual proxy address or CIDR set. The policy checks the Axum `ConnectInfo` peer before the
extractor accepts `Forwarded`, `X-Forwarded-For`, `X-Real-IP`, or `CF-Connecting-IP`. Untrusted peers
use their socket IP even if they send those Headers. Missing peer information is rejected.

Parsing still does not authenticate Header contents. Every trusted proxy must sanitize all four
supported Headers, and application ingress must prevent direct clients from connecting through an
address accepted by the policy. The library does not load proxy configuration from environment
variables.
