# How it works

`tower-rate-limiter` separates rate-limit policy from application concerns through four narrow
interfaces.

## Client key extraction

`KeyExtractor` synchronously derives an application-owned client key from the request. The crate
does not decide whether callers are identified by an account, credential owner, peer address, or
another value.

`IpKeyExtractor` reads a peer `SocketAddr` extension and returns its `IpAddr`. It deliberately does
not interpret forwarding headers: proxy trust belongs at the application boundary.

## Quota resolution

`LimitProvider` asynchronously resolves a request's quota. Calling `.limit(n)` uses a fixed `u64`,
while a custom provider can select a quota from validated request state.

Quota resolution completes before the Store is charged. Key and quota failures always reject the
request and never fail open.

## Usage storage

`Store` atomically increments a scoped key and returns:

```text
Usage { used, reset_after }
```

The Layer scopes the client key with its policy name; the window remains a separate Store argument.
Use distinct policy names when policies must not share usage.

Store failures reject by default. Applications that explicitly prefer availability can select
`StoreFailureMode::Allow`; the inner service is then called without claiming quota metadata.

## Responses and context

`ResponseFactory` maps middleware outcomes to the application's response body, status, headers, and
logging policy. The default factory returns:

| Outcome | Status |
| --- | --- |
| Rate limited | `429 Too Many Requests` |
| Client key failure | `500 Internal Server Error` |
| Quota failure | `500 Internal Server Error` |
| Store failure | `503 Service Unavailable` |

Allowed requests receive `RateLimitContext` in their extensions. Nested Layers append policies
instead of overwriting context or response fields.

## Request bypass

`RateLimitBuilder::skip` accepts a synchronous predicate over the request head. A bypassed request
reaches the inner service without key extraction, quota resolution, Store usage, response fields, or
`RateLimitContext`.

Only use application-trusted headers or extensions in this predicate. Prefer a validated identity
extension over matching a raw credential.
