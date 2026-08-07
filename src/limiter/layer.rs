//! Tower layer implementation.

use std::sync::Arc;

use tower::Layer;

use super::{
    builder::RateLimitConfig, key_extractor::KeyExtractor, response::DefaultResponseFactory, service::RateLimitService,
};

/// Layer that applies the [`RateLimitService`].
#[derive(Clone)]
#[must_use]
pub struct RateLimitLayer<K, S = (), P = u64, F = DefaultResponseFactory> {
    pub(crate) key_extractor: K,
    pub(crate) store: S,
    pub(crate) limit_provider: P,
    pub(crate) response_factory: F,
    pub(crate) config: Arc<RateLimitConfig>,
}

impl<K, S, P, F, Inner> Layer<Inner> for RateLimitLayer<K, S, P, F>
where
    K: KeyExtractor,
    S: Clone,
    P: Clone,
    F: Clone,
{
    type Service = RateLimitService<Inner, K, S, P, F>;

    fn layer(&self, inner: Inner) -> Self::Service {
        RateLimitService {
            inner,
            key_extractor: self.key_extractor.clone(),
            store: self.store.clone(),
            limit_provider: self.limit_provider.clone(),
            response_factory: self.response_factory.clone(),
            config: Arc::clone(&self.config),
        }
    }
}
