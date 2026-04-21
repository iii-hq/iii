# Changelog

All notable changes to `iii-sdk` (Rust) are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `TriggerRequest::new(function_id, payload)` constructor for the common case.
- `TriggerRequest::with_action(action)` and `TriggerRequest::with_timeout_ms(ms)`
  builder helpers for overriding defaults.
- `examples/readme_hello.rs` — the canonical source-of-truth for the Rust SDK
  README Hello World. CI verifies it compiles via `cargo check --example readme_hello`.

### Changed
- `IIIError::NotConnected` message now names the URL, the command to start the
  engine, and a link to `https://iii.dev/docs/install` — the first real error a
  new dev hits is now actionable instead of opaque.

### Documentation
- README now opens with a `Prerequisites` block covering engine install and
  engine start.
- Hello World uses `RegisterFunction::new_async`, the typed `IIITrigger::Http`
  trigger builder, and the new `TriggerRequest::new` — all of which compile
  against the current public API.
- Dropped the stale `iii-sdk = "0.3"` install line in favor of `"0.11"`.
- The struct-literal `RegisterTriggerInput { ... }` form is documented as an
  "Advanced" escape hatch; the typed builder is the recommended path.

## [0.11.0] — 2025-12

### Removed
- Removed legacy `call`, `call_void`, `trigger_void` methods. Use `trigger()`
  for all invocations:
  ```rust
  iii.trigger(TriggerRequest::new(id, payload).with_action(TriggerAction::Void)).await?
  ```

[Unreleased]: https://github.com/iii-hq/iii/tree/main/sdk/packages/rust/iii
[0.11.0]: https://github.com/iii-hq/iii/releases/tag/iii/v0.11.0
