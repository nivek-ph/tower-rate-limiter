//! Builder and immutable configuration for the rate-limit layer.

use std::{fmt, sync::Arc, time::Duration};

use super::error::ConfigError;
use super::layer::RateLimitLayer;
use super::limit::LimitProvider;
use super::response::DefaultResponseFactory;
use super::store::{Store, StoreErrorAction};

/// The minimum window duration allowed.
const MINIMUM_WINDOW: Duration = Duration::from_millis(1);

/// A callback to encode the scoped key before passing it to the [`Store`].
pub(crate) type KeyEncoding = Box<dyn Fn(&str) -> String + Send + Sync>;

/// Builder for a [`RateLimitLayer`] with compile-time store/provider/factory types.
pub struct RateLimitBuilder<K, S = (), P = u64, F = DefaultResponseFactory> {
    key_extractor: K,
    store: S,
    limit_provider: P,
    response_factory: F,
    policy_name: String,
    window: Duration,
    key_encoding: Option<KeyEncoding>,
    store_error_action: StoreErrorAction,
    emit_headers: bool,
}

impl<K> RateLimitBuilder<K> {
    pub(crate) fn new(key_extractor: K) -> Self {
        Self {
            key_extractor,
            store: (),
            limit_provider: 1,
            response_factory: DefaultResponseFactory,
            policy_name: String::from("default-policy"),
            window: Duration::from_secs(60),
            key_encoding: None,
            store_error_action: StoreErrorAction::default(),
            emit_headers: true,
        }
    }
}

impl<K, S, P, F> RateLimitBuilder<K, S, P, F> {
    /// Inject the rate-limit store and update the builder's store type state.
    pub fn with_store<S2>(self, store: S2) -> RateLimitBuilder<K, S2, P, F> {
        RateLimitBuilder {
            key_extractor: self.key_extractor,
            store,
            limit_provider: self.limit_provider,
            response_factory: self.response_factory,
            policy_name: self.policy_name,
            window: self.window,
            key_encoding: self.key_encoding,
            store_error_action: self.store_error_action,
            emit_headers: self.emit_headers,
        }
    }

    /// Set a fixed quota limit and update the provider type state.
    pub fn limit(self, limit: u64) -> RateLimitBuilder<K, S, u64, F> {
        RateLimitBuilder {
            key_extractor: self.key_extractor,
            store: self.store,
            limit_provider: limit,
            response_factory: self.response_factory,
            policy_name: self.policy_name,
            window: self.window,
            key_encoding: self.key_encoding,
            store_error_action: self.store_error_action,
            emit_headers: self.emit_headers,
        }
    }

    /// Replace the fixed provider with a custom asynchronous limit provider.
    pub fn limit_provider<P2>(self, limit_provider: P2) -> RateLimitBuilder<K, S, P2, F> {
        RateLimitBuilder {
            key_extractor: self.key_extractor,
            store: self.store,
            limit_provider,
            response_factory: self.response_factory,
            policy_name: self.policy_name,
            window: self.window,
            key_encoding: self.key_encoding,
            store_error_action: self.store_error_action,
            emit_headers: self.emit_headers,
        }
    }

    /// Set the fixed-window duration.
    pub fn window(self, window: Duration) -> Self {
        Self { window, ..self }
    }

    /// Set the stable policy identifier used in the scoped key and response metadata.
    pub fn policy_name(self, policy_name: impl Into<String>) -> Self {
        Self {
            policy_name: policy_name.into(),
            ..self
        }
    }

    /// Encode the scoped key before passing it to the [`Store`].
    ///
    /// The callback runs in the middleware future's polling path. It must be deterministic,
    /// non-blocking, free of I/O, collision-resistant for the caller's key space, and non-panicking.
    /// Without this method, the complete scoped key is passed to the Store unchanged.
    pub fn with_key_encoding<E>(self, encoder: E) -> Self
    where
        E: Fn(&str) -> String + Send + Sync + 'static,
    {
        Self {
            key_encoding: Some(Box::new(encoder)),
            ..self
        }
    }

    /// Select the action to take when the Store returns an error.
    pub fn on_store_error(mut self, action: StoreErrorAction) -> Self {
        self.store_error_action = action;
        self
    }

    /// Enable or disable the two IETF RateLimit response fields.
    pub fn emit_headers(mut self, emit: bool) -> Self {
        self.emit_headers = emit;
        self
    }

    /// Replace the response factory and update its type state.
    pub fn response_factory<F2>(self, response_factory: F2) -> RateLimitBuilder<K, S, P, F2> {
        RateLimitBuilder {
            key_extractor: self.key_extractor,
            store: self.store,
            limit_provider: self.limit_provider,
            response_factory,
            policy_name: self.policy_name,
            window: self.window,
            key_encoding: self.key_encoding,
            store_error_action: self.store_error_action,
            emit_headers: self.emit_headers,
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.window < MINIMUM_WINDOW {
            return Err(ConfigError::WindowTooShort(self.window, MINIMUM_WINDOW));
        }
        if self.policy_name.is_empty() {
            return Err(ConfigError::EmptyPolicyName);
        }
        Ok(())
    }
}

impl<K> RateLimitLayer<K, (), u64, DefaultResponseFactory> {
    /// Start a typed rate-limit layer builder.
    pub fn builder(key_extractor: K) -> RateLimitBuilder<K> {
        RateLimitBuilder::new(key_extractor)
    }
}

impl<K, S, P, F> RateLimitBuilder<K, S, P, F>
where
    S: Store,
    P: LimitProvider,
{
    /// Validate the builder and produce a configured layer.
    pub fn build(self) -> Result<RateLimitLayer<K, S, P, F>, ConfigError> {
        self.validate()?;
        Ok(RateLimitLayer {
            key_extractor: self.key_extractor,
            store: self.store,
            limit_provider: self.limit_provider,
            response_factory: self.response_factory,
            config: Arc::new(RateLimitConfig {
                policy_name: self.policy_name,
                window: self.window,
                key_encoding: self.key_encoding,
                store_error_action: self.store_error_action,
                emit_headers: self.emit_headers,
            }),
        })
    }
}

/// Immutable configuration shared by every service produced from a layer.
pub(crate) struct RateLimitConfig {
    pub(crate) policy_name: String,
    pub(crate) window: Duration,
    pub(crate) key_encoding: Option<KeyEncoding>,
    pub(crate) store_error_action: StoreErrorAction,
    pub(crate) emit_headers: bool,
}

impl fmt::Debug for RateLimitConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RateLimitConfig")
            .field("policy_name", &self.policy_name)
            .field("window", &self.window)
            .field(
                "key_encoding",
                &self.key_encoding.as_ref().map(|_| "<callback>"),
            )
            .field("store_error_action", &self.store_error_action)
            .field("emit_headers", &self.emit_headers)
            .finish()
    }
}
