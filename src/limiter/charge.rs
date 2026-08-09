//! Charged-request implementation behind [`super::RateLimitFuture`].
//!
//! Owns scoped Key construction, quota evaluation into metadata-bearing
//! outcomes, and request context annotation.
//!
//! [`ChargeMetadata`] is the internal source of truth (`limit` + [`Usage`]).
//! [`RateLimitPolicy`] is the public projection written into request extensions.

use std::time::Duration;

use http::Request;

use super::{RateLimitConfig, error::RateLimitError, response::RateLimitFields, store::Usage};

/// One policy's rate-limit state exposed to a downstream handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimitPolicy {
    /// The policy identifier.
    pub policy_name: String,
    /// The resolved quota limit.
    pub limit: u64,
    /// Charged requests in the current window.
    pub used: u64,
    /// Remaining quota after the current request.
    pub remaining: u64,
    /// Time remaining until the window resets.
    pub reset_after: Duration,
}

/// Read-only rate-limit state carried in request extensions for allowed requests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RateLimitContext {
    /// The policies in the context.
    policies: Vec<RateLimitPolicy>,
}

impl RateLimitContext {
    /// Construct an empty context.
    pub const fn new() -> Self {
        Self { policies: Vec::new() }
    }

    /// Borrow all policy entries in composition order.
    pub fn policies(&self) -> &[RateLimitPolicy] {
        &self.policies
    }
}

/// Internal source state for one charged request.
///
/// Remaining quota is always derived from `limit` and [`Usage`]. The public
/// [`RateLimitPolicy`] snapshot is produced via [`ChargeMetadata::to_policy`].
#[derive(Clone, Debug)]
pub(super) struct ChargeMetadata {
    pub(super) policy_name: String,
    pub(super) limit: u64,
    pub(super) usage: Usage,
    pub(super) window: Duration,
    pub(super) rate_limit_fields: RateLimitFields,
}

impl ChargeMetadata {
    /// Project this source state into the public policy snapshot.
    fn to_policy(&self) -> RateLimitPolicy {
        RateLimitPolicy {
            policy_name: self.policy_name.clone(),
            limit: self.limit,
            used: self.usage.used,
            remaining: self.remaining(),
            reset_after: self.usage.reset_after,
        }
    }

    /// Return the remaining quota after the current request.
    pub(super) fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.usage.used)
    }
}

/// Quota evaluation outcome for one charged request.
pub(super) enum ChargeOutcome {
    /// The charged request remains within its quota.
    Allowed(ChargeMetadata),
    /// The charged request exceeds its quota.
    RateLimited(ChargeMetadata),
}

impl ChargeOutcome {
    /// Evaluate charged usage against the resolved quota limit.
    pub(super) fn evaluate(usage: Usage, limit: u64, config: &RateLimitConfig) -> Result<Self, RateLimitError> {
        if usage.used == 0 {
            return Err(RateLimitError::Store(
                String::from("invalid_usage"),
                String::from("rate-limit store returned zero usage"),
            ));
        }

        let metadata = ChargeMetadata {
            policy_name: config.policy_name.clone(),
            limit,
            usage,
            window: config.window,
            rate_limit_fields: config.rate_limit_fields,
        };
        Ok(if usage.used > limit {
            Self::RateLimited(metadata)
        } else {
            Self::Allowed(metadata)
        })
    }
}

/// Build the scoped Key passed toward a [`super::Store`].
pub(super) fn make_key(policy_name: &str, client_key: &str) -> String {
    format!("{}:{}", escape_key_part(policy_name), escape_key_part(client_key))
}

fn escape_key_part(value: &str) -> String {
    value.replace('%', "%25").replace(':', "%3A")
}

/// Append the public policy projection to the request extensions.
pub(super) fn append_context<B>(request: &mut Request<B>, metadata: &ChargeMetadata) {
    request
        .extensions_mut()
        .get_or_insert_default::<RateLimitContext>()
        .policies
        .push(metadata.to_policy());
}
