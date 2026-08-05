//! The core Store seam, scoped key construction, and returned usage.

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

/// The fixed-window decision for one charged request.
pub(crate) enum RateLimitDecision {
    /// The charged request remains within its quota.
    Allowed(Usage),
    /// The charged request exceeds its quota.
    RateLimited(Usage),
}

impl Usage {
    /// Evaluate this charged usage against the resolved quota limit.
    pub(crate) fn evaluate(self, limit: u64) -> Result<RateLimitDecision, RateLimitError> {
        if self.used == 0 {
            return Err(RateLimitError::StoreUnavailable(
                String::from("invalid_usage"),
                String::from("rate-limit store returned zero usage"),
            ));
        }

        if self.used <= limit {
            Ok(RateLimitDecision::Allowed(self))
        } else {
            Ok(RateLimitDecision::RateLimited(self))
        }
    }
}

/// An asynchronous fixed-window rate-limit store.
pub trait Store: Clone + Send + Sync + 'static {
    /// The concrete future returned by [`Store::increment`].
    type Future: Future<Output = Result<Usage, RateLimitError>> + Send + 'static;

    /// Atomically increment `key` for the fixed `window`.
    fn increment(&self, key: &str, window: Duration) -> Self::Future;
}

/// Build the scoped key passed to a [`Store`].
pub(crate) fn make_key(policy_name: &str, key: &str) -> String {
    format!("{}:{}", escape_key_part(policy_name), escape_key_part(key),)
}

fn escape_key_part(value: &str) -> String {
    value.replace('%', "%25").replace(':', "%3A")
}
