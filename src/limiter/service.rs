//! Tower service implementation.

use std::sync::Arc;
use std::task::{Context, Poll};

use http::{Request, Response};
use tower_service::Service;

use super::{
    builder::{RateLimitConfig, check_skip_predicate},
    future::ResponseFuture,
    key_extractor::KeyExtractor,
    limit::LimitProvider,
    response::ResponseFactory,
    store::Store,
};

/// Tower service produced by [`super::RateLimitLayer`].
#[must_use]
#[derive(Clone, Debug)]
pub struct RateLimit<Inner, K, S, P, F> {
    pub(crate) inner: Inner,
    pub(crate) key_extractor: K,
    pub(crate) store: S,
    pub(crate) limit_provider: P,
    pub(crate) response_factory: F,
    pub(crate) config: Arc<RateLimitConfig>,
}

impl<Inner, K, S, P, F> RateLimit<Inner, K, S, P, F> {
    /// Borrow the wrapped service.
    pub const fn get_ref(&self) -> &Inner {
        &self.inner
    }

    /// Mutably borrow the wrapped service.
    pub fn get_mut(&mut self) -> &mut Inner {
        &mut self.inner
    }

    /// Consume this middleware and return the wrapped service.
    pub fn into_inner(self) -> Inner {
        self.inner
    }
}

impl<Inner, K, S, P, F, ReqBody, ResBody> tower_service::Service<Request<ReqBody>> for RateLimit<Inner, K, S, P, F>
where
    Inner: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    K: KeyExtractor,
    S: Store + Clone,
    P: LimitProvider,
    F: ResponseFactory<ReqBody, ResBody>,
{
    type Response = Inner::Response;
    type Error = Inner::Error;
    type Future = ResponseFuture<ReqBody, Inner, S, P, F>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let replacement = self.inner.clone();
        let inner = std::mem::replace(&mut self.inner, replacement);

        // Check if the request should be skipped.
        let request = match check_skip_predicate(self.config.skip_predicate.as_ref(), request) {
            (true, request) => {
                return ResponseFuture::skipped(
                    request,
                    inner,
                    self.store.clone(),
                    Arc::clone(&self.config),
                    self.response_factory.clone(),
                );
            },
            (false, request) => request,
        };

        // Extract the key.
        let key = match self.key_extractor.extract(&request) {
            Ok(key) => key,
            Err(error) => {
                return ResponseFuture::error(
                    request,
                    error,
                    inner,
                    self.store.clone(),
                    Arc::clone(&self.config),
                    self.response_factory.clone(),
                );
            },
        };

        let limit_future = self.limit_provider.limit(&request);
        ResponseFuture::new(
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
