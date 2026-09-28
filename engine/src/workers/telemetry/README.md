# iii-telemetry (internal)

This is an internal engine worker. It is not configurable by users and has no `iii.worker.yaml`.

The telemetry worker collects anonymous usage data from the engine to help improve III. It handles:

- Gathering anonymous runtime metrics (feature usage, error rates)
- Sending telemetry payloads to the III telemetry backend
- Respecting opt-out settings configured by the user

It also follows the guided tour: the `onboarding` worker publishes each
completed step on the `onboarding:steps:complete` topic, and this worker subscribes to
that topic (with the internal handler `iii-telemetry::on-onboarding-step`) and
reports each message as an `onboarding_step` event with the published payload.

The subscription is a `durable:subscriber` binding served by the `queue`
worker, so a step message waits in the queue and is retried until the handler
takes it. One event is reported per tour step, counted in memory: a
redelivered or repeated message reports once, and a restarted engine lets
every step report again. Nothing about tour progress is persisted here — that
belongs to the tour's own state.

The subscription is registered at boot; a project without the `onboarding` or
`queue` workers never fires it.

Apart from that handler, this module runs in the background and exposes no
functions or trigger types.
