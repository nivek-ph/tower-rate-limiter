# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

## [0.1.3](https://github.com/nivek-ph/tower-rate-limiter/compare/v0.1.2...v0.1.3) - 2026-08-10

### Added

- *(tracing)* add optional structured tracing for Store failures ([#16](https://github.com/nivek-ph/tower-rate-limiter/pull/16))

### Other

- update README for clarity and quick start guide
- *(redis)* share RedisStore contract tests across runtimes ([#15](https://github.com/nivek-ph/tower-rate-limiter/pull/15))
- *(redis)* verify RedisStore on the Smol runtime ([#12](https://github.com/nivek-ph/tower-rate-limiter/pull/12))
- *(redis)* add smol support for RedisStore and implement related tests ([#13](https://github.com/nivek-ph/tower-rate-limiter/pull/13))

## [0.1.2](https://github.com/nivek-ph/tower-rate-limiter/compare/v0.1.1...v0.1.2) - 2026-08-09

### Added

- *(redis)* update dependencies and enhance Redis integration ([#11](https://github.com/nivek-ph/tower-rate-limiter/pull/11))
- integrate moka for memory store caching in rate limiter ([#10](https://github.com/nivek-ph/tower-rate-limiter/pull/10))

### Other

- simplify rate limiter internals and tests ([#9](https://github.com/nivek-ph/tower-rate-limiter/pull/9))
- expand mdBook guides and examples ([#6](https://github.com/nivek-ph/tower-rate-limiter/pull/6))
- add mdBook website with GitHub Pages deployment ([#4](https://github.com/nivek-ph/tower-rate-limiter/pull/4))

## [0.1.1](https://github.com/nivek-ph/tower-rate-limiter/compare/v0.1.0...v0.1.1) - 2026-08-07

### Added

- unify rate-limit responses and configuration ([#2](https://github.com/nivek-ph/tower-rate-limiter/pull/2))

### Added

- Add request bypass predicates through `RateLimitBuilder::skip`.
- Add selectable Draft 7 and Draft 11 RateLimit fields, with an option to disable both fields.

### Changed

- Centralize middleware-generated response construction and RateLimit field finalization.
- Rename `StoreErrorAction` to `StoreFailureMode`.
- Rename `RateLimitError` variants to `Key`, `Quota`, and `Store`.
- Rename caller-supplied key transformation APIs to `with_key_encoder` and `KeyEncoder`.

## [0.1.0] - 2026-08-06

### Added

- Add caller-supplied encoding for complete scoped Store keys.
- Add live Redis fixed-window integration testing in CI.

### Changed

- Consolidate charged-request evaluation and response metadata handling.
- Refactor `RateLimitBuilder` configuration while preserving static dispatch.
- Harden Redis fixed-window key handling and script-result validation.
- Pin the minimum supported Rust version to 1.96.0.

## [0.1.0-alpha.0]

### Added

- Initial prerelease.
