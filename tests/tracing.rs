#![cfg(feature = "tracing")]

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt,
    future::{Future, Ready, ready},
    pin::pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Duration,
};

use http::{Request, Response, StatusCode};
use tower::{Layer, Service, ServiceExt};
use tower_rate_limiter::{KeyExtractor, LimitProvider, RateLimitError, RateLimitLayer, Store, StoreFailureMode, Usage};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
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
struct StaticKey;

impl KeyExtractor for StaticKey {
    type Key = &'static str;

    fn extract<B>(&self, _request: &Request<B>) -> Result<Self::Key, RateLimitError> {
        Ok("caller")
    }
}

#[derive(Clone, Copy)]
struct FailingStore;

impl Store for FailingStore {
    type Future = Ready<Result<Usage, RateLimitError>>;

    fn increment(&self, _key: &str, _window: Duration) -> Self::Future {
        ready(Err(RateLimitError::Store(
            String::from("test_store_failed"),
            String::from("redis://user:secret@example.invalid"),
        )))
    }
}

#[derive(Clone, Copy)]
struct InvalidUsageStore;

impl Store for InvalidUsageStore {
    type Future = Ready<Result<Usage, RateLimitError>>;

    fn increment(&self, _key: &str, window: Duration) -> Self::Future {
        ready(Ok(Usage {
            used: 0,
            reset_after: window,
        }))
    }
}

#[derive(Clone, Copy)]
struct FailingKey;

impl KeyExtractor for FailingKey {
    type Key = &'static str;

    fn extract<B>(&self, _request: &Request<B>) -> Result<Self::Key, RateLimitError> {
        Err(RateLimitError::Key(
            String::from("test_key_failed"),
            String::from("key unavailable"),
        ))
    }
}

#[derive(Clone, Copy)]
struct FailingLimit;

impl LimitProvider for FailingLimit {
    type Future = Ready<Result<u64, RateLimitError>>;

    fn limit<B>(&self, _request: &Request<B>) -> Self::Future {
        ready(Err(RateLimitError::Quota(
            String::from("test_limit_failed"),
            String::from("quota unavailable"),
        )))
    }
}

#[derive(Clone, Copy)]
struct OkService;

impl Service<Request<()>> for OkService {
    type Response = Response<()>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: Request<()>) -> Self::Future {
        ready(Ok(Response::new(())))
    }
}

fn call_with_mode(mode: StoreFailureMode) -> StatusCode {
    let service = RateLimitLayer::builder(StaticKey)
        .policy_name("login")
        .store_failure_mode(mode)
        .with_store(FailingStore)
        .build()
        .expect("valid layer")
        .layer(OkService);

    block_on(service.oneshot(Request::new(())))
        .expect("infallible service")
        .status()
}

fn call_with_level(level: tracing::Level) -> StatusCode {
    let service = RateLimitLayer::builder(StaticKey)
        .policy_name("login")
        .store_failure_mode(StoreFailureMode::Allow)
        .store_failure_tracing_level(level)
        .with_store(FailingStore)
        .build()
        .expect("valid layer")
        .layer(OkService);

    block_on(service.oneshot(Request::new(())))
        .expect("infallible service")
        .status()
}

#[derive(Clone, Debug)]
struct CapturedEvent {
    level: tracing::Level,
    target: &'static str,
    fields: HashMap<String, String>,
}

#[test]
fn store_failure_level_is_configurable_per_policy() {
    let subscriber = EventSubscriber::default();
    let events = Arc::clone(&subscriber.events);
    let levels = [
        tracing::Level::ERROR,
        tracing::Level::WARN,
        tracing::Level::INFO,
        tracing::Level::DEBUG,
        tracing::Level::TRACE,
    ];

    tracing::subscriber::with_default(subscriber, || {
        for level in levels {
            assert_eq!(call_with_level(level), StatusCode::OK);
        }
    });

    let events = events.lock().expect("event lock");
    assert_eq!(events.len(), levels.len());
    assert_eq!(events.iter().map(|event| event.level).collect::<Vec<_>>(), levels);
}

