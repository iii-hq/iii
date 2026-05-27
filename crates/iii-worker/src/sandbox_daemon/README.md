# iii-sandbox

Spawn ephemeral microVMs from worker code or the terminal. The daemon registers 14 `sandbox::*` triggers — 4 lifecycle ops plus 10 filesystem ops — every one called via `iii.trigger()`. Each sandbox boots in a few hundred milliseconds, runs commands isolated from the host, and is reaped when idle. The overlay filesystem is discarded on stop.

> **Implementation:** `crates/iii-worker/src/sandbox_daemon/`. Ships inside the `iii-worker` binary; the engine starts the daemon as `iii-worker sandbox-daemon` when `iii-sandbox` appears in `config.yaml`.

**Use it for:** running untrusted code, AI-agent tool calls, one-shot scripts, per-request isolation.

**Don't use it for:** long-lived services (use a regular worker), durable stateful tasks (overlay is wiped on stop).


## Agent Quickstart

If you are an AI agent calling `sandbox::*`, start here. Two paths:

### One-call workflow (recommended): `sandbox::run`

Boots a VM, drops your code in `/tmp/run.{ext}`, runs the interpreter, captures
stdout/stderr, and stops the VM. One call, one response.

```json
{
  "trigger": "sandbox::run",
  "payload": {
    "image": "node",
    "code": "console.log('hello from sandbox')",
    "lang": "node"
  }
}
```

`lang` accepts `"node"`, `"python"`, `"shell"`, or a custom interpreter binary path.
Pass `keep_sandbox: true` if you want to keep the VM alive to inspect afterwards.

### Surgical workflow: `sandbox::create` -> `sandbox::fs::write` -> `sandbox::exec` -> `sandbox::stop`

Use this when you need fine-grained control over multiple operations on one sandbox.

```json
// 1. boot
{ "trigger": "sandbox::create", "payload": { "image": "node" } }
// 2. write a file (content accepts a UTF-8 string directly)
{ "trigger": "sandbox::fs::write", "payload": {
    "sandbox_id": "<uuid>", "path": "/home/app/main.js",
    "content": "console.log('hi')\n"
} }
// 3. run it (3 valid cmd shapes — pick whichever you like)
{ "trigger": "sandbox::exec", "payload": {
    "sandbox_id": "<uuid>", "cmd": "node", "args": ["/home/app/main.js"]
} }
// 4. tear down
{ "trigger": "sandbox::stop", "payload": { "sandbox_id": "<uuid>" } }
```

### Three accepted `cmd` shapes for `sandbox::exec`

```json
{ "cmd": "node /home/app/main.js" }                 // shell-line, shlex-split
{ "cmd": "node", "args": ["/home/app/main.js"] }   // classic POSIX argv
{ "argv": ["node", "/home/app/main.js"] }          // single argv array
```

Shlex is **not** bash — `cmd: "echo $HOME && pwd"` won't expand variables or chain
commands. Use `sandbox::run` with `lang: "shell"` for bash semantics.

### Two accepted `env` shapes for `sandbox::create`, `sandbox::exec`, `sandbox::run`

```json
{ "env": ["FOO=bar", "PATH=/usr/bin"] }   // wire shape (legacy)
{ "env": { "FOO": "bar", "PATH": "/usr/bin" } }   // agent-natural shape
```

### Error responses carry a self-healing payload

Every error returns JSON encoded inside `error.message` (see SandboxErrorWire docs).
Parse it once:

```js
const detail = JSON.parse(err.message);
// detail.code, detail.type, detail.message, detail.docs_url, detail.retryable
// detail.fix       — ready-to-send next-call payload, null if not auto-fixable
// detail.fix_note  — one-liner explaining why fix is null
```

If `detail.fix` is non-null, your next call is `await fn(detail.fix)`. The error IS
the recovery path.

### Error codes (anchors below)


## Host requirements

Sandboxes run as libkrun microVMs and need hardware virtualization on the host:

