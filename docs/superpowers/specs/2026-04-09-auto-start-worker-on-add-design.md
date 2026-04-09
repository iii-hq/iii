# Auto-start Worker on Add When Engine is Running

## Problem

When a user runs `iii worker add ./path` while the engine is already running, the worker is added to `config.yaml` but the user must manually restart the engine or run `iii worker start <name>`. This creates unnecessary friction — the worker should just start.

## Solution

Detect whether the engine is running via a TCP port probe. If it is, automatically start the worker after adding it to `config.yaml`. This applies to all worker types: local-path, binary, and OCI.

## Design

### Port Probe: `is_engine_running()`

A shared function (in `config_file.rs` or a small util module) that:

1. Reads the engine port from `config.yaml` (e.g., `port` or `server.port` field)
2. Falls back to the default engine port if not found in config
3. Attempts a TCP connect to `127.0.0.1:<port>` with a ~200ms timeout
4. Returns `bool`

### Modified Add Flows

After the config.yaml write succeeds, all three add handlers gain the same tail logic:

1. Call `is_engine_running()`
2. If `true`: call the appropriate start function for the worker type, print `"  ✓ Worker auto-started"`
3. If start fails: print `"  ⚠ Could not auto-start worker. Run 'iii worker start <name>' manually."`
4. If engine not running: keep the existing `"Start the engine to run it"` message (no change)

**Affected handlers:**
- `handle_local_add()` in `local_worker.rs`
- Binary worker add in `managed.rs` (~line 148)
- OCI worker add in `managed.rs` (~line 433)

### UX

Engine running:
```
$ iii worker add ../../todo-worker-python
  Adding local worker todo-worker-python...

  ✓ Worker todo-worker-python added to config.yaml
  Path  /Users/andersonleal/projetos/motia/workers/todo-worker-python
  ✓ Worker auto-started
```

Engine not running (unchanged):
```
  ✓ Worker todo-worker-python added to config.yaml
  Path  /Users/andersonleal/projetos/motia/workers/todo-worker-python
  Start the engine to run it, or edit config.yaml to customize.
```

Auto-start error (add still succeeds):
```
  ✓ Worker todo-worker-python added to config.yaml
  Path  /Users/andersonleal/projetos/motia/workers/todo-worker-python
  ⚠ Could not auto-start worker. Run `iii worker start todo-worker-python` manually.
```

## Scope

| Component | Change |
|-----------|--------|
| `is_engine_running()` | New shared function |
| `handle_local_add()` | Modified tail |
| Binary add handler | Modified tail |
| OCI add handler | Modified tail |
| Engine | No changes |

## Error Handling

Auto-start is best-effort. If it fails, the add command still succeeds (exit 0) and prints a warning with a manual fallback command. The config.yaml write is never rolled back.

## Testing

- Unit test `is_engine_running()` with a mock TCP listener on a known port
- Integration test: add a worker with a listener active, verify start function is invoked
