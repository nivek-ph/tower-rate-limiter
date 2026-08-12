use std::{
    collections::HashMap,
    convert::Infallible,
    future::{Future, Ready, ready},
    marker::PhantomData,
    pin::pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

use http::{Request, Response, StatusCode};
use tower::{Layer, Service};
use tower_rate_limiter::{
    KeyExtractor, LimitProvider, RateLimitContext, RateLimitError, RateLimitFields, RateLimitLayer, ResponseFactory,
    ResponseReason, Store, Usage,
};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

#[derive(Clone, Copy)]
struct StaticKey(&'static str);

impl KeyExtractor for StaticKey {
    type Key = String;

    fn extract<B>(&self, _request: &Request<B>) -> Result<Self::Key, RateLimitError> {
        Ok(self.0.to_owned())
    }
}

#[derive(Clone, Copy)]
struct FailingKey;

impl KeyExtractor for FailingKey {
    type Key = String;

    fn extract<B>(&self, _request: &Request<B>) -> Result<Self::Key, RateLimitError> {
        Err(RateLimitError::Key(
            String::from("test_key_failed"),
            String::from("key"),
        ))
    }
}

#[derive(Clone, Default)]
struct FakeStore {
    counts: Arc<Mutex<HashMap<String, u64>>>,
    calls: Arc<AtomicUsize>,
    fail: bool,
    zero_usage: bool,
    reset_after: Option<Duration>,
}

impl Store for FakeStore {
    type Future = Ready<Result<Usage, RateLimitError>>;

    fn increment(&self, key: &str, window: Duration) -> Self::Future {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.fail {
            return ready(Err(RateLimitError::Store(
                String::from("test_store_failed"),
                String::from("store"),
            )));
        }
        if self.zero_usage {
            return ready(Ok(Usage {
                used: 0,
                reset_after: window,
            }));
        }
        let mut counts = self.counts.lock().expect("fake store lock");
        let count = counts.entry(key.to_owned()).or_default();
        *count += 1;
        ready(Ok(Usage {
            used: *count,
            reset_after: self.reset_after.unwrap_or(window),
        }))
    }
}

#[derive(Clone, Copy)]
struct FailingLimit;

impl LimitProvider for FailingLimit {
    type Future = Ready<Result<u64, RateLimitError>>;

    fn limit<B>(&self, _request: &Request<B>) -> Self::Future {
        ready(Err(RateLimitError::Quota(
            String::from("test_limit_failed"),
            String::from("limit"),
        )))
    }
}

#[derive(Debug)]
struct TestError(&'static str);

impl std::fmt::Display for TestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for TestError {}

#[derive(Clone, Default)]
struct OkService {
    calls: Arc<AtomicUsize>,
    bodies: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Service<Request<Vec<u8>>> for OkService {
    type Response = Response<Vec<u8>>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Vec<u8>>) -> Self::Future {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.bodies
            .lock()
            .expect("body capture lock")
            .push(request.body().clone());
        ready(Ok(Response::new(b"ok".to_vec())))
    }
}

#[derive(Clone, Default)]
struct ContextCaptureService {
    contexts: Arc<Mutex<Vec<RateLimitContext>>>,
}

impl Service<Request<Vec<u8>>> for ContextCaptureService {
    type Response = Response<Vec<u8>>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Vec<u8>>) -> Self::Future {
        if let Some(context) = request.extensions().get::<RateLimitContext>() {
            self.contexts.lock().expect("context lock").push(context.clone());
        }
        ready(Ok(Response::new(Vec::new())))
    }
}

fn request() -> Request<Vec<u8>> {
    Request::new(Vec::new())
}

#[derive(Clone, Copy)]
struct TrustedCaller;

fn call_service<S>(service: &mut S, request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, S::Error>
where
    S: Service<Request<Vec<u8>>, Response = Response<Vec<u8>>>,
{
    block_on(async {
        std::future::poll_fn(|cx| service.poll_ready(cx)).await?;
        service.call(request).await
    })
}

fn call_status<S>(service: &mut S) -> StatusCode
where
    S: Service<Request<Vec<u8>>, Response = Response<Vec<u8>>>,
    S::Error: std::fmt::Debug,
{
    call_service(service, request()).expect("response").status()
}

fn assert_header<B>(response: &Response<B>, name: &str, value: &str) {
    assert_eq!(response.headers().get(name).expect("response header"), value);
}

fn layer(store: FakeStore, limit: u64) -> RateLimitLayer<StaticKey, FakeStore> {
    RateLimitLayer::builder(StaticKey("caller"))
        .limit(limit)
        .window(Duration::from_secs(60))
        .with_store(store)
        .build()
        .expect("valid layer")
}

#[test]
fn first_limit_requests_pass_and_next_request_is_rate_limited() {
    let store = FakeStore::default();
    let calls = Arc::clone(&store.calls);
    let mut service = layer(store, 2).layer(OkService::default());

    assert_eq!(call_status(&mut service), StatusCode::OK);
    assert_eq!(call_status(&mut service), StatusCode::OK);
    assert_eq!(call_status(&mut service), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(calls.load(Ordering::Relaxed), 3);
}

#[test]
fn skip_predicate_bypasses_the_complete_rate_limit_path() {
    let store = FakeStore::default();
    let store_calls = Arc::clone(&store.calls);
    let inner = OkService::default();
    let inner_calls = Arc::clone(&inner.calls);
    let inner_bodies = Arc::clone(&inner.bodies);
    let mut service = RateLimitLayer::builder(FailingKey)
        .limit_provider(FailingLimit)
        .skip(|request| request.extensions().get::<TrustedCaller>().is_some())
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(inner);
    let mut bypassed = request();
    bypassed.extensions_mut().insert(TrustedCaller);
    *bypassed.body_mut() = b"payload".to_vec();

    let response = call_service(&mut service, bypassed).expect("bypassed response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(store_calls.load(Ordering::Relaxed), 0);
    assert_eq!(inner_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        inner_bodies.lock().expect("body capture lock").as_slice(),
        [b"payload".to_vec()]
    );
    assert!(response.headers().get("ratelimit").is_none());
    assert!(response.extensions().get::<RateLimitContext>().is_none());

    let response = call_service(&mut service, request()).expect("enforced response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(store_calls.load(Ordering::Relaxed), 0);
    assert_eq!(inner_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn limit_failures_do_not_call_store_or_inner() {
    let store = FakeStore::default();
    let store_calls = Arc::clone(&store.calls);
    let inner = OkService::default();
    let inner_calls = Arc::clone(&inner.calls);
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit_provider(FailingLimit)
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(inner);
    assert_eq!(call_status(&mut service), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(store_calls.load(Ordering::Relaxed), 0);
    assert_eq!(inner_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn empty_client_keys_are_passed_to_the_store() {
    let store = FakeStore::default();
    let stored_keys = Arc::clone(&store.counts);
    let mut service = RateLimitLayer::builder(StaticKey(""))
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    assert_eq!(call_status(&mut service), StatusCode::OK);
    assert!(stored_keys.lock().expect("store lock").contains_key("default-policy:"));
}

#[test]
fn store_error_is_rejected_or_allowed_as_configured() {
    let store = FakeStore {
        fail: true,
        ..FakeStore::default()
    };
    let inner = OkService::default();
    let inner_calls = Arc::clone(&inner.calls);
    let mut service = layer(store, 1).layer(inner);
    assert_eq!(call_status(&mut service), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(inner_calls.load(Ordering::Relaxed), 0);

    let store = FakeStore {
        fail: true,
        ..FakeStore::default()
    };
    let inner = OkService::default();
    let inner_calls = Arc::clone(&inner.calls);
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit(1)
        .store_failure_mode(tower_rate_limiter::StoreFailureMode::Allow)
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(inner);
    let response = call_service(&mut service, request()).expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(inner_calls.load(Ordering::Relaxed), 1);
    assert!(response.extensions().get::<RateLimitContext>().is_none());
}

#[test]
fn zero_usage_is_store_unavailable_by_default() {
    let store = FakeStore {
        zero_usage: true,
        ..FakeStore::default()
    };
    let mut service = layer(store, 1).layer(OkService::default());
    assert_eq!(call_status(&mut service), StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn allow_passes_through_when_store_usage_is_invalid() {
    let store = FakeStore {
        zero_usage: true,
        ..FakeStore::default()
    };
    let inner = OkService::default();
    let inner_calls = Arc::clone(&inner.calls);
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit(1)
        .store_failure_mode(tower_rate_limiter::StoreFailureMode::Allow)
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(inner);

    assert_eq!(call_status(&mut service), StatusCode::OK);
    assert_eq!(inner_calls.load(Ordering::Relaxed), 1);
}

#[derive(Clone, Copy)]
struct ErrorService;

impl Service<Request<Vec<u8>>> for ErrorService {
    type Response = Response<Vec<u8>>;
    type Error = TestError;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: Request<Vec<u8>>) -> Self::Future {
        ready(Err(TestError("inner")))
    }
}

#[test]
fn inner_error_type_is_preserved() {
    let mut service = layer(FakeStore::default(), 1).layer(ErrorService);
    let error = call_service(&mut service, request()).expect_err("inner error");
    assert_eq!(error.0, "inner");
}

#[derive(Clone, Copy)]
struct ReasonFactory;

impl ResponseFactory<Vec<u8>, Vec<u8>> for ReasonFactory {
    fn build(&self, _request: Request<Vec<u8>>, reason: ResponseReason) -> Response<Vec<u8>> {
        let status = match reason {
            ResponseReason::RateLimited(policy) => {
                assert_eq!(policy.name, "default-policy");
                assert_eq!(policy.limit, 0);
                assert_eq!(policy.window, Duration::from_secs(60));
                assert_eq!(policy.used, 1);
                assert_eq!(policy.reset_after, Duration::from_secs(60));
                assert_eq!(policy.remaining(), 0);
                assert!(policy.is_rate_limited());
                StatusCode::IM_A_TEAPOT
            },
            ResponseReason::Error(RateLimitError::Key(_, _)) | ResponseReason::Error(RateLimitError::Quota(_, _)) => {
                StatusCode::BAD_REQUEST
            },
            ResponseReason::Error(RateLimitError::Store(_, _)) => StatusCode::NOT_ACCEPTABLE,
        };
        Response::builder()
            .status(status)
            .body(Vec::new())
            .expect("valid response")
    }
}

#[test]
fn custom_response_factory_receives_rate_limited_reason() {
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit(0)
        .with_store(FakeStore::default())
        .response_factory(ReasonFactory)
        .build()
        .expect("valid layer")
        .layer(OkService::default());
    assert_eq!(call_status(&mut service), StatusCode::IM_A_TEAPOT);
}

#[test]
fn custom_response_factory_decides_how_to_handle_limit_errors() {
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit_provider(FailingLimit)
        .with_store(FakeStore::default())
        .response_factory(ReasonFactory)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    assert_eq!(call_status(&mut service), StatusCode::BAD_REQUEST);
}

#[test]
fn custom_response_factory_decides_how_to_handle_key_errors() {
    let mut service = RateLimitLayer::builder(FailingKey)
        .with_store(FakeStore::default())
        .response_factory(ReasonFactory)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    assert_eq!(call_status(&mut service), StatusCode::BAD_REQUEST);
}

#[test]
fn custom_response_factory_decides_how_to_handle_store_errors() {
    let store = FakeStore {
        fail: true,
        ..FakeStore::default()
    };
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .with_store(store)
        .response_factory(ReasonFactory)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    assert_eq!(call_status(&mut service), StatusCode::NOT_ACCEPTABLE);
}

#[derive(Clone)]
struct Ordered(Arc<Mutex<Vec<&'static str>>>);

impl Ordered {
    fn record(&self, event: &'static str) {
        self.0.lock().expect("order lock").push(event);
    }
}

impl KeyExtractor for Ordered {
    type Key = String;

    fn extract<B>(&self, _request: &Request<B>) -> Result<Self::Key, RateLimitError> {
        self.record("key");
        Ok(String::from("caller"))
    }
}

impl LimitProvider for Ordered {
    type Future = Ready<Result<u64, RateLimitError>>;

    fn limit<B>(&self, _request: &Request<B>) -> Self::Future {
        self.record("limit");
        ready(Ok(1))
    }
}

impl Store for Ordered {
    type Future = Ready<Result<Usage, RateLimitError>>;

    fn increment(&self, _key: &str, window: Duration) -> Self::Future {
        self.record("store");
        ready(Ok(Usage {
            used: 1,
            reset_after: window,
        }))
    }
}

#[test]
fn request_flow_resolves_key_then_limit_then_store() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ordered = Ordered(Arc::clone(&events));
    let mut service = RateLimitLayer::builder(ordered.clone())
        .limit_provider(ordered.clone())
        .with_store(ordered)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    call_service(&mut service, request()).expect("allowed response");
    assert_eq!(*events.lock().expect("order lock"), vec!["key", "limit", "store"]);
}

#[test]
fn key_encoder_runs_after_key_scoping_before_store() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = FakeStore::default();
    let stored_keys = Arc::clone(&store.counts);
    let encoding_events = Arc::clone(&events);
    let ordered = Ordered(Arc::clone(&events));
    let mut service = RateLimitLayer::builder(ordered.clone())
        .limit_provider(ordered)
        .policy_name("api")
        .window(Duration::from_secs(60))
        .with_key_encoder(move |key| {
            encoding_events.lock().expect("order lock").push("encoding");
            assert_eq!(key, "api:caller");
            format!("encoded:{key}")
        })
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    call_service(&mut service, request()).expect("allowed response");

    assert_eq!(*events.lock().expect("order lock"), vec!["key", "limit", "encoding"]);
    assert!(
        stored_keys
            .lock()
            .expect("store lock")
            .contains_key("encoded:api:caller")
    );
}

#[test]
fn scoped_key_is_raw_without_a_key_encoder() {
    let store = FakeStore::default();
    let stored_keys = Arc::clone(&store.counts);
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .policy_name("api")
        .window(Duration::from_secs(60))
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    call_service(&mut service, request()).expect("allowed response");

    assert!(stored_keys.lock().expect("store lock").contains_key("api:caller"));
}

#[test]
fn scoped_key_escapes_colons_and_percents_before_store() {
    let store = FakeStore::default();
    let stored_keys = Arc::clone(&store.counts);
    let mut service = RateLimitLayer::builder(StaticKey("c%d:e"))
        .policy_name("a:b")
        .window(Duration::from_secs(60))
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    call_service(&mut service, request()).expect("allowed response");

    assert!(stored_keys.lock().expect("store lock").contains_key("a%3Ab:c%25d%3Ae"));
}

#[test]
fn key_encoder_receives_the_escaped_scoped_key() {
    let store = FakeStore::default();
    let stored_keys = Arc::clone(&store.counts);
    let mut service = RateLimitLayer::builder(StaticKey("c%d:e"))
        .policy_name("a:b")
        .window(Duration::from_secs(60))
        .with_key_encoder(|key| {
            assert_eq!(key, "a%3Ab:c%25d%3Ae");
            format!("encoded:{key}")
        })
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    call_service(&mut service, request()).expect("allowed response");

    assert!(
        stored_keys
            .lock()
            .expect("store lock")
            .contains_key("encoded:a%3Ab:c%25d%3Ae")
    );
}

#[test]
fn policy_names_isolate_usage_when_one_store_is_reused() {
    let store = FakeStore::default();
    let mut first = RateLimitLayer::builder(StaticKey("caller"))
        .policy_name("first")
        .limit(1)
        .with_store(store.clone())
        .build()
        .expect("valid layer")
        .layer(OkService::default());
    let mut second = RateLimitLayer::builder(StaticKey("caller"))
        .policy_name("second")
        .limit(1)
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    assert_eq!(call_status(&mut first), StatusCode::OK);
    assert_eq!(call_status(&mut first), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(call_status(&mut second), StatusCode::OK);
}

#[test]
fn same_policy_shares_usage_when_windows_differ() {
    let store = FakeStore::default();
    let mut short = RateLimitLayer::builder(StaticKey("caller"))
        .policy_name("same-policy")
        .window(Duration::from_secs(30))
        .limit(1)
        .with_store(store.clone())
        .build()
        .expect("valid layer")
        .layer(OkService::default());
    let mut long = RateLimitLayer::builder(StaticKey("caller"))
        .policy_name("same-policy")
        .window(Duration::from_secs(60))
        .limit(1)
        .with_store(store)
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    assert_eq!(call_status(&mut short), StatusCode::OK);
    assert_eq!(call_status(&mut long), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(call_status(&mut short), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(call_status(&mut long), StatusCode::TOO_MANY_REQUESTS);
}

struct DelayedLimitFuture {
    polled: bool,
}

impl Future for DelayedLimitFuture {
    type Output = Result<u64, RateLimitError>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.polled {
            Poll::Ready(Ok(1))
        } else {
            self.polled = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[derive(Clone, Copy)]
struct DelayedLimit;

impl LimitProvider for DelayedLimit {
    type Future = DelayedLimitFuture;

    fn limit<B>(&self, _request: &Request<B>) -> Self::Future {
        DelayedLimitFuture { polled: false }
    }
}

#[test]
fn asynchronous_limit_provider_is_awaited_before_store() {
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit_provider(DelayedLimit)
        .with_store(FakeStore::default())
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    assert_eq!(call_status(&mut service), StatusCode::OK);
}

#[test]
fn allowed_response_has_draft_eleven_fields_and_context() {
    let inner = ContextCaptureService::default();
    let contexts = Arc::clone(&inner.contexts);
    let mut service = layer(FakeStore::default(), 2).layer(inner);
    let response = call_service(&mut service, request()).expect("allowed response");

    assert_header(&response, "RateLimit-Policy", "\"default-policy\";q=2;w=60");
    assert_header(&response, "RateLimit", "\"default-policy\";r=1;t=60");
    assert!(response.headers().get("Retry-After").is_none());
    let context = contexts
        .lock()
        .expect("context lock")
        .first()
        .cloned()
        .expect("rate-limit context");
    assert_eq!(context.policies().len(), 1);
    assert_eq!(context.policies()[0].name, "default-policy");
    assert_eq!(context.policies()[0].remaining(), 1);
    assert_eq!(context.policies()[0].window, Duration::from_secs(60));
}

#[test]
fn blocked_response_has_rate_fields_and_retry_after() {
    let mut service = layer(FakeStore::default(), 1).layer(OkService::default());
    let _ = call_service(&mut service, request()).expect("allowed response");
    let response = call_service(&mut service, request()).expect("blocked response");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_header(&response, "RateLimit-Policy", "\"default-policy\";q=1;w=60");
    assert_header(&response, "RateLimit", "\"default-policy\";r=0;t=60");
    assert_header(&response, "Retry-After", "60");
}

#[test]
fn nested_layers_append_policy_fields_and_context_entries() {
    let first = RateLimitLayer::builder(StaticKey("caller"))
        .policy_name("first")
        .limit(2)
        .with_store(FakeStore::default())
        .build()
        .expect("valid layer");
    let second = RateLimitLayer::builder(StaticKey("caller"))
        .policy_name("second")
        .limit(3)
        .with_store(FakeStore::default())
        .build()
        .expect("valid layer");
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let inner = first.layer(ContextCaptureService {
        contexts: Arc::clone(&contexts),
    });
    let mut service = second.layer(inner);

    let response = call_service(&mut service, request()).expect("allowed response");
    let policies = response
        .headers()
        .get_all("RateLimit-Policy")
        .iter()
        .map(|value| value.to_str().expect("policy value"))
        .collect::<Vec<_>>();
    assert_eq!(policies, vec!["\"first\";q=2;w=60", "\"second\";q=3;w=60"]);
    assert_eq!(
        contexts
            .lock()
            .expect("context lock")
            .first()
            .expect("rate-limit context")
            .policies()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "first"]
    );
}

#[test]
fn draft11_is_the_default_rate_limit_fields_revision() {
    assert_eq!(RateLimitFields::default(), RateLimitFields::Draft11);
}

#[test]
fn draft7_fields_use_the_legacy_dictionary_format() {
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit(1)
        .rate_limit_fields(RateLimitFields::Draft7)
        .with_store(FakeStore::default())
        .build()
        .expect("valid layer")
        .layer(OkService::default());

    let response = call_service(&mut service, request()).expect("allowed response");

    assert_header(&response, "RateLimit-Policy", "1;w=60");
    assert_header(&response, "RateLimit", "limit=1, remaining=0, reset=60");
}

#[test]
fn disabling_fields_keeps_retry_after_on_blocked_responses() {
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit(1)
        .rate_limit_fields(RateLimitFields::Disabled)
        .with_store(FakeStore::default())
        .build()
        .expect("valid layer")
        .layer(OkService::default());
    let _ = call_service(&mut service, request()).expect("allowed response");
    let response = call_service(&mut service, request()).expect("blocked response");

    assert!(response.headers().get("RateLimit").is_none());
    assert!(response.headers().get("RateLimit-Policy").is_none());
    assert_header(&response, "Retry-After", "60");
}

#[test]
fn active_subsecond_durations_round_up_to_one_second() {
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit(1)
        .window(Duration::from_millis(1))
        .with_store(FakeStore::default())
        .build()
        .expect("valid layer")
        .layer(OkService::default());
    let _ = call_service(&mut service, request()).expect("allowed response");
    let response = call_service(&mut service, request()).expect("blocked response");

    assert_header(&response, "RateLimit-Policy", "\"default-policy\";q=1;w=1");
    assert_header(&response, "RateLimit", "\"default-policy\";r=0;t=1");
    assert_header(&response, "Retry-After", "1");
}

#[test]
fn zero_reset_duration_rounds_up_to_one_second() {
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit(1)
        .with_store(FakeStore {
            reset_after: Some(Duration::ZERO),
            ..FakeStore::default()
        })
        .build()
        .expect("valid layer")
        .layer(OkService::default());
    let _ = call_service(&mut service, request()).expect("allowed response");
    let response = call_service(&mut service, request()).expect("blocked response");

    assert_header(&response, "RateLimit", "\"default-policy\";r=0;t=1");
    assert_header(&response, "Retry-After", "1");
}

#[test]
fn unavailable_responses_do_not_claim_quota_metadata() {
    let store = FakeStore {
        fail: true,
        ..FakeStore::default()
    };
    let mut service = layer(store, 1).layer(OkService::default());
    let response = call_service(&mut service, request()).expect("store error response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response.headers().get("RateLimit").is_none());
    assert!(response.headers().get("RateLimit-Policy").is_none());
    assert!(response.headers().get("Retry-After").is_none());
}

#[derive(Debug)]
struct ReadinessService {
    id: usize,
    next_id: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<(usize, &'static str)>>>,
}

type ReadinessEvents = Arc<Mutex<Vec<(usize, &'static str)>>>;

impl ReadinessService {
    fn new() -> (Self, ReadinessEvents) {
        let events = ReadinessEvents::default();
        let next_id = Arc::new(AtomicUsize::new(1));
        (
            Self {
                id: 0,
                next_id,
                events: Arc::clone(&events),
            },
            events,
        )
    }
}

impl Clone for ReadinessService {
    fn clone(&self) -> Self {
        Self {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            next_id: Arc::clone(&self.next_id),
            events: Arc::clone(&self.events),
        }
    }
}

impl Service<Request<Vec<u8>>> for ReadinessService {
    type Response = Response<Vec<u8>>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.events.lock().expect("readiness lock").push((self.id, "ready"));
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: Request<Vec<u8>>) -> Self::Future {
        self.events.lock().expect("readiness lock").push((self.id, "call"));
        ready(Ok(Response::new(b"ready".to_vec())))
    }
}

#[derive(Clone)]
struct DropTrackingService {
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}

impl Drop for DropTrackingService {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

impl Service<Request<Vec<u8>>> for DropTrackingService {
    type Response = Response<Vec<u8>>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: Request<Vec<u8>>) -> Self::Future {
        self.calls.fetch_add(1, Ordering::Relaxed);
        ready(Ok(Response::new(Vec::new())))
    }
}

struct PendingDropFuture<T> {
    drops: Arc<AtomicUsize>,
    _output: PhantomData<fn() -> T>,
}

impl<T> PendingDropFuture<T> {
    fn new(drops: Arc<AtomicUsize>) -> Self {
        Self {
            drops,
            _output: PhantomData,
        }
    }
}

impl<T> Future for PendingDropFuture<T> {
    type Output = T;

    fn poll(self: std::pin::Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl<T> Drop for PendingDropFuture<T> {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone)]
struct PendingLimitProvider {
    future_drops: Arc<AtomicUsize>,
}

impl LimitProvider for PendingLimitProvider {
    type Future = PendingDropFuture<Result<u64, RateLimitError>>;

    fn limit<B>(&self, _request: &Request<B>) -> Self::Future {
        PendingDropFuture::new(Arc::clone(&self.future_drops))
    }
}

#[derive(Clone)]
struct PendingStore {
    future_drops: Arc<AtomicUsize>,
}

impl Store for PendingStore {
    type Future = PendingDropFuture<Result<Usage, RateLimitError>>;

    fn increment(&self, _key: &str, _window: Duration) -> Self::Future {
        PendingDropFuture::new(Arc::clone(&self.future_drops))
    }
}

#[test]
fn allowed_request_calls_the_exact_inner_instance_reserved_by_poll_ready() {
    let (inner, events) = ReadinessService::new();
    let mut service = layer(FakeStore::default(), 1).layer(inner);
    let mut context = Context::from_waker(Waker::noop());

    assert!(matches!(service.poll_ready(&mut context), Poll::Ready(Ok(()))));
    let response = block_on(service.call(request())).expect("allowed response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(*events.lock().expect("readiness lock"), vec![(0, "ready"), (0, "call")]);
}

#[test]
fn rejected_request_drops_the_reserved_inner_without_calling_it() {
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let inner = DropTrackingService {
        calls: Arc::clone(&calls),
        drops: Arc::clone(&drops),
    };
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit(0)
        .with_store(FakeStore::default())
        .build()
        .expect("valid layer")
        .layer(inner);

    let response = call_service(&mut service, request()).expect("blocked response");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn cancelling_a_pending_limit_releases_the_future_and_reserved_inner_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let inner_drops = Arc::new(AtomicUsize::new(0));
    let limit_drops = Arc::new(AtomicUsize::new(0));
    let inner = DropTrackingService {
        calls: Arc::clone(&calls),
        drops: Arc::clone(&inner_drops),
    };
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .limit_provider(PendingLimitProvider {
            future_drops: Arc::clone(&limit_drops),
        })
        .with_store(FakeStore::default())
        .build()
        .expect("valid layer")
        .layer(inner);
    let mut context = Context::from_waker(Waker::noop());

    assert!(matches!(service.poll_ready(&mut context), Poll::Ready(Ok(()))));
    let mut response = Box::pin(service.call(request()));
    assert!(matches!(response.as_mut().poll(&mut context), Poll::Pending));
    drop(response);

    assert_eq!(limit_drops.load(Ordering::Relaxed), 1);
    assert_eq!(inner_drops.load(Ordering::Relaxed), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn cancelling_a_pending_store_releases_the_future_and_reserved_inner_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let inner_drops = Arc::new(AtomicUsize::new(0));
    let store_drops = Arc::new(AtomicUsize::new(0));
    let inner = DropTrackingService {
        calls: Arc::clone(&calls),
        drops: Arc::clone(&inner_drops),
    };
    let mut service = RateLimitLayer::builder(StaticKey("caller"))
        .with_store(PendingStore {
            future_drops: Arc::clone(&store_drops),
        })
        .build()
        .expect("valid layer")
        .layer(inner);
    let mut context = Context::from_waker(Waker::noop());

    assert!(matches!(service.poll_ready(&mut context), Poll::Ready(Ok(()))));
    let mut response = Box::pin(service.call(request()));
    assert!(matches!(response.as_mut().poll(&mut context), Poll::Pending));
    drop(response);

    assert_eq!(store_drops.load(Ordering::Relaxed), 1);
    assert_eq!(inner_drops.load(Ordering::Relaxed), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}
