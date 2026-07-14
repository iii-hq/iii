# configuration — fetched by the daemon, finalized before spawn

The compose file never carries a full configuration and the worker never does
a startup dance (boot → connect → query → maybe restart). The **daemon**
resolves the final config and the worker receives it ready at start.

## The flow

```
configuration worker (base, by name)
        │  fetch
        ▼
compose daemon ── merge ──► final config ── standard contract ──► worker start
        ▲
        │  sparse override
worker-compose.yaml (config_override)
```

1. The container names its base config: `config_name: orders-api` (or a
   `config_uri` for explicit sources). Names resolve through the
   **configuration worker**, whose adapter decides storage — local fs today,
   secrets manager or a separate iii instance in cloud. File paths are not
   the contract; names are, so the same compose file works when storage
   moves.
2. The daemon fetches the base **before** anything else runs for that
   container. Fetch failure = the container does not start (`up` fails rather
   than booting on wrong defaults — an http worker on the wrong port is worse
   than no http worker).
3. `config_override` merges over the base: maps merge per key, arrays and
   scalars replace, `null` is an explicit value.
4. The finalized config reaches the worker through the standard CLI/env
   contract (`--config` / env; see cli-contract.md).

## Why the daemon delivers values (not a pointer)

Today `--config` is a **first-boot seed**: it populates the configuration
entry only when nothing is stored, and afterwards "the configuration worker
is the authoritative source and `--config` is ignored"
(`workers/http/src/main.rs` doc comment). If compose passed only a pointer,
every restart after first boot would silently ignore the compose overrides.
Delivering resolved values keeps `worker-compose.yaml` honest: what the file
says is what the process got — while the configuration worker remains the
authority for everything the override does not touch.

## Boundaries

- **env vs config is a hard line**: env is machine/deployment context
  (engine url, namespace); config is worker behavior (ports, adapters,
  origins). The compose file does not blur them.
- No full config bodies in the compose file — one source of truth, no
  "edited the YAML and nothing happened" trap.
- Runtime config reload is out of scope here: config is fixed for the life of
  the process (the configuration worker's watch/reload path is a separate
  track).

## Example

```yaml
containers:
  state:
    worker: package://workers.iii.dev/state
    version: 0.21.x
    config_name: orders-state
    config_override:
      cors:
        allowed_origins: ["https://app.example.com"]
```

The state worker's base `orders-state` config lives with the configuration
worker (any adapter); the compose file pins one field; the process starts
with the merged result and never re-fetches.
