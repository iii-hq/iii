# iii-engine-functions (internal)

This is an internal engine worker. It is not configurable by users and has no `iii.worker.yaml`.

The engine functions module provides core built-in functions that are always available in the engine, regardless of which optional workers are configured. This includes:

- Internal trigger dispatch helpers
- Core engine introspection functions
- Foundation utilities used by other built-in workers

These functions are registered automatically at engine startup and are not part of the public worker registry.
