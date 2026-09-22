---
name: iii-engine-config
description: >-
  Configure a managed iii engine through worker-compose.yaml or a directly supervised engine
  through config.yaml. Use for engine ports, RBAC via the rbac-proxy worker, streams, sandboxes,
  and configuration storage.
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
iii trigger -n orders-daemon compose::add worker=state
```

## RBAC with `rbac-proxy`

Keep the engine's worker-manager port internal and put the `rbac-proxy` worker in front of it when
untrusted workers, browsers, or agents need to connect:

```bash
iii trigger -n orders-daemon compose::add worker=rbac-proxy
```

`rbac-proxy` opens its own WebSocket port, speaks the worker protocol verbatim, and at the boundary
authenticates each connection (`rbac.auth_function_id`), gates every invocation and trigger binding
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

Only the proxy's port should face an untrusted network, and a public proxy must always set
`auth_function_id`. Full schema: https://workers.iii.dev/workers/rbac-proxy.

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

- Bind the worker-manager listener to `127.0.0.1`; expose only the `rbac-proxy` port.
- Set `rbac.auth_function_id` on every public proxy and keep `expose_functions` narrow.
- Keep secrets in environment-backed configuration; do not commit literal credentials.
- Preserve configuration, stream, state, and queue storage paths during migrations.
- Use `compose::status` for process ownership and `engine::workers::list` for live connections.
- `compose::add` never edits `engine:` or restarts the engine.

## When to Use

- Use this skill for managed `engine:` maps, direct `config.yaml`, engine-owned workers, ports,
  adapters, RBAC via `rbac-proxy`, or engine lifecycle selection.
- Use it when migrating engine worker blocks into Compose.

## Boundaries

- Project worker lifecycle belongs to Compose, not `config.yaml` or `iii worker`.
- For function and trigger code, use `iii-core-primitives`.
- For worker-specific HTTP, queue, cron, state, or pubsub behavior, use that worker's docs.