#[derive(Clone, Default)]
struct EventSubscriber {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl Subscriber for EventSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _attributes: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.events.lock().expect("event lock").push(CapturedEvent {
            level: *event.metadata().level(),
            target: event.metadata().target(),
            fields: visitor.fields,
        });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[derive(Default)]
struct FieldVisitor {
    fields: HashMap<String, String>,
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields.insert(field.name().to_owned(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields.insert(field.name().to_owned(), format!("{value:?}"));
    }
}

#[test]
fn store_failures_emit_structured_warnings_without_diagnostic_details() {
    let subscriber = EventSubscriber::default();
    let events = Arc::clone(&subscriber.events);

    tracing::subscriber::with_default(subscriber, || {
        assert_eq!(call_with_mode(StoreFailureMode::Allow), StatusCode::OK);
        assert_eq!(
            call_with_mode(StoreFailureMode::Reject),
            StatusCode::SERVICE_UNAVAILABLE
        );
    });

    let events = events.lock().expect("event lock");
    assert_eq!(events.len(), 2);

    for (event, expected_mode) in events.iter().zip(["allow", "reject"]) {
        assert_eq!(event.level, tracing::Level::WARN);
        assert_eq!(event.target, "tower_rate_limiter::store");
        assert_eq!(
            event.fields.keys().map(String::as_str).collect::<HashSet<_>>(),
            HashSet::from(["message", "event", "policy_name", "failure_mode", "error_code"])
        );
        assert_eq!(event.fields.get("event").map(String::as_str), Some("store_failure"));
        assert_eq!(event.fields.get("policy_name").map(String::as_str), Some("login"));
        assert_eq!(
            event.fields.get("failure_mode").map(String::as_str),
            Some(expected_mode)
        );
        assert_eq!(
            event.fields.get("error_code").map(String::as_str),
            Some("test_store_failed")
        );
        assert!(
            event
                .fields
                .values()
                .all(|value| !value.contains("redis://") && !value.contains("secret"))
        );
    }
}

#[test]
fn invalid_usage_emits_store_failure() {
    let subscriber = EventSubscriber::default();
    let events = Arc::clone(&subscriber.events);
    let service = RateLimitLayer::builder(StaticKey)
        .policy_name("login")
        .with_store(InvalidUsageStore)
        .build()
        .expect("valid layer")
        .layer(OkService);

    tracing::subscriber::with_default(subscriber, || {
        assert_eq!(
            block_on(service.oneshot(Request::new(())))
                .expect("infallible service")
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    });

    let events = events.lock().expect("event lock");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].target, "tower_rate_limiter::store");
    assert_eq!(events[0].fields.get("event").map(String::as_str), Some("store_failure"));
    assert_eq!(
        events[0].fields.get("error_code").map(String::as_str),
        Some("invalid_usage")
    );
}

#[test]
fn key_and_quota_failures_do_not_emit_store_failure() {
    let subscriber = EventSubscriber::default();
    let events = Arc::clone(&subscriber.events);
    let key_failure = RateLimitLayer::builder(FailingKey)
        .with_store(FailingStore)
        .build()
        .expect("valid layer")
        .layer(OkService);
    let quota_failure = RateLimitLayer::builder(StaticKey)
        .limit_provider(FailingLimit)
        .with_store(FailingStore)
        .build()
        .expect("valid layer")
        .layer(OkService);

    tracing::subscriber::with_default(subscriber, || {
        assert_eq!(
            block_on(key_failure.oneshot(Request::new(())))
                .expect("infallible service")
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            block_on(quota_failure.oneshot(Request::new(())))
                .expect("infallible service")
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    });

    assert!(events.lock().expect("event lock").is_empty());
}
