//! The core Store seam and returned usage.

use std::{future::Future, time::Duration};

use super::RateLimitError;

/// The action to take when the rate-limit store fails.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StoreErrorAction {
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
pub trait Store: Clone + Send + Sync + 'static {
    /// The concrete future returned by [`Store::increment`].
    type Future: Future<Output = Result<Usage, RateLimitError>> + Send + 'static;

    /// Atomically increment `key` for the fixed `window`.
    fn increment(&self, key: &str, window: Duration) -> Self::Future;
}
