//! The request-aware rate-limiting seam.
//!
//! Public interfaces are re-exported here; private files group builder configuration, request
//! execution, response handling, identity extraction, quota resolution, and Store ownership.

mod builder;
mod context;
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
pub use context::{RateLimitContext, RateLimitPolicy};
pub(crate) use context::{RateLimitMetadata, append_context};
pub use error::{ConfigError, RateLimitError};
pub use future::RateLimitFuture;
pub use key_extractor::{IpKeyExtractor, KeyExtractor};
pub use layer::RateLimitLayer;
pub use limit::LimitProvider;
pub use response::{DefaultResponseFactory, ResponseFactory, ResponseReason};
pub use service::RateLimitService;
pub use store::{Store, StoreErrorAction, Usage};
