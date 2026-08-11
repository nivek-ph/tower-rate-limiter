//! The request-aware rate-limiting seam.
//!
//! Public interfaces are re-exported here; private files group builder configuration, the Tower
//! lifecycle, Store ownership, policy state, errors, and response finalization.

mod builder;
mod error;
pub mod future;
mod key_extractor;
mod layer;
mod limit;
mod policy;
mod response;
mod service;
mod store;

pub use builder::RateLimitBuilder;
pub(crate) use builder::RateLimitConfig;
pub use error::{ConfigError, RateLimitError};
pub use key_extractor::{ClientIpKeyExtractor, IpKeyExtractor, KeyExtractor, TrustedProxyClientIpKeyExtractor};
pub use layer::RateLimitLayer;
pub use limit::LimitProvider;
pub use policy::{Policy, RateLimitContext};
pub use response::{DefaultResponseFactory, RateLimitFields, ResponseFactory, ResponseReason};
pub use service::RateLimit;
pub use store::{Store, StoreFailureMode, Usage};
