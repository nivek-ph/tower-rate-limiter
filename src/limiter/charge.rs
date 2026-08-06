//! Charged-request implementation behind [`super::RateLimitFuture`].
//!
//! Owns scoped Key construction, quota evaluation into metadata-bearing
//! outcomes, request context annotation, and RateLimit response fields.
//!
//! [`ChargeMetadata`] is the internal source of truth (`limit` + [`Usage`]).
//! [`RateLimitPolicy`] is the public projection written into request extensions.

use std::time::Duration;

use http::{HeaderValue, Request, Response, StatusCode, header::HeaderName};

use super::RateLimitConfig;
use super::error::RateLimitError;
use super::store::Usage;

const RATE_LIMIT: HeaderName = HeaderName::from_static("ratelimit");
const RATE_LIMIT_POLICY: HeaderName = HeaderName::from_static("ratelimit-policy");
const RETRY_AFTER: HeaderName = HeaderName::from_static("retry-after");

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

/// The structured reason passed to a [`ResponseFactory`].
#[derive(Debug)]
pub enum ResponseReason {
    /// The request exceeded its resolved quota.
    RateLimited(u64, Usage),
    /// The middleware could not resolve or charge the request's policy.
    Error(RateLimitError),
}

impl ResponseReason {
    /// Return the default HTTP status for this reason.
    pub const fn status_code(&self) -> StatusCode {
        match self {
            Self::RateLimited(_, _) => StatusCode::TOO_MANY_REQUESTS,
            Self::Error(RateLimitError::KeyUnavailable(_, _))
            | Self::Error(RateLimitError::LimitUnavailable(_, _)) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::Error(RateLimitError::StoreUnavailable(_, _)) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

/// A factory for responses to rate-limit middleware outcomes.
pub trait ResponseFactory<B>: Clone + Send + Sync {
    /// Build a response using the original request and structured reason.
    fn build(&self, request: Request<B>, reason: ResponseReason) -> Response<B>;
}

/// The default empty-body response factory.
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct DefaultResponseFactory;

impl<B> ResponseFactory<B> for DefaultResponseFactory
where
    B: Default,
{
    fn build(&self, _request: Request<B>, reason: ResponseReason) -> Response<B> {
        let mut response = Response::new(B::default());
        *response.status_mut() = reason.status_code();
        response
    }
}

/// Internal source state for one charged request.
///
/// Remaining quota is always derived from `limit` and [`Usage`]. The public
/// [`RateLimitPolicy`] snapshot is produced via [`ChargeMetadata::to_policy`].
#[derive(Clone, Debug)]
pub(super) struct ChargeMetadata {
    policy_name: String,
    limit: u64,
    usage: Usage,
    window: Duration,
    emit_headers: bool,
}

impl ChargeMetadata {
    /// Project this source state into the public policy snapshot.
    fn to_policy(&self) -> RateLimitPolicy {
        RateLimitPolicy {
            policy_name: self.policy_name.clone(),
            limit: self.limit,
            used: self.usage.used,
            remaining: self.limit.saturating_sub(self.usage.used),
            reset_after: self.usage.reset_after,
        }
    }

    /// Build the structured reason passed to a [`ResponseFactory`].
    pub(super) fn rate_limited_reason(&self) -> ResponseReason {
        ResponseReason::RateLimited(self.limit, self.usage)
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
    pub(super) fn evaluate(
        usage: Usage,
        limit: u64,
        config: &RateLimitConfig,
    ) -> Result<Self, RateLimitError> {
        if usage.used == 0 {
            return Err(RateLimitError::StoreUnavailable(
                String::from("invalid_usage"),
                String::from("rate-limit store returned zero usage"),
            ));
        }

        let metadata = ChargeMetadata {
            policy_name: config.policy_name.clone(),
            limit,
            usage,
            window: config.window,
            emit_headers: config.emit_headers,
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
    format!(
        "{}:{}",
        escape_key_part(policy_name),
        escape_key_part(client_key)
    )
}

fn escape_key_part(value: &str) -> String {
    value.replace('%', "%25").replace(':', "%3A")
}

/// Append the public policy projection to the request extensions.
pub(super) fn append_context<B>(request: &mut Request<B>, metadata: &ChargeMetadata) {
    request
        .extensions_mut()
        .get_or_insert_default::<RateLimitContext>()
        .push(metadata.to_policy());
}

/// Decorate an allowed (or fail-open) inner response with Rate Limit Fields.
///
/// `None` means fail-open: no fields are written. When present, fields follow
/// `emit_headers`; `Retry-After` is never added on this path.
pub(super) fn append_inner_response_headers<B>(
    response: Response<B>,
    metadata: Option<ChargeMetadata>,
) -> Response<B> {
    match metadata {
        Some(metadata) => append_rate_limit_fields(response, &metadata),
        None => response,
    }
}

/// Decorate a rate-limited middleware response with Rate Limit Fields and Retry-After.
///
/// Rate Limit Fields still respect `emit_headers`. `Retry-After` is always added.
pub(super) fn append_rate_limited_response_headers<B>(
    response: Response<B>,
    metadata: ChargeMetadata,
) -> Response<B> {
    let mut response = append_rate_limit_fields(response, &metadata);
    append_header(
        &mut response,
        RETRY_AFTER,
        &ceil_seconds(metadata.usage.reset_after).to_string(),
    );
    response
}

/// Shared Rate Limit / RateLimit-Policy field writer.
fn append_rate_limit_fields<B>(
    mut response: Response<B>,
    metadata: &ChargeMetadata,
) -> Response<B> {
    if !metadata.emit_headers {
        return response;
    }

    let name = &metadata.policy_name;
    append_header(
        &mut response,
        RATE_LIMIT_POLICY,
        &format!(
            r#""{name}";q={};w={}"#,
            metadata.limit,
            ceil_seconds(metadata.window)
        ),
    );
    append_header(
        &mut response,
        RATE_LIMIT,
        &format!(
            r#""{name}";r={};t={}"#,
            metadata.limit.saturating_sub(metadata.usage.used),
            ceil_seconds(metadata.usage.reset_after)
        ),
    );
    response
}

fn append_header<B>(response: &mut Response<B>, name: HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        response.headers_mut().append(name, value);
    }
}

fn ceil_seconds(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() != 0))
        .max(1)
}
