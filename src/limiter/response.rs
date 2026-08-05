//! Response reasons and response factories for middleware-originated outcomes.

use std::time::Duration;

use http::{HeaderValue, Request, Response, StatusCode, header::HeaderName};

use super::store::Usage;
use super::{RateLimitError, RateLimitMetadata};

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

/// Append rate-limit fields to a middleware or downstream response.
pub(crate) fn append_rate_limit_headers<B>(
    mut response: Response<B>,
    metadata: Option<RateLimitMetadata>,
) -> Response<B> {
    let Some(metadata) = metadata else {
        return response;
    };

    if metadata.emit_headers {
        append_header(
            &mut response,
            RATE_LIMIT_POLICY,
            &format!(
                r#""{policy_name}";q={limit};w={window}"#,
                policy_name = metadata.policy_name,
                limit = metadata.limit,
                window = ceil_seconds(metadata.window),
            ),
        );
        append_header(
            &mut response,
            RATE_LIMIT,
            &format!(
                r#""{policy_name}";r={remaining};t={window}"#,
                policy_name = metadata.policy_name,
                remaining = metadata.limit.saturating_sub(metadata.usage.used),
                window = ceil_seconds(metadata.usage.reset_after),
            ),
        );
    }

    if metadata.rate_limited {
        append_header(
            &mut response,
            RETRY_AFTER,
            &ceil_seconds(metadata.usage.reset_after).to_string(),
        );
    }

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
