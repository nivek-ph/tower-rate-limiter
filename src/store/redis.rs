//! Redis rate-limit store, enabled by the `redis` Cargo feature.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use redis::{Script, aio::MultiplexedConnection};

use crate::{RateLimitError, Store, Usage};

const REDIS_PREFIX: &str = "rl:";

const INCREMENT_SCRIPT: &str = r#"
local count = redis.call('INCR', KEYS[1])
if count == 1 then
    redis.call('PEXPIRE', KEYS[1], ARGV[1])
end
return {count, redis.call('PTTL', KEYS[1])}
"#;

/// Redis-backed implementation of the common fixed-window [`Store`] seam.
///
/// The connection is supplied by the caller and is cloned for each increment. A Redis
/// `MultiplexedConnection` clone shares its underlying connection and does not transfer
/// connection lifecycle ownership to this adapter.
#[derive(Clone, Debug)]
pub struct RedisStore {
    connection: MultiplexedConnection,
    namespace: Option<String>,
}

impl RedisStore {
    /// Construct a store from an established Redis multiplexed connection.
    pub fn new(connection: MultiplexedConnection) -> Self {
        Self {
            connection,
            namespace: None,
        }
    }

    /// Add an optional namespace. Empty namespaces are treated as absent.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Format the Redis transport key for this store's optional namespace.
    fn redis_key(&self, key: &str) -> String {
        format_redis_key(self.namespace.as_deref(), key)
    }
}

impl Store for RedisStore {
    type Future = RedisStoreFuture;

    fn increment(&self, key: &str, window: Duration) -> Self::Future {
        let window_millis = match checked_window_millis(window) {
            Ok(window_millis) => window_millis,
            Err(error) => return RedisStoreFuture::error(error),
        };
        let redis_key = self.redis_key(key);
        let mut connection = self.connection.clone();

        RedisStoreFuture::new(async move {
            let script = Script::new(INCREMENT_SCRIPT);
            let mut invocation = script.key(redis_key);
            invocation.arg(window_millis);
            let result: (i64, i64) =
                invocation
                    .invoke_async(&mut connection)
                    .await
                    .map_err(|error: redis::RedisError| {
                        RateLimitError::Store(String::from("redis_command_failed"), error.to_string())
                    })?;
            usage_from_script_result(result)
        })
    }
}

/// Convert the atomic Lua result `(INCR count, PTTL milliseconds)` into [`Usage`].
///
/// Redis reports `-1` for a persistent key and `-2` for a missing key. Both, as well as a
/// zero TTL, are errors because the fixed window cannot be trusted without a positive TTL.
fn usage_from_script_result((used, reset_after_millis): (i64, i64)) -> Result<Usage, RateLimitError> {
    if used < 1 {
        return Err(RateLimitError::Store(
            String::from("redis_invalid_count"),
            format!("Redis returned invalid usage {used}"),
        ));
    }
    if reset_after_millis <= 0 {
        return Err(RateLimitError::Store(
            String::from("redis_invalid_pttl"),
            format!("Redis key has invalid PTTL {reset_after_millis}"),
        ));
    }

    Ok(Usage {
        used: used as u64,
        reset_after: Duration::from_millis(reset_after_millis as u64),
    })
}

/// Named future returned by [`RedisStore::increment`](Store::increment).
pub struct RedisStoreFuture {
    inner: Pin<Box<dyn Future<Output = Result<Usage, RateLimitError>> + Send>>,
}

impl RedisStoreFuture {
    /// Create a future for one Redis usage increment.
    fn new<F>(future: F) -> Self
    where
        F: Future<Output = Result<Usage, RateLimitError>> + Send + 'static,
    {
        Self {
            inner: Box::pin(future),
        }
    }

    /// Create a new RedisStoreFuture from an error.
    fn error(error: RateLimitError) -> Self {
        Self::new(std::future::ready(Err(error)))
    }
}

impl Unpin for RedisStoreFuture {}

impl Future for RedisStoreFuture {
    type Output = Result<Usage, RateLimitError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(context)
    }
}

fn checked_window_millis(window: Duration) -> Result<i64, RateLimitError> {
    if window.is_zero() {
        return Err(RateLimitError::Store(
            String::from("redis_invalid_window"),
            String::from("Redis window must be non-zero"),
        ));
    }
    let window_millis = window.as_millis();
    if window_millis > i64::MAX as u128 {
        return Err(RateLimitError::Store(
            String::from("redis_window_too_large"),
            String::from("Redis window is too large"),
        ));
    }
    Ok(window_millis as i64)
}

/// Redis transport naming for a scoped Key already owned by the Rate Limiter.
fn format_redis_key(namespace: Option<&str>, key: &str) -> String {
    match namespace.filter(|value| !value.is_empty()) {
        Some(namespace) => format!("{namespace}:{REDIS_PREFIX}{key}"),
        None => format!("{REDIS_PREFIX}{key}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_store_implements_the_common_store_seam() {
        fn assert_store<T: Store>() {}
        assert_store::<RedisStore>();
    }

    #[test]
    fn script_result_requires_positive_usage_and_ttl() {
        let usage = usage_from_script_result((4, 1_500)).expect("valid Redis script result");
        assert_eq!(
            usage,
            Usage {
                used: 4,
                reset_after: Duration::from_millis(1_500),
            }
        );

        assert!(matches!(
            usage_from_script_result((0, 1_500)),
            Err(RateLimitError::Store(code, _)) if code == "redis_invalid_count"
        ));
        assert!(matches!(
            usage_from_script_result((4, 0)),
            Err(RateLimitError::Store(code, _)) if code == "redis_invalid_pttl"
        ));
        assert!(matches!(
            usage_from_script_result((4, -1)),
            Err(RateLimitError::Store(code, _)) if code == "redis_invalid_pttl"
        ));
    }

    #[test]
    fn redis_transport_key_keeps_namespace_private_to_the_adapter() {
        assert_eq!(format_redis_key(None, "policy:client"), "rl:policy:client");
        assert_eq!(
            format_redis_key(Some("tenant"), "policy:client"),
            "tenant:rl:policy:client"
        );
        assert_eq!(format_redis_key(Some(""), "policy:client"), "rl:policy:client");
    }

    #[test]
    fn window_must_be_representable_as_positive_redis_milliseconds() {
        assert!(matches!(
            checked_window_millis(Duration::ZERO),
            Err(RateLimitError::Store(code, _)) if code == "redis_invalid_window"
        ));
        assert_eq!(
            checked_window_millis(Duration::from_millis(1)).expect("one millisecond"),
            1
        );
    }
}
