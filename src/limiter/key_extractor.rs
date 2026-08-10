//! Request-facing client-key extraction interfaces.

use std::{
    fmt::{Debug, Display},
    hash::Hash,
    net::IpAddr,
};

use http::Request;

use super::error::RateLimitError;

/// Extract a client key synchronously from a request.
pub trait KeyExtractor: Clone + Send + Sync {
    /// The type of the key.
    type Key: Clone + Hash + Eq + Debug + Display;

    /// Extract the key from the request.
    fn extract<T>(&self, request: &Request<T>) -> Result<Self::Key, RateLimitError>;
}

/// Extract a client key from a socket-address request extension.
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
        http_extract::extract_socket_ip(request).ok_or_else(|| {
            RateLimitError::Key(
                String::from("socket_ip_unavailable"),
                String::from("request extensions do not contain a socket ip address"),
            )
        })
    }
}

/// Extract a client IP key from supported client-IP headers, falling back to the socket IP.
///
/// Header-derived addresses are not authenticated. Only use this extractor when every accepted
/// client-IP header is removed or overwritten by a trusted proxy. If no client-IP header is
/// present, the extractor falls back to an Axum `ConnectInfo<SocketAddr>` or a generic
/// `SocketAddr` request extension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClientIpKeyExtractor;

impl ClientIpKeyExtractor {
    /// Construct the header-aware client IP extractor.
    pub const fn new() -> Self {
        Self
    }
}

impl KeyExtractor for ClientIpKeyExtractor {
    type Key = IpAddr;

    /// Extract the client IP from the request, falling back to the socket IP.
    fn extract<T>(&self, request: &Request<T>) -> Result<Self::Key, RateLimitError> {
        http_extract::extract_proxy_client_ip(request)
            .map_err(|error| RateLimitError::Key(String::from("invalid_client_ip"), error.to_string()))?
            .ok_or_else(|| {
                RateLimitError::Key(
                    String::from("client_ip_unavailable"),
                    String::from("request does not contain a client or socket IP address"),
                )
            })
    }
}
