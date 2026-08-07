//! Builder and immutable configuration for the rate-limit layer.

use std::{fmt, sync::Arc, time::Duration};

use http::Request;

use super::{
    error::ConfigError,
    layer::RateLimitLayer,
    limit::LimitProvider,
    response::{DefaultResponseFactory, RateLimitFields},
    store::{Store, StoreFailureMode},
};

/// The minimum window duration allowed.
const MINIMUM_WINDOW: Duration = Duration::from_millis(1);

/// A callback to encode the scoped key before passing it to the [`Store`].
pub(crate) type KeyEncoder = Box<dyn Fn(&str) -> String + Send + Sync>;

/// A callback that decides whether a request bypasses rate limiting.
pub(crate) type SkipPredicate = Box<dyn Fn(&Request<()>) -> bool + Send + Sync>;

/// Check if the request should be skipped based on the skip predicate.
pub(crate) fn check_skip_predicate<B>(predicate: Option<&SkipPredicate>, request: Request<B>) -> (bool, Request<B>) {
    let Some(predicate) = predicate else {
        return (false, request);
    };

    // This is a hack to get the request head. maybe there's a better way to do this.
    let (parts, body) = request.into_parts();
    let request_head = Request::from_parts(parts, ());
    let should_skip = predicate(&request_head);
    let (parts, ()) = request_head.into_parts();

    (should_skip, Request::from_parts(parts, body))
}

/// Builder for a rate-limit layer with compile-time store/resolver/factory types.
pub struct RateLimitBuilder<K, S = (), P = u64, F = DefaultResponseFactory> {
    key_extractor: K,
    store: S,
    limit_provider: P,
    response_factory: F,
    config: RateLimitConfig,
}

impl<K> RateLimitBuilder<K> {
    pub(crate) fn new(key_extractor: K) -> Self {
        Self {
            key_extractor,
            store: (),
            limit_provider: 1,
            response_factory: DefaultResponseFactory,
            config: RateLimitConfig {
                policy_name: String::from("default-policy"),
                window: Duration::from_secs(60),
                key_encoder: None,
                skip_predicate: None,
                store_failure_mode: StoreFailureMode::default(),
                rate_limit_fields: RateLimitFields::default(),
            },
        }
    }
}

impl<K, S, P, F> RateLimitBuilder<K, S, P, F> {
    /// Inject the rate-limit store and update the builder's store type state.
    pub fn with_store<S2>(self, store: S2) -> RateLimitBuilder<K, S2, P, F> {
        let Self {
            key_extractor,
            limit_provider,
            response_factory,
            config,
            ..
        } = self;
        RateLimitBuilder {
            key_extractor,
            store,
            limit_provider,
            response_factory,
            config,
        }
    }

    /// Set a fixed quota limit and update the provider type state.
    pub fn limit(self, limit: u64) -> RateLimitBuilder<K, S, u64, F> {
        let Self {
            key_extractor,
            store,
            response_factory,
            config,
            ..
        } = self;
        RateLimitBuilder {
            key_extractor,
            store,
            limit_provider: limit,
            response_factory,
            config,
        }
    }

    /// Replace the fixed provider with a custom asynchronous limit provider.
    pub fn limit_provider<P2>(self, limit_provider: P2) -> RateLimitBuilder<K, S, P2, F> {
        let Self {
            key_extractor,
            store,
            response_factory,
            config,
            ..
        } = self;
        RateLimitBuilder {
            key_extractor,
            store,
            limit_provider,
            response_factory,
            config,
        }
    }

    /// Replace the response factory and update its type state.
    pub fn response_factory<F2>(self, response_factory: F2) -> RateLimitBuilder<K, S, P, F2> {
        let Self {
            key_extractor,
            store,
            limit_provider,
            config,
            ..
        } = self;
        RateLimitBuilder {
            key_extractor,
            store,
            limit_provider,
            response_factory,
            config,
        }
    }

    /// Set the fixed-window duration.
    pub fn window(mut self, window: Duration) -> Self {
        self.config.window = window;
        self
    }

    /// Set the stable policy identifier used in the scoped key and response metadata.
    pub fn policy_name(mut self, policy_name: impl Into<String>) -> Self {
        self.config.policy_name = policy_name.into();
        self
    }

    /// Encode the scoped key before passing it to the [`Store`].
    ///
    /// The callback runs in the middleware future's polling path. It must be deterministic,
    /// non-blocking, free of I/O, collision-resistant for the caller's key space, and
    /// non-panicking. Without this method, the complete scoped key is passed to the Store
    /// unchanged.
    pub fn with_key_encoder<E>(mut self, encoder: E) -> Self
    where
        E: Fn(&str) -> String + Send + Sync + 'static,
    {
        self.config.key_encoder = Some(Box::new(encoder));
        self
    }

    /// Bypass rate limiting when `predicate` returns `true` for the request.
    ///
    /// The predicate receives the request head and extensions with a unit body. It runs
    /// synchronously before client-key extraction and must be non-blocking, free of I/O, and
    /// non-panicking. A bypassed request calls the inner service without resolving a limit,
    /// charging the Store, or adding rate-limit context or response fields.
    pub fn skip<Predicate>(mut self, predicate: Predicate) -> Self
    where
        Predicate: Fn(&Request<()>) -> bool + Send + Sync + 'static,
    {
        self.config.skip_predicate = Some(Box::new(predicate));
        self
    }

    /// Select the mode to use when the Store fails.
    pub fn store_failure_mode(mut self, mode: StoreFailureMode) -> Self {
        self.config.store_failure_mode = mode;
        self
    }

    /// Select the Rate Limit Fields revision emitted in responses.
    pub fn rate_limit_fields(mut self, fields: RateLimitFields) -> Self {
        self.config.rate_limit_fields = fields;
        self
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.config.window < MINIMUM_WINDOW {
            return Err(ConfigError::WindowTooShort(self.config.window, MINIMUM_WINDOW));
        }
        if self.config.policy_name.is_empty() {
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
            config: Arc::new(self.config),
        })
    }
}

/// Immutable configuration shared by every service produced from a layer.
pub(crate) struct RateLimitConfig {
    /// The stable policy identifier used in the scoped key and response metadata.
    pub(crate) policy_name: String,
    /// The fixed-window duration.
    pub(crate) window: Duration,
    /// Encode the scoped key before passing it to the [`Store`].
    pub(crate) key_encoder: Option<KeyEncoder>,
    /// Decide whether a request bypasses rate limiting.
    pub(crate) skip_predicate: Option<SkipPredicate>,
    /// Select the mode to use when the Store fails.
    pub(crate) store_failure_mode: StoreFailureMode,
    /// Select the [`RateLimitFields`] revision emitted in responses.
    pub(crate) rate_limit_fields: RateLimitFields,
}

impl fmt::Debug for RateLimitConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimitConfig")
            .field("policy_name", &self.policy_name)
            .field("window", &self.window)
            .field("has_key_encoder", &self.key_encoder.is_some())
            .field("has_skip_predicate", &self.skip_predicate.is_some())
            .field("store_failure_mode", &self.store_failure_mode)
            .field("rate_limit_fields", &self.rate_limit_fields)
            .finish()
    }
}
