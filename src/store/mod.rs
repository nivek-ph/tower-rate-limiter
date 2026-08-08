//! Concrete rate-limit storage adapters.

#[cfg(feature = "memory")]
mod memory;

#[cfg(feature = "memory")]
pub use memory::{MemoryStore, MemoryStoreError};

#[cfg(feature = "redis")]
mod redis;

#[cfg(feature = "redis")]
pub use redis::{RedisStore, RedisStoreFuture};
