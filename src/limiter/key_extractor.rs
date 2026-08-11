//! Request-facing client-key extraction interfaces.

use std::{
    fmt::{Debug, Display},
    hash::Hash,
    net::IpAddr,
    sync::Arc,
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

/// Extract an IP client key from only the socket peer.
///
/// Use this extractor for direct connections or whenever the transport peer itself should identify
/// the caller. It never reads forwarding Headers. Choose one built-in IP extractor for the
/// application's network topology; do not layer this extractor with another one.
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
/// Use this extractor only when a platform or deployment boundary already guarantees that every
/// accepted client-IP Header is trustworthy, such as an application reachable only through a proxy
/// that removes or overwrites those Headers. This extractor does not validate the socket peer. If no
/// client-IP Header is present, it falls back to an Axum `ConnectInfo<SocketAddr>` or a generic
/// `SocketAddr` request extension. Choose one built-in IP extractor for the application's network
/// topology; do not layer this extractor with another one.
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

/// Extract a client IP key only when the socket peer satisfies an application trust policy.
///
/// Use this extractor when trusted proxies and direct or untrusted peers may reach the same
/// application. It requires the socket peer and validates that peer with the synchronous policy
/// supplied to [`Self::new`] before reading any forwarding Header. Choose one built-in IP extractor
/// for the application's network topology; do not layer this extractor with another one.
///
/// An untrusted peer always uses its socket IP and all forwarding Headers are ignored. A trusted
/// peer uses the same Header order and strict parsing as [`ClientIpKeyExtractor`], falling back to
/// the peer when no supported Header is present.
///
/// The policy establishes which transport peers may assert a client address; Header parsing does
/// not authenticate the value. Applications must still ensure every trusted proxy removes or
/// overwrites each supported client-IP Header.
#[derive(Clone)]
pub struct TrustedProxyClientIpKeyExtractor {
    is_trusted_proxy: Arc<dyn Fn(IpAddr) -> bool + Send + Sync>,
}

impl TrustedProxyClientIpKeyExtractor {
    /// Construct an extractor with an application-defined trusted-peer policy.
    pub fn new<F>(is_trusted_proxy: F) -> Self
    where
        F: Fn(IpAddr) -> bool + Send + Sync + 'static,
    {
        Self {
            is_trusted_proxy: Arc::new(is_trusted_proxy),
        }
    }
}

impl KeyExtractor for TrustedProxyClientIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, request: &Request<T>) -> Result<Self::Key, RateLimitError> {
        let peer = http_extract::extract_socket_ip(request).ok_or_else(|| {
            RateLimitError::Key(
                String::from("socket_ip_unavailable"),
                String::from("request extensions do not contain a socket ip address"),
            )
        })?;

        if !(self.is_trusted_proxy)(peer) {
            return Ok(peer);
        }

        http_extract::extract_client_ip(request.headers())
            .map(|client_ip| client_ip.unwrap_or(peer))
            .map_err(|error| RateLimitError::Key(String::from("invalid_client_ip"), error.to_string()))
    }
}
