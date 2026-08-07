<div class="hero">
  <p class="eyebrow">Tower-first · keyed · fixed window</p>
  <h1>Rate limiting that fits your service</h1>
  <p class="hero-copy">
    Extract a client key, resolve its quota, and charge an explicit Store—without coupling your
    middleware to a web framework or runtime.
  </p>
  <div class="hero-actions">
    <a class="button primary" href="getting-started.html">Get started</a>
    <a class="button secondary" href="https://github.com/nivek-ph/tower-rate-limiter">View on GitHub</a>
  </div>
</div>

<div class="feature-grid">
  <div class="feature-card">
    <h3>Tower-first</h3>
    <p>Compose a regular Layer around any compatible Tower service. Axum remains optional.</p>
  </div>
  <div class="feature-card">
    <h3>Your identity policy</h3>
    <p>Use an IP address, authenticated account, API key owner, or any application-owned key.</p>
  </div>
  <div class="feature-card">
    <h3>Explicit storage</h3>
    <p>Start in memory, share counters through Redis, or implement the narrow Store interface.</p>
  </div>
</div>

## A small core with clear seams

Every charged request moves through four application-facing components:

```text
Request
  → KeyExtractor
  → LimitProvider
  → Store
  → ResponseFactory
```

The first requests in a fixed window reach the inner service. Once the resolved quota is exhausted,
the middleware returns `429 Too Many Requests` immediately. Allowed and rejected responses expose
standard `RateLimit` fields, and allowed requests carry a `RateLimitContext` extension for downstream
middleware.

## Choose only what you need

| Cargo feature | What it adds |
| --- | --- |
| `memory` (default) | A runtime-independent, process-local `MemoryStore` |
| `axum` | Axum `ConnectInfo<SocketAddr>` support in `IpKeyExtractor` |
| `redis` | A `RedisStore` using an existing multiplexed connection |

With `default-features = false`, the core does not pull in Axum, Redis, Tokio, or SHA-2.

<div class="next-step">
  <strong>Ready to add a limiter?</strong>
  <a href="getting-started.html">Build your first Tower service →</a>
</div>
