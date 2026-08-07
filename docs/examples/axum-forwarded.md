# Trusted forwarded addresses

This example demonstrates where an application-owned forwarding-header policy can live. It assumes
a trusted Nginx proxy removes client-provided forwarding headers and writes the value received by
the application.

```sh
cargo run --example axum_x_forwarded_for --features axum,memory
```

The server listens on `http://127.0.0.1:3001`.

## Complete source

```rust,ignore
{{#include ../../examples/axum_x_forwarded_for.rs}}
```

Do not copy this parser unless the deployment enforces the trust assumption described above. On an
internet-facing server that accepts arbitrary `X-Forwarded-For`, a client could select or rotate its
own rate-limit key.
