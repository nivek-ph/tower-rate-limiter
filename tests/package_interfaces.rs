use std::{
    future::{Ready, ready},
    net::SocketAddr,
    time::Duration,
};

use http::{Request, Response};
use tower_rate_limiter::{
    ConfigError, IpKeyExtractor, KeyExtractor, LimitProvider, RateLimitError, RateLimitFuture,
    RateLimitLayer, ResponseFactory, ResponseReason, Store, Usage,
};

#[derive(Clone)]
struct StaticKey;

impl KeyExtractor for StaticKey {
    type Key = String;

    fn extract<B>(&self, _request: &Request<B>) -> Result<Self::Key, RateLimitError> {
        Ok(String::from("caller"))
    }
}

#[derive(Clone)]
struct TestStore;

impl Store for TestStore {
    type Future = Ready<Result<Usage, RateLimitError>>;

    fn increment(&self, _key: &str, window: Duration) -> Self::Future {
        ready(Ok(Usage {
            used: 1,
            reset_after: window,
        }))
    }
}

#[derive(Clone)]
struct TestLimit;

impl LimitProvider for TestLimit {
    type Future = Ready<Result<u64, RateLimitError>>;

    fn limit<B>(&self, _request: &Request<B>) -> Self::Future {
        ready(Ok(7))
    }
}

#[test]
fn rate_limit_future_is_publicly_nameable() {
    let _: Option<RateLimitFuture<(), (), (), (), (), (), ()>> = None;
}

#[test]
fn rate_limit_errors_expose_a_stable_code_and_message() {
    let error = RateLimitError::StoreUnavailable(
        String::from("redis_unavailable"),
        String::from("usage increment failed"),
    );

    assert!(matches!(
        error,
        RateLimitError::StoreUnavailable(code, message)
            if code == "redis_unavailable" && message == "usage increment failed"
    ));
}

#[derive(Clone)]
struct TestFactory;

impl ResponseFactory<Vec<u8>> for TestFactory {
    fn build(&self, _request: Request<Vec<u8>>, _reason: ResponseReason) -> Response<Vec<u8>> {
        Response::new(Vec::new())
    }
}

#[test]
fn custom_store_builder_is_available_without_default_features() {
    let _layer = RateLimitLayer::builder(StaticKey)
        .limit_provider(TestLimit)
        .window(Duration::from_secs(30))
        .policy_name("api")
        .with_store(TestStore)
        .response_factory(TestFactory)
        .build()
        .expect("valid builder configuration");
}

#[cfg(feature = "memory")]
#[test]
fn memory_store_is_explicitly_injected() {
    use tower_rate_limiter::MemoryStore;

    let _layer = RateLimitLayer::builder(StaticKey)
        .with_store(MemoryStore::new())
        .build()
        .expect("explicit memory store");
}

#[test]
fn invalid_window_and_policy_are_rejected_at_build() {
    let too_short = RateLimitLayer::builder(StaticKey)
        .window(Duration::from_micros(999))
        .with_store(TestStore)
        .build();
    assert!(matches!(too_short, Err(ConfigError::WindowTooShort(_, _))));

    let empty_policy = RateLimitLayer::builder(StaticKey)
        .policy_name("")
        .with_store(TestStore)
        .build();
    assert!(matches!(empty_policy, Err(ConfigError::EmptyPolicyName)));
}

#[test]
fn ip_key_extractor_reads_a_tower_socket_addr_extension() {
    let address: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(address);

    assert_eq!(
        IpKeyExtractor::new().extract(&request).unwrap(),
        address.ip()
    );
}
