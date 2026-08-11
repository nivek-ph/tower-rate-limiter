use std::{
    future::{Ready, ready},
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use http::{Request, Response, header};
use tower_rate_limiter::{
    ClientIpKeyExtractor, ConfigError, IpKeyExtractor, KeyExtractor, LimitProvider, RateLimitError, RateLimitFuture,
    RateLimitLayer, ResponseFactory, ResponseReason, Store, TrustedProxyClientIpKeyExtractor, Usage,
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
    let error = RateLimitError::Store(
        String::from("redis_unavailable"),
        String::from("usage increment failed"),
    );

    assert!(matches!(
        error,
        RateLimitError::Store(code, message)
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

    assert_eq!(IpKeyExtractor::new().extract(&request).unwrap(), address.ip());
}

#[test]
fn client_ip_key_extractor_prefers_a_client_ip_header_over_the_peer() {
    let peer: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert(header::FORWARDED, "for=198.51.100.8".parse().unwrap());

    assert_eq!(
        ClientIpKeyExtractor::new().extract(&request).unwrap(),
        "198.51.100.8".parse::<IpAddr>().unwrap()
    );
}

#[test]
fn client_ip_key_extractor_rejects_an_invalid_client_ip_header() {
    let peer: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert(header::FORWARDED, "for=not-an-ip".parse().unwrap());

    let error = ClientIpKeyExtractor::new()
        .extract(&request)
        .expect_err("an invalid client IP header must not fall back to the peer");

    assert!(matches!(
        error,
        RateLimitError::Key(code, _message) if code == "invalid_client_ip"
    ));
}

#[test]
fn client_ip_key_extractor_falls_back_to_the_peer() {
    let peer: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(peer);

    assert_eq!(ClientIpKeyExtractor::new().extract(&request).unwrap(), peer.ip());
}

#[test]
fn client_ip_key_extractor_reports_when_no_address_is_available() {
    let error = ClientIpKeyExtractor::new()
        .extract(&Request::new(()))
        .expect_err("missing client and peer IP must fail");

    assert!(matches!(
        error,
        RateLimitError::Key(code, _message) if code == "client_ip_unavailable"
    ));
}

#[test]
fn ip_key_extractor_ignores_untrusted_client_ip_headers() {
    let peer: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert(header::FORWARDED, "for=198.51.100.8".parse().unwrap());

    assert_eq!(IpKeyExtractor::new().extract(&request).unwrap(), peer.ip());
}

#[test]
fn trusted_proxy_client_ip_key_extractor_requires_a_socket_peer() {
    let mut request = Request::new(());
    request
        .headers_mut()
        .insert(header::FORWARDED, "for=198.51.100.8".parse().unwrap());

    let error = TrustedProxyClientIpKeyExtractor::new(|_| true)
        .extract(&request)
        .expect_err("a Header alone must not establish the client identity");

    assert!(matches!(
        error,
        RateLimitError::Key(code, _message) if code == "socket_ip_unavailable"
    ));
}

#[test]
fn trusted_proxy_client_ip_key_extractor_ignores_headers_from_an_untrusted_peer() {
    let peer: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert(header::FORWARDED, "for=198.51.100.8".parse().unwrap());

    let extractor = TrustedProxyClientIpKeyExtractor::new(|_| false);

    assert_eq!(extractor.extract(&request).unwrap(), peer.ip());
}

#[test]
fn trusted_proxy_client_ip_key_extractor_does_not_parse_headers_from_an_untrusted_peer() {
    let peer: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert(header::FORWARDED, "for=not-an-ip".parse().unwrap());

    let extractor = TrustedProxyClientIpKeyExtractor::new(|_| false);

    assert_eq!(extractor.extract(&request).unwrap(), peer.ip());
}

#[test]
fn trusted_proxy_client_ip_key_extractor_uses_headers_from_a_trusted_peer() {
    let peer: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert(header::FORWARDED, "for=198.51.100.8".parse().unwrap());

    let trusted_peer = peer.ip();
    let extractor = TrustedProxyClientIpKeyExtractor::new(move |candidate| candidate == trusted_peer);

    assert_eq!(
        extractor.extract(&request).unwrap(),
        "198.51.100.8".parse::<IpAddr>().unwrap()
    );
}

#[test]
fn trusted_proxy_client_ip_key_extractor_falls_back_to_a_trusted_peer() {
    let peer: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(peer);

    let extractor = TrustedProxyClientIpKeyExtractor::new(|_| true);

    assert_eq!(extractor.extract(&request).unwrap(), peer.ip());
}

#[test]
fn trusted_proxy_client_ip_key_extractor_fails_closed_on_the_first_present_header() {
    let peer: SocketAddr = "192.0.2.7:443".parse().unwrap();
    let mut request = Request::new(());
    request.extensions_mut().insert(peer);
    request
        .headers_mut()
        .insert(header::FORWARDED, "for=not-an-ip".parse().unwrap());
    request
        .headers_mut()
        .insert("x-forwarded-for", "198.51.100.8".parse().unwrap());

    let error = TrustedProxyClientIpKeyExtractor::new(|_| true)
        .extract(&request)
        .expect_err("an invalid first-present Header must not fall through");

    assert!(matches!(
        error,
        RateLimitError::Key(code, _message) if code == "invalid_client_ip"
    ));
}
