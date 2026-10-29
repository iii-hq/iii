---
name: iii-engine-config
description: >-
  Configure a managed iii engine through worker-compose.yaml or a directly supervised engine
  through config.yaml. Use for engine ports, RBAC via the rbac-proxy worker, observer limits,
  streams, sandboxes, and configuration storage.
---

# Engine Config

iii has two engine configuration modes. Do not mix them.

## Managed Compose engine

The presence of `engine:` means that Compose owns the engine process. Engine worker values are
direct mappings; there is no nested `config:` field.

```yaml
namespace: orders
engine:
  url: ws://127.0.0.1:49134
  registration_namespace_grace_ms: 5000
  workers:
    configuration:
      adapter:
        name: fs
        config:
          directory: ./config
    iii-worker-manager:
      host: 127.0.0.1
      port: 49134
    iii-http-functions: {}
    iii-stream:
      host: 127.0.0.1
      port: 3112
    iii-sandbox:
      auto_install: true
containers: {}
```

Start it with:

```bash
iii compose --namespace orders-daemon --up --file worker-compose.yaml
```

`engine.url` defaults to `ws://127.0.0.1:49134` and uses Compose's `${VAR:-default}` expansion.
`engine.workers` stays opaque until the engine reads the generated YAML, so its values use
`${VAR:default}` and retain numeric types. Compose materializes an owner-only config under
`<project-dir>/.iii/compose/<daemon-namespace>/` and removes it after clean shutdown. With
`III_COMPOSE_STATE_DIR`, it uses `<state-root>/<project-slug>/<daemon-namespace>/`. Changing `engine:`
requires restarting Compose. Do not combine a managed file with explicit `--engine`.

Allowed engine worker base names are:

- `configuration`
- `iii-worker-manager`
- `iii-http-functions`
- `iii-stream`
- `iii-sandbox`

Use `#instance` for another instance of an allowed worker. `iii-engine-functions`,
`iii-telemetry`, and `iii-observability` are injected automatically and must not be declared.

HTTP, cron, queue, state, pubsub, bridge, application, and custom workers belong under
`containers:`. Add registry packages with:

```bash
iii call -n orders-daemon compose::add worker=state
```

## Observers

The engine reads an optional top-level `observers:` section (`iii-observe` explains the model).
Every key is optional; these are the defaults:

```yaml
observers:
  local_dispatch: true            # let SDKs run a worker's own functions in-process
  max_per_connection: 1024        # observers one connection may own (LIMIT_EXCEEDED)
  max_per_function: 4096          # observers one function may have (LIMIT_EXCEEDED)
  max_observe_depth: 8            # deeper activations are dropped (LOOP_GUARD)
  observer_grace_ms: 30000        # suspended observers of a disconnected worker wait this long
  disposed_retention_ms: 3600000  # a worker back after the grace period is told they were disposed
  owner_ack_timeout_ms: 30000     # an observer not acked by its host in time fails with OWNER_TIMEOUT
  activation_timeout_ms: 30000    # an unanswered activation frees its slot and counts as TIMEOUT
  default_max_in_flight: 64       # max_in_flight of an observer that sets none
  max_queue_per_observer: 1024    # queued activations beyond max_in_flight (then OVERFLOW)
```

- In `config.yaml` it sits next to `workers:`. In a managed Compose file it goes under
  `engine.observers` and is passed through as written.
- Unknown keys and zero counts or depth are rejected at load; zero timeouts only warn. Changes need
  an engine restart.
- `observers.local_dispatch: false` turns off in-process calls for every worker.
- Inspect live observers with `engine::observers::list({ function_id?, namespace?, worker? })` and
  `engine::observers::get({ id })` (results are filtered to functions the caller may invoke).
  `engine::observers::dispose({ id })` is allowed for the observing worker and unrestricted callers.
- Metrics: `iii.observer.activations.total{function_id,delivery}` (`delivery` is `engine`, `host`,
  or `observer_call`),
  `iii.observer.dropped.total{function_id,reason}`, `iii.observers{state}`,
  `iii.observer.reports.total{function_id}`,
  `iii.observer.activation.failures.total{function_id,reason}`; locally dispatched calls are counted
  in `iii.invocations.total` with `dispatch="local"`.