- **macOS:** Apple Silicon (M-series). Intel Macs can't boot sandboxes.
- **Linux:** `/dev/kvm` readable by the engine process.
- **Windows:** unsupported.

Hosts without hardware virtualization will fail `sandbox::create` with error `S300` and a stderr tail from the failed VM process. See `S300` in `docs/api-reference/sandbox.mdx` for the full diagnostic flow.

## Sample Configuration

```yaml
- name: iii-sandbox
  config:
    auto_install: true
    image_allowlist:
      - python
      - node
    default_idle_timeout_secs: 300
    max_concurrent_sandboxes: 32
    default_cpus: 1
    default_memory_mb: 512
```

`iii worker add iii-sandbox` appends this block to your `config.yaml`. Trim or extend `image_allowlist` and `custom_images` to control what callers can boot.

## Configuration

| Field | Type | Default | Description |
|---|---|---|---|
| `auto_install` | boolean | `true` | Pull the image from its OCI ref on first use when the rootfs isn't cached. Set `false` in air-gapped or pre-provisioned deployments — callers get `S101` and operators pre-pull with `iii worker add iiidev/<image>`. |
| `image_allowlist` | string[] | `[]` | **Fail-closed** list of image names that may be booted. Entries must be preset names (`python`, `node`) or keys from `custom_images`. Empty list denies everything — `sandbox::create` returns `S100` for every request. |
| `default_idle_timeout_secs` | number | `300` | Reap a sandbox when `now - last_exec_at` exceeds this. The reaper runs every 10 s. Per-request `idle_timeout_secs` on `sandbox::create` overrides. |
| `max_concurrent_sandboxes` | number | `32` | Hard cap on live sandboxes. The 33rd concurrent `sandbox::create` returns `S400`. Size by host RAM (default RAM per sandbox × cap ≤ available RAM). |
| `default_cpus` | number | `1` | vCPUs per sandbox when the request omits `cpus`. |
| `default_memory_mb` | number | `512` | RAM ceiling per sandbox when the request omits `memory_mb`. |
| `per_image_caps` | map | `{}` | Per-image hard caps. Each value is `{ max_cpus: N, max_memory_mb: N }`. Requests exceeding a cap return `S400`. |
| `custom_images` | map | `{}` | Deployment-specific images beyond the built-in presets. Map key is the name used in `image_allowlist` and the `image` field on `sandbox::create`; value is a fully-qualified OCI reference (e.g. `ghcr.io/acme/my-app:1.2.3`). Preset names (`python`, `node`) are reserved. See `docs/api-reference/sandbox.mdx`. |

## Triggers

All 14 triggers are dispatched via `iii.trigger({ function_id, payload, timeoutMs })`. Recommended `timeoutMs` is in each table; lifecycle ops have meaningful timeout pressure, filesystem ops generally don't (the daemon is local).

### Lifecycle (4)

#### `sandbox::create`

Boot a microVM and return a `sandbox_id`. Recommended `timeoutMs`: `300_000` (cold pull can take 5-30 s).

| Field | Type | Default | Description |
|---|---|---|---|
| `image` | string | required | Preset (`python`, `node`) or `custom_images` key. |
| `cpus` | number | `default_cpus` | vCPUs. Capped by `per_image_caps`. |
| `memory_mb` | number | `default_memory_mb` | RAM ceiling. Capped by `per_image_caps`. |
| `name` | string | none | Human label for `sandbox::list`. |
| `network` | boolean | `false` | Enable guest networking. |
| `idle_timeout_secs` | number | `default_idle_timeout_secs` | Override the per-sandbox idle reaper. |
| `env` | string[] | `[]` | `K=V` entries injected into the guest. |

Returns: `{ sandbox_id, image }`.

#### `sandbox::exec`

