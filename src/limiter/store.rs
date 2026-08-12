//! The core Store seam and returned usage.

use std::{future::Future, time::Duration};

use super::RateLimitError;

/// The operating mode to use when the rate-limit store fails.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StoreFailureMode {
    /// Build an error response without calling the inner service.
    #[default]
    Reject,
    /// Call the inner service without rate-limit metadata.
    Allow,
}

/// The usage and reset duration returned by a store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Total charged requests in the current window, including this increment.
    pub used: u64,
    /// Time remaining until the current window resets.
    pub reset_after: Duration,
}

/// An asynchronous fixed-window rate-limit store.
///
/// Implementations receive a complete, policy-scoped key and must treat it as opaque. For each
/// key, [`Store::increment`] must atomically create or increment one fixed window, start expiry on
/// the first increment, and leave that expiry unchanged on later increments. The returned
/// [`Usage`] includes the current increment, so `used` is at least one and `reset_after` is the
/// remaining duration of the active window.
///
/// A Store used by [`super::RateLimit`] must also implement [`Clone`], and clones of one Store value
/// must observe the same counter state. Independently constructed Store values may use separate
/// state. Backend failures are represented as [`RateLimitError::Store`].
pub trait Store {
    /// The concrete future returned by [`Store::increment`].
    type Future: Future<Output = Result<Usage, RateLimitError>>;

    /// Atomically increment `key` for the fixed `window`.
    fn increment(&self, key: &str, window: Duration) -> Self::Future;
}
