# Changelog

All notable changes to `iii-sdk` (Node.js) are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- `NotConnected` / max-retries log message now names the URL and links to
  `https://iii.dev/docs/install` so devs can diagnose connection failures without
  leaving the terminal.

### Documentation
- README now opens with a `Prerequisites` block covering engine install and
  engine start — the two steps that were previously tribal knowledge.

## [0.11.0] — 2025-12

### Removed
- `call`, `callVoid`, and `triggerVoid` have been removed. Use `trigger()` for
  all invocations. For fire-and-forget:
  ```javascript
  iii.trigger({ function_id, payload, action: TriggerAction.Void() })
  ```

[Unreleased]: https://github.com/iii-hq/iii/tree/main/sdk/packages/node/iii
[0.11.0]: https://github.com/iii-hq/iii/releases/tag/iii/v0.11.0