Run a command inside a live sandbox. Recommended `timeoutMs`: `35_000` (daemon's 30 s default + 5 s margin).

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string (UUID) | required | From `sandbox::create`. |
| `cmd` | string | required | Executable name. |
| `args` | string[] | `[]` | argv tail. |
| `stdin` | string (base64) | none | Bytes piped to the process's stdin. |
| `env` | string[] | `[]` | `K=V` entries merged on top of the boot env. |
| `timeout_ms` | number | `300_000` | Per-exec deadline enforced inside the daemon. Sized for cold `npm install` / `pip install` / `cargo build`; pass a smaller value for probes and version checks. |
| `workdir` | string | guest home | Working directory. |

Returns: `{ stdout, stderr, exit_code, timed_out, duration_ms, success }`.

#### `sandbox::list`

Enumerate active sandboxes. Empty payload (`{}`). Returns an array of `{ sandbox_id, image, name, status, created_at, last_exec_at }`.

#### `sandbox::stop`

Tear down a sandbox and reclaim resources.

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string (UUID) | required | Sandbox to stop. |
| `wait` | boolean | `false` | Block until the VM process has exited. |

Returns: `{ sandbox_id, stopped }`.

### Filesystem (10)

All filesystem triggers take `sandbox_id` (UUID) plus operation-specific fields. Bumping the sandbox's idle clock is automatic — fs activity counts as liveness. Errors return `S2xx`.

#### `sandbox::fs::ls`

| Field | Type | Description |
|---|---|---|
| `sandbox_id` | string | Sandbox to operate in. |
| `path` | string | Directory to list. |

Returns: `{ entries: FsEntry[] }` where each entry has `{ name, is_dir, size, mode, mtime, is_symlink }`.

#### `sandbox::fs::stat`

| Field | Type | Description |
|---|---|---|
| `sandbox_id` | string | Sandbox to operate in. |
| `path` | string | Path to inspect. |

Returns: `{ name, is_dir, size, mode, mtime, is_symlink }`.

#### `sandbox::fs::mkdir`

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string | required | Sandbox to operate in. |
| `path` | string | required | Directory to create. |
| `mode` | string | `"0755"` | Octal permissions. |
| `parents` | boolean | `false` | Create intermediate directories (`mkdir -p`). |

Returns: `{ created: boolean }`.

#### `sandbox::fs::write`

Stream a file into the sandbox. The `content` field is a `StreamChannelRef` that the caller writes to (channel-paced — no envelope timeout cap).

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string | required | Sandbox to operate in. |
| `path` | string | required | Destination path inside the guest. |
| `mode` | string | `"0644"` | Octal permissions for the new file. |
| `parents` | boolean | `false` | Create missing parent directories. |
| `content` | StreamChannelRef | required | Channel handle the caller writes bytes to. |

Returns: `{ bytes_written, path }`.

#### `sandbox::fs::read`

Read a file out of the sandbox. Always returns a `content: StreamChannelRef` the caller can subscribe to for the full file bytes. For UTF-8 text files under 1 MiB, the response also includes an inline `body: string` so callers can short-circuit the channel subscription and use the body directly.

| Field | Type | Description |
|---|---|---|
| `sandbox_id` | string | Sandbox to operate in. |
| `path` | string | Path to read. |

Returns: `{ content: StreamChannelRef, body?: string, size, mode, mtime }`.

- `content` is always populated. The same bytes are delivered through it whether or not `body` is also set, so peers that statically type `content` as `StreamChannelRef` keep working unchanged.
- `body` is present (`Some`) for files that fit in the 1 MiB inline cap **and** decode cleanly as UTF-8. Absent (`None`) for large or binary files — read those through `content` instead.

#### `sandbox::fs::rm`

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string | required | Sandbox to operate in. |
| `path` | string | required | Path to remove. |
| `recursive` | boolean | `false` | Recurse into directories (`rm -r`). |

Returns: `{ removed: boolean }`.

#### `sandbox::fs::chmod`

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string | required | Sandbox to operate in. |
| `path` | string | required | Target path. |
| `mode` | string | required | Octal permissions (e.g. `"0644"`). |
| `uid` | number | unchanged | New owner UID. |
| `gid` | number | unchanged | New group GID. |
| `recursive` | boolean | `false` | Apply recursively to a directory tree. |

Returns: `{ updated: number }` — count of paths changed.

#### `sandbox::fs::mv`

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string | required | Sandbox to operate in. |
| `src` | string | required | Source path. |
| `dst` | string | required | Destination path. |
| `overwrite` | boolean | `false` | Allow overwriting an existing destination. |

Returns: `{ moved: boolean }`.

#### `sandbox::fs::grep`

Recursive regex search.

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string | required | Sandbox to operate in. |
| `path` | string | required | Root path. |
| `pattern` | string | required | Regex (RE2 syntax). |
| `recursive` | boolean | `true` | Walk subdirectories. |
| `ignore_case` | boolean | `false` | Case-insensitive match. |
| `include_glob` | string[] | `[]` | Gitignore-style include filter. |
| `exclude_glob` | string[] | `[]` | Gitignore-style exclude filter. |
| `max_matches` | number | `10000` | Stop after N matches. |
| `max_line_bytes` | number | `4096` | Truncate any single line longer than this. |

Returns: `{ matches: FsMatch[], truncated }` where each match has `{ path, line_no, byte_offset, line }`.

#### `sandbox::fs::sed`

Find-and-replace across files. Pass either `files` (explicit list) or `path` (walk like grep) — exactly one. Passing both, or neither, returns `S210`.

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string | required | Sandbox to operate in. |
| `files` | string[] | `[]` | Explicit list of paths. Mutually exclusive with `path`. |
| `path` | string | none | Root path to walk. Mutually exclusive with `files`. |
| `recursive` | boolean | `true` | Walk subdirectories. Only meaningful with `path`. |
| `include_glob` | string[] | `[]` | Include filter (only with `path`). |
| `exclude_glob` | string[] | `[]` | Exclude filter (only with `path`). |
| `pattern` | string | required | Regex or literal (see `regex` flag). |
| `replacement` | string | required | Replacement string. |
| `regex` | boolean | `true` | Treat `pattern` as a regex; `false` for literal. |
| `first_only` | boolean | `false` | Replace only the first match in each file. |
| `ignore_case` | boolean | `false` | Case-insensitive match. |

Returns: `{ results: FsSedFileResult[], total_replacements }`.

## Example: create → exec → stop

```typescript
import { registerWorker } from 'iii-sdk'

const iii = registerWorker('ws://127.0.0.1:49134')

const { sandbox_id } = await iii.trigger({
  function_id: 'sandbox::create',
  payload: { image: 'python', cpus: 1, memory_mb: 512 },
  timeoutMs: 300_000,
})

const out = await iii.trigger({
  function_id: 'sandbox::exec',
  payload: { sandbox_id, cmd: 'python3', args: ['-c', 'print(2 + 2)'] },
  timeoutMs: 35_000,
})
console.log(out.stdout) // "4\n"

await iii.trigger({
  function_id: 'sandbox::stop',
  payload: { sandbox_id, wait: true },
})
```

```python
from iii import register_worker

iii = register_worker("ws://127.0.0.1:49134")

result = await iii.trigger({
    "function_id": "sandbox::create",
    "payload": {"image": "python", "cpus": 1, "memory_mb": 512},
    "timeout_ms": 300_000,
})
sandbox_id = result["sandbox_id"]

out = await iii.trigger({
    "function_id": "sandbox::exec",
    "payload": {"sandbox_id": sandbox_id, "cmd": "python3", "args": ["-c", "print(2 + 2)"]},
    "timeout_ms": 35_000,
})
print(out["stdout"])  # "4\n"

await iii.trigger({
    "function_id": "sandbox::stop",
    "payload": {"sandbox_id": sandbox_id, "wait": True},
})
```

## CLI

The `iii sandbox` subcommands wrap a curated subset of the lifecycle and fs surface:

```
iii sandbox run <image> -- <cmd> [args...]    # one-shot create+exec+stop
iii sandbox create <image> [--idle-timeout N] # boot, print sandbox_id
iii sandbox exec <sandbox_id> -- <cmd> ...    # exec into a live sandbox
iii sandbox list
iii sandbox stop <sandbox_id>
iii sandbox upload <sandbox_id> <local> <remote>
iii sandbox download <sandbox_id> <remote> <local>
```

Anything outside this set (e.g. `fs::grep`, `fs::sed`, `fs::chmod`) is reachable only via `iii.trigger()` from worker code.

## Errors

The daemon returns typed `SandboxError`s with S-codes embedded in the wire
payload (`{type, code, message, docs_url, fix, fix_note, retryable}`).

`error.docs_url` resolves to one of the anchors below. The anchor IDs are
case-sensitive HTML anchors so a regression test
(`sandbox_docs_anchor_stability`) can verify each `SandboxErrorCode` has a
documented home.

### Request validation

<a id="S001"></a>
#### S001 — invalid request
Bad UUID, missing required field, ambiguous `cmd`/`args`/`argv` combo, invalid
env var name. Check `error.message` for the specific cause.

<a id="S002"></a>
#### S002 — sandbox not found
The `sandbox_id` is well-formed but no sandbox with that id exists. Call
`sandbox::create` first.

<a id="S003"></a>
#### S003 — concurrent exec
Another exec is already in flight on this sandbox. Await it before submitting
another.

<a id="S004"></a>
#### S004 — sandbox already stopped
The sandbox was reaped or explicitly stopped. Create a new one.

### Image catalog

<a id="S100"></a>
#### S100 — image not in catalog
The `image` value isn't a built-in preset (`python`, `node`) and isn't a key in
`sandbox.custom_images` of `iii.config.yaml`. Either pick a known preset or add
a custom_images entry.

<a id="S101"></a>
#### S101 — rootfs missing
The image is in the catalog but the rootfs isn't on disk. Operator action:
run `iii worker add <image-ref>` on the host.

<a id="S102"></a>
#### S102 — auto-install failed (transient)
Pull or extract of the image bundle failed. Often transient — retry.

### Exec runtime

<a id="S200"></a>
#### S200 — exec timed out
The `timeout_ms` window elapsed before the binary exited. Raise the timeout or
break the work into smaller steps.

### Filesystem

<a id="S210"></a>
#### S210 — fs invalid request
Mutually exclusive fields, missing required field, unsupported operation
combination. Check `error.message`.

<a id="S211"></a>
#### S211 — file not found

<a id="S212"></a>
#### S212 — wrong file type (expected file, found directory, or vice versa)

<a id="S213"></a>
#### S213 — already exists

<a id="S214"></a>
#### S214 — directory not empty

<a id="S215"></a>
#### S215 — permission denied

<a id="S216"></a>
#### S216 — fs i/o error

<a id="S217"></a>
#### S217 — invalid regex pattern

<a id="S218"></a>
#### S218 — fs channel aborted (transient)
The streaming channel closed before the operation completed. Retry.

<a id="S219"></a>
#### S219 — fs operation unsupported
The sandbox supervisor inside the VM is too old to implement this fs op.
Upgrade iii-worker.

### Platform

<a id="S300"></a>
#### S300 — VM boot failed
The microVM couldn't boot. Almost always missing virtualization on the host
(no `/dev/kvm`, Intel Mac, Windows). Check the stderr tail in `error.message`.

<a id="S400"></a>
#### S400 — resource limit exceeded
Capacity bound (`max_concurrent_sandboxes`, per-image caps) reached.

For richer diagnostics per code see
`docs/api-reference/sandbox.mdx#s-codes` (when that page exists; see TODO in
`crates/iii-worker/src/sandbox_daemon/errors.rs` for the docs-site migration).


## See also

- [`docs/api-reference/sandbox.mdx`](https://github.com/iii-hq/iii/blob/main/docs/api-reference/sandbox.mdx) — full payload reference, S-code diagnostics, custom images, environment variables, troubleshooting.
- [`docs/how-to/developing-sandbox-workers`](https://github.com/iii-hq/iii/blob/main/docs/how-to/developing-sandbox-workers) — how worker processes themselves run inside isolated microVMs (different topic from this trigger surface).
