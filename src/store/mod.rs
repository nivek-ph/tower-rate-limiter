//! Concrete rate-limit storage adapters.

#[cfg(feature = "memory")]
mod memory;

#[cfg(feature = "memory")]
pub use memory::{MemoryStore, MemoryStoreError};

#[cfg(all(feature = "redis", any(feature = "runtime-tokio", feature = "runtime-smol")))]
mod redis;

#[cfg(all(feature = "redis", any(feature = "runtime-tokio", feature = "runtime-smol")))]
pub use redis::{RedisStore, RedisStoreError, RedisStoreFuture};
