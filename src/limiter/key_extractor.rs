//! Request identity extraction for the rate-limiting layer.

use std::{
    fmt::{Debug, Display},
    hash::Hash,
    net::{IpAddr, SocketAddr},
};

use http::Request;

use crate::RateLimitError;

/// Extract a client key synchronously from a request.
pub trait KeyExtractor: Clone + Send + Sync {
    /// The type of the key.
    type Key: Clone + Hash + Eq + Debug + Display;

    /// Extract the key from the request.
    fn extract<T>(&self, request: &Request<T>) -> Result<Self::Key, RateLimitError>;
}

#[cfg(feature = "axum")]
use ::axum::extract::ConnectInfo;

#[cfg(feature = "axum")]
fn maybe_connect_info<T>(request: &Request<T>) -> Option<IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|address| address.0.ip())
}

#[cfg(not(feature = "axum"))]
fn maybe_connect_info<T>(_request: &Request<T>) -> Option<IpAddr> {
    None
}

fn maybe_socket_addr<T>(request: &Request<T>) -> Option<IpAddr> {
    request.extensions().get::<SocketAddr>().map(SocketAddr::ip)
}

/// Extract a client key from a peer `SocketAddr` request extension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IpKeyExtractor;

impl IpKeyExtractor {
    /// Construct the default extractor.
    pub const fn new() -> Self {
        Self
    }
}

impl KeyExtractor for IpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, request: &Request<T>) -> Result<Self::Key, RateLimitError> {
        maybe_connect_info(request)
            .or_else(|| maybe_socket_addr(request))
            .ok_or_else(|| {
                RateLimitError::KeyUnavailable(
                    String::from("peer_ip_unavailable"),
                    String::from("request extensions do not contain a peer socket address"),
                )
            })
    }
}
