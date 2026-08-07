//! Middleware response construction and Rate Limit Fields.
//!
//! Owns the public response customization seam and the private finalization
//! path shared by every response produced before the inner service is called.

use std::time::Duration;

use http::{HeaderValue, Request, Response, StatusCode, header::HeaderName};

use super::{charge::ChargeMetadata, error::RateLimitError, store::Usage};

const RATE_LIMIT: HeaderName = HeaderName::from_static("ratelimit");
const RATE_LIMIT_POLICY: HeaderName = HeaderName::from_static("ratelimit-policy");
const RETRY_AFTER: HeaderName = HeaderName::from_static("retry-after");

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
            Self::Error(RateLimitError::Key(_, _)) | Self::Error(RateLimitError::Quota(_, _)) => {
                StatusCode::INTERNAL_SERVER_ERROR
            },
            Self::Error(RateLimitError::Store(_, _)) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

/// A factory for responses to rate-limit middleware results.
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

/// A response produced by the middleware before the inner service is called.
///
/// Response construction and rate-limit field decoration stay deferred until
/// the future's common ready state.
pub(super) enum MiddlewareResponse<B> {
    RateLimited(Request<B>, ChargeMetadata),
    Error(Request<B>, RateLimitError),
}

impl<B> MiddlewareResponse<B> {
    /// Retain an error for common finalization.
    pub(super) fn error(request: Request<B>, error: RateLimitError) -> Self {
        Self::Error(request, error)
    }

    /// Retain a rate-limited metadata for common finalization.
    pub(super) fn rate_limited(request: Request<B>, metadata: ChargeMetadata) -> Self {
        Self::RateLimited(request, metadata)
    }

    /// Build and decorate the final HTTP response through one middleware path.
    pub(super) fn finalize<F>(self, factory: &F) -> Response<B>
    where
        F: ResponseFactory<B>,
    {
        match self {
            Self::RateLimited(request, metadata) => {
                let reason = ResponseReason::RateLimited(metadata.limit, metadata.usage);
                let response = factory.build(request, reason);

                append_rate_limited_response_headers(response, metadata)
            },
            Self::Error(request, error) => factory.build(request, ResponseReason::Error(error)),
        }
    }
}

/// Decorate an allowed (or fail-open) inner response with Rate Limit Fields.
///
/// `None` means no quota metadata is available after bypass or fail-open, so no fields are
/// written. When present, fields follow `emit_headers`; `Retry-After` is never added on this path.
pub(super) fn append_inner_response_headers<B>(response: Response<B>, metadata: Option<ChargeMetadata>) -> Response<B> {
    match metadata {
        Some(metadata) => append_rate_limit_fields(response, &metadata),
        None => response,
    }
}

/// Decorate a rate-limited middleware response with Rate Limit Fields and Retry-After.
///
/// Rate Limit Fields still respect `emit_headers`. `Retry-After` is always added.
fn append_rate_limited_response_headers<B>(response: Response<B>, metadata: ChargeMetadata) -> Response<B> {
    let mut response = append_rate_limit_fields(response, &metadata);
    append_header(
        &mut response,
        RETRY_AFTER,
        &ceil_seconds(metadata.usage.reset_after).to_string(),
    );
    response
}

/// Shared Rate Limit / RateLimit-Policy field writer.
fn append_rate_limit_fields<B>(mut response: Response<B>, metadata: &ChargeMetadata) -> Response<B> {
    if !metadata.emit_headers {
        return response;
    }

    let name = &metadata.policy_name;
    append_header(
        &mut response,
        RATE_LIMIT_POLICY,
        &format!(r#""{name}";q={};w={}"#, metadata.limit, ceil_seconds(metadata.window)),
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

/// Append a header to the response.
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
