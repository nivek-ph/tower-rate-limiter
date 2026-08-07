//! The unboxed request-execution state machine for the rate-limit service.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
};

use http::{Request, Response};
use pin_project_lite::pin_project;

use super::{
    RateLimitConfig, RateLimitError,
    charge::{ChargeMetadata, ChargeOutcome, append_context, make_key},
    response::{MiddlewareResponse, ResponseFactory, append_inner_response_headers},
    store::{Store, StoreFailureMode, Usage},
};

pin_project! {
    #[project = StateProjection]
    #[project_replace = StateProjectionReplace]
    enum FutureState<B, LimitFuture, StoreFuture, InnerFuture> {
        Limit {
            request: Request<B>,
            key: String,
            #[pin]
            future: LimitFuture,
        },
        Store {
            request: Request<B>,
            #[pin]
            future: StoreFuture,
            limit: u64,
        },
        Inner {
            #[pin]
            future: InnerFuture,
            metadata: Option<ChargeMetadata>,
        },
        Ready {
            response: MiddlewareResponse<B>,
        },
        Done,
    }
}

pin_project! {
    /// Response future for [`super::RateLimitService`].
    pub struct RateLimitFuture<B, Inner, S, LimitFuture, StoreFuture, InnerFuture, Factory> {
        #[pin]
        state: FutureState<B, LimitFuture, StoreFuture, InnerFuture>,
        inner: Inner,
        store: S,
        config: Arc<RateLimitConfig>,
        factory: Factory,
    }
}

impl<B, Inner, S, LimitFuture, StoreFuture, InnerFuture, Factory>
    RateLimitFuture<B, Inner, S, LimitFuture, StoreFuture, InnerFuture, Factory>
{
    pub(crate) fn new(
        request: Request<B>,
        inner: Inner,
        store: S,
        key: String,
        future: LimitFuture,
        config: Arc<RateLimitConfig>,
        factory: Factory,
    ) -> Self {
        Self {
            state: FutureState::Limit { request, key, future },
            inner,
            store,
            config,
            factory,
        }
    }

    pub(crate) fn error(
        request: Request<B>,
        error: RateLimitError,
        inner: Inner,
        store: S,
        config: Arc<RateLimitConfig>,
        factory: Factory,
    ) -> Self {
        Self {
            state: FutureState::Ready {
                response: MiddlewareResponse::error(request, error),
            },
            inner,
            store,
            config,
            factory,
        }
    }

    pub(crate) fn skipped(
        request: Request<B>,
        mut inner: Inner,
        store: S,
        config: Arc<RateLimitConfig>,
        factory: Factory,
    ) -> Self
    where
        Inner: tower::Service<Request<B>, Future = InnerFuture>,
    {
        let future = inner.call(request);
        Self {
            state: FutureState::Inner { future, metadata: None },
            inner,
            store,
            config,
            factory,
        }
    }
}

impl<B, Inner, S, LimitFuture, StoreFuture, InnerFuture, Factory> Future
    for RateLimitFuture<B, Inner, S, LimitFuture, StoreFuture, InnerFuture, Factory>
where
    B: Send,
    Inner: Send,
    S: Store<Future = StoreFuture>,
    StoreFuture: Future<Output = Result<Usage, RateLimitError>> + Send,
    LimitFuture: Future<Output = Result<u64, RateLimitError>> + Send,
    InnerFuture: Future<Output = Result<Response<B>, Inner::Error>> + Send,
    Inner: tower::Service<Request<B>, Response = Response<B>, Future = InnerFuture>,
    Inner::Error: Send,
    Factory: ResponseFactory<B>,
{
    type Output = Result<Response<B>, Inner::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        loop {
            match this.state.as_mut().project() {
                StateProjection::Limit { future, .. } => {
                    let result = ready!(future.poll(cx));
                    let previous = this.state.as_mut().project_replace(FutureState::Done);
                    let StateProjectionReplace::Limit { request, key, .. } = previous else {
                        unreachable!("rate-limit future state changed while polling Limit")
                    };

                    match result {
                        Err(error) => {
                            let response = MiddlewareResponse::error(request, error);
                            this.state.as_mut().project_replace(FutureState::Ready { response });
                        },
                        Ok(limit) => {
                            let mut key = make_key(&this.config.policy_name, &key);
                            if let Some(encoder) = this.config.key_encoder.as_ref() {
                                key = encoder(&key);
                            }
                            let store_future = this.store.increment(&key, this.config.window);
                            this.state.as_mut().project_replace(FutureState::Store {
                                request,
                                future: store_future,
                                limit,
                            });
                        },
                    }
                },
                StateProjection::Store { future, .. } => {
                    let result = ready!(future.poll(cx));
                    let previous = this.state.as_mut().project_replace(FutureState::Done);
                    let StateProjectionReplace::Store { request, limit, .. } = previous else {
                        unreachable!("rate-limit future state changed while polling Store")
                    };

                    let outcome = result.and_then(|usage| ChargeOutcome::evaluate(usage, limit, this.config));

                    match outcome {
                        Err(_) if this.config.store_failure_mode == StoreFailureMode::Allow => {
                            let future = this.inner.call(request);
                            this.state
                                .as_mut()
                                .project_replace(FutureState::Inner { future, metadata: None });
                        },
                        Err(error) => {
                            let response = MiddlewareResponse::error(request, error);
                            this.state.as_mut().project_replace(FutureState::Ready { response });
                        },
                        Ok(ChargeOutcome::Allowed(metadata)) => {
                            let mut request = request;
                            append_context(&mut request, &metadata);
                            let future = this.inner.call(request);
                            this.state.as_mut().project_replace(FutureState::Inner {
                                future,
                                metadata: Some(metadata),
                            });
                        },
                        Ok(ChargeOutcome::RateLimited(metadata)) => {
                            let response = MiddlewareResponse::rate_limited(request, metadata);
                            this.state.as_mut().project_replace(FutureState::Ready { response });
                        },
                    }
                },
                StateProjection::Inner { future, metadata } => {
                    let result = ready!(future.poll(cx));
                    let metadata = metadata.take();
                    let previous = this.state.as_mut().project_replace(FutureState::Done);
                    let StateProjectionReplace::Inner { .. } = previous else {
                        unreachable!("rate-limit future state changed while polling inner service")
                    };
                    return Poll::Ready(result.map(|response| append_inner_response_headers(response, metadata)));
                },
                StateProjection::Ready { .. } => {
                    let previous = this.state.as_mut().project_replace(FutureState::Done);
                    let StateProjectionReplace::Ready { response } = previous else {
                        unreachable!("rate-limit future state changed while returning response")
                    };
                    return Poll::Ready(Ok(response.finalize(this.factory)));
                },
                StateProjection::Done { .. } => {
                    panic!("rate-limit response future polled after completion")
                },
            }
        }
    }
}
