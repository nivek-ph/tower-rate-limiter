//! Fixed and request-derived quota-limit resolution.

use std::future::{Future, Ready, ready};

use http::Request;

use super::error::RateLimitError;

/// Resolve a fixed or request-derived quota limit asynchronously.
///
/// A [`super::RateLimitLayer`] clones its provider into each produced Service.
pub trait LimitProvider: Clone {
    /// The concrete future returned by [`LimitProvider::limit`].
    type Future: Future<Output = Result<u64, RateLimitError>>;

    /// Resolve the quota limit for one request.
    fn limit<B>(&self, request: &Request<B>) -> Self::Future;
}

/// Treat a raw `u64` as the fixed provider used by the builder's `.limit(...)` method.
impl LimitProvider for u64 {
    type Future = Ready<Result<u64, RateLimitError>>;

    fn limit<B>(&self, _request: &Request<B>) -> Self::Future {
        ready(Ok(*self))
    }
}
