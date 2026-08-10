# Production guide

A rate limiter sits on a trust and availability boundary. Before deploying, make identity, counter
sharing, failure behavior, and rollout compatibility explicit.

## Deployment checklist

- Give every semantically distinct policy a stable, non-empty name.
- Extract identity from validated application state or a verified peer address.
- Define the trusted-proxy boundary before reading forwarding headers.
- Use a shared Store when the quota must apply across replicas.
- Decide whether Store failure rejects or fails open for each policy.
- Apply timeouts and health monitoring to externally backed quota resolution and storage.
- Verify emitted fields and `429` behavior from outside the service.
- Avoid secrets in keys, error messages, response bodies, and logs.

## Identity behind proxies

`IpKeyExtractor` reads the socket peer supplied in request extensions. Behind a reverse proxy, that
peer is normally the proxy, so all clients may collapse into one key.

Do not fix this by trusting `X-Forwarded-For` from every request. The deployment must define which
proxies are trusted and ensure they replace or sanitize client-supplied forwarding values. Then an
application-owned extractor can derive a forwarded client address. If that guarantee cannot be
made, use an authenticated identity or the direct peer address.

## Multiple replicas

`MemoryStore` is process-local. If three replicas each allow 100 requests, a caller routed across
all three may receive roughly 300 requests per window. Use `RedisStore` or an application-provided
shared Store when the limit must be global across replicas.

Make load-balancer behavior part of the decision. Sticky routing may reduce the difference but does
not make process-local state durable or authoritative.

## Timeouts and failure policy

The limiter awaits a custom `LimitProvider` and Store future. Bound remote work with the
application's timeout strategy; an unbounded dependency call can hold the request even when the
inner service was ready.

Monitor at least:

- rate-limited responses by policy;
- key, quota, and Store failures by stable error code;
- fail-open events when `StoreFailureMode::Allow` is used;
- latency of remote quota and Store operations;
- Redis connectivity and command failures.

Fail-open responses intentionally contain no quota metadata. Record this path in application
observability without exposing raw keys or credentials. Enabling the `tracing` Cargo feature emits
one structured event for every Store failure on both fail-open and reject paths. Events default to
`WARN`, and each policy may configure another level through the builder. Applications can use the
stable target and fields for filtering, counting, and alerting.

## Policy changes and rollout

Changing a limit keeps the same active counter identity. Changing the policy name or key encoder
creates a different identity, so new requests will not see the old counter.

During a rolling deployment, replicas with mismatched configuration may emit different fields or
charge different counters. Coordinate changes to:

- policy names;
- window durations;
- key extraction rules;
- key encoding;
- Store namespaces;
- rate-limit field revision.

If a policy's window changes, use a new policy name to prevent one logical counter from being used
with incompatible window assumptions.

## Smoke test

For a test policy with limit `2`, send three requests using the same client identity:

```text
request 1 -> inner response; remaining 1
request 2 -> inner response; remaining 0
request 3 -> 429; Retry-After present
```

Then repeat with a different identity and confirm it begins at its own quota. In a replicated
deployment, alternate requests across replicas to verify they share one Store counter. Finally,
exercise the chosen Store failure mode in a controlled environment.

## Intentional scope

Version 0.1 focuses on fixed-window request counting. It does not provide sliding windows, token
buckets, weighted requests, refunds, Redis Cluster lifecycle management, or a built-in
forwarding-header trust policy.
