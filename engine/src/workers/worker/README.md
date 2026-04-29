# iii-worker-manager (internal)

This is an internal engine worker. It is not configurable by users and has no `iii.worker.yaml`.

The worker manager supervises external worker processes — workers that connect to the engine over WebSocket to register functions and triggers. It handles:

- Spawning and restarting worker processes defined in `iii-config.yaml`
- Tracking process lifecycle (start, crash, shutdown)
- Propagating shutdown signals to managed worker processes

This module is always active and is not listed in the worker registry.
