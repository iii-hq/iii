# iii-telemetry (internal)

This is an internal engine worker. It is not configurable by users and has no `iii.worker.yaml`.

The telemetry worker collects anonymous usage data from the engine to help improve III. It handles:

- Gathering anonymous runtime metrics (feature usage, error rates)
- Sending telemetry payloads to the III telemetry backend
- Respecting opt-out settings configured by the user

This module runs in the background and does not expose any functions or trigger types.
