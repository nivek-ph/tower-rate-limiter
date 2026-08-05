//! The unboxed request-execution state machine for the rate-limit service.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
};

use http::{Request, Response};
use pin_project_lite::pin_project;

use super::response::append_rate_limit_headers;
use super::store::{RateLimitDecision, Store, StoreErrorAction, Usage, make_key};
use super::{
    RateLimitConfig, RateLimitError, RateLimitMetadata, ResponseFactory, ResponseReason,
    append_context,
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
            metadata: Option<RateLimitMetadata>,
        },
        Ready {
            response: Response<B>,
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
            state: FutureState::Limit {
                request,
                key,
                future,
            },
            inner,
            store,
            config,
            factory,
        }
    }

    pub(crate) fn ready(
        response: Response<B>,
        inner: Inner,
        store: S,
        config: Arc<RateLimitConfig>,
        factory: Factory,
    ) -> Self {
        Self {
            state: FutureState::Ready { response },
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
                            let response =
                                this.factory.build(request, ResponseReason::Error(error));
                            this.state
                                .as_mut()
                                .project_replace(FutureState::Ready { response });
                        }
                        Ok(limit) => {
                            let mut store_key = make_key(&this.config.policy_name, &key);
                            if let Some(encoder) = this.config.key_encoding.as_ref() {
                                store_key = encoder(&store_key);
                            }
                            let store_future = this.store.increment(&store_key, this.config.window);
                            this.state.as_mut().project_replace(FutureState::Store {
                                request,
                                future: store_future,
                                limit,
                            });
                        }
                    }
                }
                StateProjection::Store { future, .. } => {
                    let result = ready!(future.poll(cx));
                    let previous = this.state.as_mut().project_replace(FutureState::Done);
                    let StateProjectionReplace::Store { request, limit, .. } = previous else {
                        unreachable!("rate-limit future state changed while polling Store")
                    };

                    let decision = result.and_then(|state| state.evaluate(limit));

                    match decision {
                        Err(_) if this.config.store_error_action == StoreErrorAction::Allow => {
                            let future = this.inner.call(request);
                            this.state.as_mut().project_replace(FutureState::Inner {
                                future,
                                metadata: None,
                            });
                        }
                        Err(error) => {
                            let response =
                                this.factory.build(request, ResponseReason::Error(error));
                            this.state
                                .as_mut()
                                .project_replace(FutureState::Ready { response });
                        }
                        Ok(RateLimitDecision::Allowed(usage)) => {
                            let metadata = RateLimitMetadata {
                                policy_name: this.config.policy_name.clone(),
                                limit,
                                usage,
                                window: this.config.window,
                                emit_headers: this.config.emit_headers,
                                rate_limited: false,
                            };
                            let mut request = request;
                            append_context(&mut request, &metadata);
                            let future = this.inner.call(request);
                            this.state.as_mut().project_replace(FutureState::Inner {
                                future,
                                metadata: Some(metadata),
                            });
                        }
                        Ok(RateLimitDecision::RateLimited(usage)) => {
                            let metadata = RateLimitMetadata {
                                policy_name: this.config.policy_name.clone(),
                                limit,
                                usage,
                                window: this.config.window,
                                emit_headers: this.config.emit_headers,
                                rate_limited: true,
                            };
                            let response = append_rate_limit_headers(
                                this.factory.build(
                                    request,
                                    ResponseReason::RateLimited(metadata.limit, metadata.usage),
                                ),
                                Some(metadata),
                            );
                            this.state
                                .as_mut()
                                .project_replace(FutureState::Ready { response });
                        }
                    }
                }
                StateProjection::Inner { future, metadata } => {
                    let result = ready!(future.poll(cx));
                    let metadata = metadata.take();
                    let previous = this.state.as_mut().project_replace(FutureState::Done);
                    let StateProjectionReplace::Inner { .. } = previous else {
                        unreachable!("rate-limit future state changed while polling inner service")
                    };
                    return Poll::Ready(
                        result.map(|response| append_rate_limit_headers(response, metadata)),
                    );
                }
                StateProjection::Ready { .. } => {
                    let previous = this.state.as_mut().project_replace(FutureState::Done);
                    let StateProjectionReplace::Ready { response } = previous else {
                        unreachable!("rate-limit future state changed while returning response")
                    };
                    return Poll::Ready(Ok(response));
                }
                StateProjection::Done { .. } => {
                    panic!("rate-limit response future polled after completion")
                }
            }
        }
    }
}
