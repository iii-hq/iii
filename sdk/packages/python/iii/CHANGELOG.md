# Changelog

All notable changes to `iii-sdk` (Python) are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- `ConnectionError` raised by `register_worker` / `_wait_until_connected` now
  includes the target URL, the command to start the engine, and a link to
  `https://iii.dev/docs/install`.

### Documentation
- README now opens with a `Prerequisites` block covering engine install and
  engine start.
- Hello World switched to the string form `register_function("greet", handler)`
  and removed the incorrect `iii.connect()` call — `register_worker` already
  auto-connects and blocks until ready.
- Dict-form `register_function({"id": "greet"}, handler)` is still supported
  but documented under "Advanced" instead of the teaching path.

## [0.11.0] — 2025-12

### Removed
- Removed legacy `call`, `callVoid`, `triggerVoid` methods. Use `trigger()`
  for all invocations:
  ```python
  iii.trigger({"function_id": id, "payload": data, "action": TriggerAction.Void()})
  ```

[Unreleased]: https://github.com/iii-hq/iii/tree/main/sdk/packages/python/iii
[0.11.0]: https://github.com/iii-hq/iii/releases/tag/iii/v0.11.0
