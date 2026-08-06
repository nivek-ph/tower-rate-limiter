//! Tower service implementation.

use std::sync::Arc;

use http::{Request, Response};
use tower::Service;

use super::builder::RateLimitConfig;
use super::charge::{ResponseFactory, ResponseReason};
use super::future::RateLimitFuture;
use super::key_extractor::KeyExtractor;
use super::limit::LimitProvider;
use super::store::Store;

/// Tower service produced by [`super::RateLimitLayer`].
#[must_use]
#[derive(Clone)]
pub struct RateLimitService<Inner, K, S, P, F> {
    pub(crate) inner: Inner,
    pub(crate) key_extractor: K,
    pub(crate) store: S,
    pub(crate) limit_provider: P,
    pub(crate) response_factory: F,
    pub(crate) config: Arc<RateLimitConfig>,
}

impl<Inner, K, S, P, F, ReqBody> Service<Request<ReqBody>> for RateLimitService<Inner, K, S, P, F>
where
    Inner: Service<Request<ReqBody>, Response = Response<ReqBody>> + Clone + Send,
    Inner::Future: Send,
    Inner::Error: Send,
    ReqBody: Send,
    K: KeyExtractor,
    S: Store,
    P: LimitProvider,
    F: ResponseFactory<ReqBody>,
{
    type Response = Response<ReqBody>;
    type Error = Inner::Error;
    type Future = RateLimitFuture<ReqBody, Inner, S, P::Future, S::Future, Inner::Future, F>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let replacement = self.inner.clone();
        let inner = std::mem::replace(&mut self.inner, replacement);
        let key = match self.key_extractor.extract(&request) {
            Ok(key) => key,
            Err(error) => {
                return RateLimitFuture::ready(
                    self.response_factory
                        .build(request, ResponseReason::Error(error)),
                    inner,
                    self.store.clone(),
                    Arc::clone(&self.config),
                    self.response_factory.clone(),
                );
            }
        };

        let limit_future = self.limit_provider.limit(&request);
        RateLimitFuture::new(
            request,
            inner,
            self.store.clone(),
            key.to_string(),
            limit_future,
            Arc::clone(&self.config),
            self.response_factory.clone(),
        )
    }
}
