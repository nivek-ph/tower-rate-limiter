//! Request extension context and internal response metadata.

use std::time::Duration;

use http::Request;

use super::store::Usage;

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

/// Typed rate-limit state carried in request extensions for allowed requests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RateLimitContext {
    /// The policies in the context.
    policies: Vec<RateLimitPolicy>,
}

impl RateLimitContext {
    /// Construct an empty context.
    pub const fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    /// Borrow all policy entries in composition order.
    pub fn policies(&self) -> &[RateLimitPolicy] {
        &self.policies
    }

    /// Append one policy entry.
    pub fn push(&mut self, entry: RateLimitPolicy) {
        self.policies.push(entry);
    }
}

/// Rate-limit metadata carried in request extensions for allowed requests.
#[derive(Clone, Debug)]
pub(crate) struct RateLimitMetadata {
    /// The policy identifier.
    pub policy_name: String,
    /// The resolved quota limit.
    pub limit: u64,
    /// The current usage.
    pub usage: Usage,
    /// The window duration.
    pub window: Duration,
    /// Whether to emit headers.
    pub emit_headers: bool,
    /// Whether the request was rate-limited.
    pub rate_limited: bool,
}

/// Append the rate-limit metadata to the request extensions.
pub(crate) fn append_context<B>(request: &mut Request<B>, metadata: &RateLimitMetadata) {
    let policy = RateLimitPolicy {
        policy_name: metadata.policy_name.clone(),
        limit: metadata.limit,
        used: metadata.usage.used,
        remaining: metadata.limit.saturating_sub(metadata.usage.used),
        reset_after: metadata.usage.reset_after,
    };
    request
        .extensions_mut()
        .get_or_insert_default::<RateLimitContext>()
        .push(policy);
}
