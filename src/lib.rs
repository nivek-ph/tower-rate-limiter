//! Keyed HTTP rate limiting middleware for Tower.
//!
//! The crate is Tower-first: the core interfaces do not require Axum, Tokio, Redis, or a
//! particular request identity policy. Optional adapters are enabled through Cargo features.

mod limiter;
mod store;

pub use limiter::{
    ClientIpKeyExtractor, ConfigError, DefaultResponseFactory, IpKeyExtractor, KeyExtractor, LimitProvider, Policy,
    RateLimit, RateLimitBuilder, RateLimitContext, RateLimitError, RateLimitFields, RateLimitLayer, ResponseFactory,
    ResponseFuture, ResponseReason, Store, StoreFailureMode, TrustedProxyClientIpKeyExtractor, Usage,
};

#[cfg(feature = "memory")]
pub use store::{MemoryStore, MemoryStoreError};

#[cfg(all(feature = "redis", any(feature = "runtime-tokio", feature = "runtime-smol")))]
pub use store::{RedisStore, RedisStoreError};
