#![cfg(feature = "axum")]

use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use axum::extract::ConnectInfo;
use http::Request;
use tower_rate_limiter::{IpKeyExtractor, KeyExtractor, RateLimitError};

fn request_with_connect_info(address: Option<SocketAddr>) -> Request<()> {
    let mut request = Request::new(());
    if let Some(address) = address {
        request.extensions_mut().insert(ConnectInfo(address));
    }
    request
}

#[test]
fn ipv4_uses_the_full_canonical_address() {
    let request = request_with_connect_info(Some("192.0.2.7:443".parse().unwrap()));

    assert_eq!(
        IpKeyExtractor::new().extract(&request).unwrap(),
        "192.0.2.7".parse::<IpAddr>().unwrap()
    );
}

#[test]
fn ipv4_mapped_ipv6_uses_the_standard_ip_representation() {
    let address = SocketAddr::new(
        IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0xc000, 0x0207)),
        443,
    );
    let request = request_with_connect_info(Some(address));

    assert_eq!(
        IpKeyExtractor::new().extract(&request).unwrap(),
        address.ip()
    );
}

#[test]
fn native_ipv6_uses_the_standard_ip_representation() {
    let address: SocketAddr = "[2001:db8:abcd:1234:5678:9abc:def0:1111]:443"
        .parse()
        .unwrap();
    let request = request_with_connect_info(Some(address));

    assert_eq!(
        IpKeyExtractor::new().extract(&request).unwrap(),
        address.ip()
    );
}

#[test]
fn missing_connect_info_is_key_unavailable() {
    let request = request_with_connect_info(None);
    let error = IpKeyExtractor::new()
        .extract(&request)
        .expect_err("missing ConnectInfo must fail");

    assert!(matches!(
        error,
        RateLimitError::KeyUnavailable(code, _message) if code == "peer_ip_unavailable"
    ));
}
