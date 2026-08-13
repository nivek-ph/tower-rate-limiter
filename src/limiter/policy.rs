//! Policy state and response metadata behind [`super::future::ResponseFuture`].
//!
//! Owns scoped Key construction, Policy construction, response metadata, and
//! request context annotation.
//!
//! [`Policy`] is the source of truth for one charged request.
//! [`ResponseMetadata`] adds the private response-field configuration.

use std::time::Duration;

use http::Request;

use super::{error::RateLimitError, response::RateLimitFields, store::Usage};

/// A policy's rate-limit state exposed to a downstream handler.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// The policy identifier.
    pub name: String,
    /// The resolved quota limit.
    pub limit: u64,
    /// The policy's window.
    pub window: Duration,
    /// Used requests in the current window.
    pub used: u64,
    /// Time remaining until the window resets.
    pub reset_after: Duration,
}

impl Policy {
    /// Build one policy state from a successful Store result.
    pub(super) fn from_usage(name: String, window: Duration, limit: u64, usage: Usage) -> Result<Self, RateLimitError> {
        if usage.used == 0 {
            return Err(RateLimitError::Store(
                String::from("invalid_usage"),
                String::from("rate-limit store returned zero usage"),
            ));
        }

        Ok(Self {
            name,
            limit,
            window,
            used: usage.used,
            reset_after: usage.reset_after,
        })
    }

    /// Return the remaining quota after the current request.
    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }

    /// Return true if the policy is rate limited.
    pub fn is_rate_limited(&self) -> bool {
        self.used > self.limit
    }
}

/// Read-only rate-limit state carried in request extensions for allowed requests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RateLimitContext {
    /// The policies in the context.
    policies: Vec<Policy>,
}

impl RateLimitContext {
    /// Construct an empty context.
    pub const fn new() -> Self {
        Self { policies: Vec::new() }
    }

    /// Borrow all policy entries in composition order.
    pub fn policies(&self) -> &[Policy] {
        &self.policies
    }
}

/// Internal response-field configuration for one charged policy.
#[derive(Clone, Debug)]
pub(super) struct ResponseMetadata {
    pub(super) policy: Policy,
    pub(super) fields: RateLimitFields,
}

impl ResponseMetadata {
    pub(super) fn new(policy: Policy, fields: RateLimitFields) -> Self {
        Self { policy, fields }
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
pub(super) fn append_context<B>(request: &mut Request<B>, metadata: &ResponseMetadata) {
    request
        .extensions_mut()
        .get_or_insert_default::<RateLimitContext>()
        .policies
        .push(metadata.policy.clone());
}
