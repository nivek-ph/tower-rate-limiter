# Axum with Redis

This example shares usage through Redis, separates transport keys with a namespace, limits both peer
addresses and user IDs, and maps application-specific identity failures to HTTP statuses.

Set `REDIS_URL` before running it:

```sh
export REDIS_URL=redis://127.0.0.1:6379/
cargo run --example axum_redis --no-default-features --features axum,redis,runtime-tokio
```

The server listens on `http://127.0.0.1:3000`.

## Complete source

```rust,ignore
{{#include ../../examples/axum_redis.rs}}
```

The example uses Tokio because Axum runs on Tokio. `RedisStore` itself also supports Smol when
`redis` (or `redis-lua`) is combined with `runtime-smol`. The example owns connection creation,
while `RedisStore` owns only atomic fixed-window usage. A production application should
additionally define connection recovery, timeouts, shutdown, and observability.