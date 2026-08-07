# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

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
