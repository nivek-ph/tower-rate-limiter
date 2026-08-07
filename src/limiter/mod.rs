//! The request-aware rate-limiting seam.
//!
//! Public interfaces are re-exported here; private files group builder configuration, the Tower
//! lifecycle, Store ownership, errors, charged-request behaviour, and response finalization.

mod builder;
mod charge;
mod error;
mod future;
mod key_extractor;
mod layer;
mod limit;
mod response;
mod service;
mod store;

pub use builder::RateLimitBuilder;
pub(crate) use builder::RateLimitConfig;
pub use charge::{RateLimitContext, RateLimitPolicy};
pub use error::{ConfigError, RateLimitError};
pub use future::RateLimitFuture;
pub use key_extractor::{IpKeyExtractor, KeyExtractor};
pub use layer::RateLimitLayer;
pub use limit::LimitProvider;
pub use response::{DefaultResponseFactory, RateLimitFields, ResponseFactory, ResponseReason};
pub use service::RateLimitService;
pub use store::{Store, StoreFailureMode, Usage};
