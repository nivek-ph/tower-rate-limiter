//! Fixed and request-derived quota limits.

use std::future::{Future, Ready, ready};

use http::Request;

use super::RateLimitError;

/// A quota provider that can resolve a fixed or request-derived limit asynchronously.
pub trait LimitProvider: Clone + Send + Sync + 'static {
    /// The concrete future returned by [`LimitProvider::limit`].
    type Future: Future<Output = Result<u64, RateLimitError>> + Send + 'static;

    /// Resolve the quota limit for one request.
    fn limit<B>(&self, request: &Request<B>) -> Self::Future;
}

/// Treat a raw `u64` as a fixed quota provider for the builder's `.limit(...)` method.
impl LimitProvider for u64 {
    type Future = Ready<Result<u64, RateLimitError>>;

    fn limit<B>(&self, _request: &Request<B>) -> Self::Future {
        ready(Ok(*self))
    }
}
