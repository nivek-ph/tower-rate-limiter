# Request-derived quotas

This example extracts a user ID, chooses a quota from request state, and reads `RateLimitContext`
inside the downstream service. It also sends enough requests to show the first rejected request for
both plans.

```sh
cargo run --example tower_dynamic --features memory
```

## Complete source

```rust,ignore
{{#include ../../examples/tower_dynamic.rs}}
```

The headers stand in for application-owned identity and plan state. Production code should normally
authenticate first and place validated values in request extensions.
