//! Keyed HTTP rate limiting middleware for Tower.
//!
//! The crate is Tower-first: the core interfaces do not require Axum, Tokio, Redis, or a
//! particular request identity policy. Optional adapters are enabled through Cargo features.

pub mod limiter;
pub mod store;

pub use limiter::{
    ConfigError, DefaultResponseFactory, IpKeyExtractor, KeyExtractor, LimitProvider,
    RateLimitBuilder, RateLimitContext, RateLimitError, RateLimitFuture, RateLimitLayer,
    RateLimitPolicy, ResponseFactory, ResponseReason, Store, StoreFailureMode, Usage,
};

#[cfg(feature = "memory")]
pub use store::MemoryStore;

#[cfg(feature = "redis")]
pub use store::{RedisStore, RedisStoreFuture};
