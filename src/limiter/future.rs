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
    LimitProvider, RateLimitConfig, RateLimitError,
    policy::{Policy, ResponseMetadata, append_context, make_key},
    response::{MiddlewareResponse, ResponseFactory, append_inner_response_headers},
    store::{Store, StoreFailureMode},
};

pin_project! {
    #[project = StateProj]
    #[project_replace = StateProjReplace]
    enum State<B, LimitFut, StoreFut, InnerFut> {
        Limit {
            #[pin]
            future: LimitFut,
            request: Request<B>,
            key: String,
        },
        Store {
            #[pin]
            future: StoreFut,
            request: Request<B>,
            limit: u64,
        },
        Inner {
            #[pin]
            future: InnerFut,
            metadata: Option<ResponseMetadata>,
        },
        Ready {
            response: MiddlewareResponse<B>,
        },
        Done,
    }
}

pin_project! {
    /// Response future for [`super::RateLimit`].
    pub struct ResponseFuture<B, Inner, S, P, F>
        where
        Inner: tower::Service<Request<B>>,
        P: LimitProvider,
        S: Store,
    {
        #[pin]
        state: State<B, P::Future, S::Future, Inner::Future>,
        inner: Inner,
        store: S,
        config: Arc<RateLimitConfig>,
        factory: F,
    }
}

impl<B, Inner, S, P, F> ResponseFuture<B, Inner, S, P, F>
where
    P: LimitProvider,
    S: Store,
    Inner: tower::Service<Request<B>>,
{
    pub(crate) fn new(
        request: Request<B>,
        inner: Inner,
        store: S,
        key: String,
        future: P::Future,
        config: Arc<RateLimitConfig>,
        factory: F,
    ) -> Self {
        Self {
            state: State::Limit { request, key, future },
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
        factory: F,
    ) -> Self {
        Self {
            state: State::Ready {
                response: MiddlewareResponse::Error(request, error),
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
        factory: F,
    ) -> Self {
        let future = inner.call(request);
        Self {
            state: State::Inner { future, metadata: None },
            inner,
            store,
            config,
            factory,
        }
    }
}

impl<B, Inner, S, P, F> Future for ResponseFuture<B, Inner, S, P, F>
where
    B: Send,
    Inner: tower::Service<Request<B>, Response = Response<B>> + Send,
    Inner::Future: Send,
    Inner::Error: Send,
    S: Store,
    P: LimitProvider,
    F: ResponseFactory<B>,
{
    type Output = Result<Response<B>, Inner::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        loop {
            match this.state.as_mut().project() {
                StateProj::Limit { future, .. } => {
                    let result = ready!(future.poll(cx));
                    let StateProjReplace::Limit { request, key, .. } = this.state.as_mut().project_replace(State::Done)
                    else {
                        unreachable!("rate-limit future state changed while polling Limit")
                    };

                    match result {
                        Err(error) => {
                            let response = MiddlewareResponse::Error(request, error);
                            this.state.as_mut().project_replace(State::Ready { response });
                        },
                        Ok(limit) => {
                            let mut key = make_key(&this.config.policy_name, &key);
                            if let Some(encoder) = this.config.key_encoder.as_ref() {
                                key = encoder(&key);
                            }
                            let store_future = this.store.increment(&key, this.config.window);
                            this.state.as_mut().project_replace(State::Store {
                                request,
                                future: store_future,
                                limit,
                            });
                        },
                    }
                },
                StateProj::Store { future, .. } => {
                    let result = ready!(future.poll(cx));
                    let StateProjReplace::Store { request, limit, .. } =
                        this.state.as_mut().project_replace(State::Done)
                    else {
                        unreachable!("rate-limit future state changed while polling Store")
                    };

                    let policy = result.and_then(|usage| {
                        Policy::from_usage(this.config.policy_name.clone(), this.config.window, limit, usage)
                    });

                    let next_state = match policy {
                        Err(error) => {
                            #[cfg(feature = "tracing")]
                            trace_store_failure(
                                &error,
                                this.config.store_failure_mode,
                                &this.config.policy_name,
                                this.config.store_failure_tracing_level,
                            );
                            if this.config.store_failure_mode == StoreFailureMode::Allow {
                                State::Inner {
                                    future: this.inner.call(request),
                                    metadata: None,
                                }
                            } else {
                                State::Ready {
                                    response: MiddlewareResponse::Error(request, error),
                                }
                            }
                        },
                        Ok(policy) => {
                            let metadata = ResponseMetadata::new(policy, this.config.rate_limit_fields);
                            if metadata.policy.is_rate_limited() {
                                State::Ready {
                                    response: MiddlewareResponse::RateLimited(request, metadata),
                                }
                            } else {
                                let mut request = request;
                                append_context(&mut request, &metadata);
                                State::Inner {
                                    future: this.inner.call(request),
                                    metadata: Some(metadata),
                                }
                            }
                        },
                    };
                    this.state.as_mut().project_replace(next_state);
                },
                StateProj::Inner { future, .. } => {
                    let result = ready!(future.poll(cx));
                    let StateProjReplace::Inner { metadata, .. } = this.state.as_mut().project_replace(State::Done)
                    else {
                        unreachable!("rate-limit future state changed while polling inner service")
                    };
                    return Poll::Ready(result.map(|response| append_inner_response_headers(response, metadata)));
                },
                StateProj::Ready { .. } => {
                    let StateProjReplace::Ready { response } = this.state.as_mut().project_replace(State::Done) else {
                        unreachable!("rate-limit future state changed while returning response")
                    };
                    return Poll::Ready(Ok(response.finalize(this.factory)));
                },
                StateProj::Done { .. } => {
                    panic!("rate-limit response future polled after completion")
                },
            }
        }
    }
}

#[cfg(feature = "tracing")]
fn trace_store_failure(
    error: &RateLimitError,
    failure_mode: StoreFailureMode,
    policy_name: &str,
    level: tracing::Level,
) {
    let failure_mode = match failure_mode {
        StoreFailureMode::Reject => "reject",
        StoreFailureMode::Allow => "allow",
    };

    macro_rules! emit {
        ($level:expr) => {
            tracing::event!(
                target: "tower_rate_limiter::store",
                $level,
                event = "store_failure",
                policy_name,
                failure_mode,
                error_code = error.code(),
                "rate-limit Store failed"
            )
        };
    }

    match level {
        tracing::Level::ERROR => emit!(tracing::Level::ERROR),
        tracing::Level::WARN => emit!(tracing::Level::WARN),
        tracing::Level::INFO => emit!(tracing::Level::INFO),
        tracing::Level::DEBUG => emit!(tracing::Level::DEBUG),
        tracing::Level::TRACE => emit!(tracing::Level::TRACE),
    }
}