## RBAC with `rbac-proxy`

Keep the engine port internal and put the `rbac-proxy` worker in front of it when
untrusted workers, browsers, or agents need to connect:

```bash
iii call -n orders-daemon compose::add worker=rbac-proxy
```

`rbac-proxy` opens its own WebSocket port, speaks the worker protocol verbatim, and at the boundary
authenticates each connection (`rbac.auth_function_id`), gates every invocation and observer
(`rbac.expose_functions`, plus `allowed_functions` / `forbidden_functions` from the auth result),
namespaces a session's registrations (`function_registration_prefix`), runs
`middleware_function_id` and the registration hooks, and filters `engine::*` discovery results to
what the caller may invoke. Its settings live in the `configuration` worker under id `rbac-proxy`:

```yaml
host: 0.0.0.0
port: 49200                        # the public RBAC port
engine_url: ws://127.0.0.1:49134   # the trusted internal engine listener
rbac:
  auth_function_id: my-project::auth-function
  expose_functions:
    - match("api::*")
```

RBAC is mandatory on untrusted networks: only the proxy's port may face one, never the engine port,
and a public proxy must always set `auth_function_id`. Full schema: https://workers.iii.dev/workers/rbac-proxy.

Observers under RBAC: creating one requires that the session's grants (`expose_functions`,
`namespaces`) cover both the observed function and the target, or it fails with `FORBIDDEN`. A
private function is observable by any session whose grants cover it. The engine's `rbac:` settings accept
`on_observer_registration_function_id`, a hook whose object answer may rewrite `function_id`,
`target_function_id`, `args`, `filters`, and `metadata`; any other answer denies with `FORBIDDEN`.
The `on_function_registration_function_id` hook receives `args_format`, `private`, and `observable`
with the rest of the registration, and its answer may rewrite them. Sessions with a
`middleware_function_id` get no local dispatch, so the middleware always runs. Check the
rbac-proxy worker's docs for the version that forwards the observe messages.

## Directly supervised engine

Keep list-shaped `config.yaml` only when systemd, Kubernetes, or another supervisor owns the
engine:

```yaml
workers:
  - name: configuration
    config:
      adapter:
        name: fs
        config:
          directory: ./config
  - name: iii-worker-manager
    config:
      host: 127.0.0.1
      port: 49134
```

Start the engine and external Compose daemon separately:

```bash
iii --config config.yaml
iii compose --namespace orders-daemon --engine ws://127.0.0.1:49134 --up --file worker-compose.yaml
```

The external Compose file must omit `engine:`. `III_URL` may replace `--engine`. Direct config
expansion uses `${VAR:default}` and accepts only the same five engine-owned worker types. Any
project worker stops startup/reload with `UNSUPPORTED_CONFIG_WORKERS`.

## Security and operations

- Bind the engine port to `127.0.0.1`; expose only the `rbac-proxy` port.
- Set `rbac.auth_function_id` on every public proxy and keep `expose_functions` narrow.
- Keep secrets in environment-backed configuration; do not commit literal credentials.
- Preserve configuration, stream, state, and queue storage paths during migrations.
- Use `compose::status` for process ownership and `engine::workers::list` for live connections.
- Use `iii ps` to find iii processes on the machine, including orphaned workers, and `iii kill --orphans`, `--port` or `--compose` to stop them.
- `compose::add` never edits `engine:` or restarts the engine.

## When to Use

- Use this skill for managed `engine:` maps, direct `config.yaml`, engine-owned workers, ports,
  adapters, RBAC via `rbac-proxy`, or engine lifecycle selection.
- Use it when migrating engine worker blocks into Compose.

## Boundaries

- Project worker lifecycle belongs to Compose, not `config.yaml` or `iii worker`.
- For function code and calls, use `iii-core-primitives`; for observer code, filters, and event
  sources, use `iii-observe`.
- For worker-specific HTTP, queue, cron, state, or pubsub behavior, use that worker's docs.
