//! Error types for the rate limiter.

use std::time::Duration;

/// A failure produced while resolving or charging a rate-limit policy.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum RateLimitError {
    /// The client key could not be resolved.
    #[error("client key unavailable ({0}): {1}")]
    Key(String, String),
    /// The request's quota limit could not be resolved.
    #[error("rate-limit quota unavailable ({0}): {1}")]
    Quota(String, String),
    /// The rate-limit store could not charge the request reliably.
    #[error("rate-limit store unavailable ({0}): {1}")]
    Store(String, String),
}

/// Errors produced while validating a rate-limit builder.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The configured window is shorter than the one-millisecond minimum.
    #[error("rate-limit window {0:?} is shorter than the minimum {1:?}")]
    WindowTooShort(Duration, Duration),
    /// The policy identifier is invalid.
    #[error("empty policy name")]
    EmptyPolicyName,
}
