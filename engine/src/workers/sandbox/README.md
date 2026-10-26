# iii-sandbox

Spawn ephemeral microVMs from worker code. The router serves 16 `sandbox::*` triggers — 4 lifecycle ops, 10 filesystem ops, the one-shot `sandbox::run`, and `sandbox::catalog::list` — every one called via `iii.trigger()`. Each sandbox boots in a few hundred milliseconds, runs commands isolated from the host, and is reaped when idle. The overlay filesystem is discarded on stop.

> **Implementation:** `engine/src/workers/sandbox/`. The engine's built-in sandbox router. It
> registers every `sandbox::*` function in the `default` namespace and forwards each call,
> unchanged, to the bound sandbox adapter worker (`sandbox-libkrun` by default). See
> [Binding](#binding).

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
Adapter errors carry the SDK's `handler error: ` prefix, so parse from the first `{`:

```js
const detail = JSON.parse(err.message.slice(err.message.indexOf("{")));
// detail.code, detail.type, detail.message, detail.docs_url, detail.retryable
// detail.fix       — ready-to-send next-call payload, null if not auto-fixable
// detail.fix_note  — one-liner explaining why fix is null
```

If `detail.fix` is non-null, **merge** its fields into your original request and
resubmit with `await fn(mergedRequest)` — keep your original fields and let
`detail.fix` override only what it names; do not call `fn(detail.fix)` on its own.
For example `FsParentNotFound` returns `fix: { "parents": true }`, which you merge
into the original `sandbox::fs::write` / `sandbox::fs::mkdir` request rather than
replacing it. `detail.fix_note` spells out the merge. The error IS the recovery path.

## Binding

The router serves `sandbox::*` and nothing else; the adapter worker boots the VMs. The Compose file
that owns the engine picks the adapter:

```yaml
engine:
  sandbox:
    adapter: sandbox-libkrun   # function prefix = the adapter's manifest name; default
    namespace: my-proj         # optional; default = this project's namespace
```

- A Compose-managed engine without an `engine.sandbox` section is bound to `sandbox-libkrun` in the
  owner's namespace. Compose installs the adapter when a VM container, `engine.sandbox` (unless its
  `namespace` names another one) or `compose::add iii-sandbox` asks for it.
- An engine no Compose file manages (External mode: the file has no `engine:` section) gets no
  binding. Set `sandbox:` in that engine's config, or its callers get `S303`.
- Changing the section needs a Compose restart.
- Adapter settings (`image_allowlist`, `custom_images`, `default_*`, `max_*`, `auto_install`, ...)
  are not engine config. They go under `containers.sandbox-libkrun.config_override` with the same
  keys they had under `engine.workers.iii-sandbox`.
- Known limit: the router re-reads `adapter::info` only after an `S301` or `invocation_stopped`. An
  adapter swapped for another major between calls is not caught until then.

Host requirements (hardware virtualization, the embedded guest init) belong to `sandbox-libkrun`.

## Triggers

All 16 triggers are dispatched via `iii.trigger({ function_id, payload, timeoutMs })`. Recommended `timeoutMs` is in each table; lifecycle ops have meaningful timeout pressure, filesystem ops generally don't (the daemon is local).

### Lifecycle (4)

#### `sandbox::create`

Boot a microVM and return a `sandbox_id`. Recommended `timeoutMs`: `300_000` (cold pull can take 5-30 s).

| Field | Type | Default | Description |
|---|---|---|---|
| `image` | string | required | Preset (`python`, `node`) or `custom_images` key. |
| `cpus` | number | `default_cpus` | vCPUs. Capped by `per_image_caps`. |
| `memory_mb` | number | `default_memory_mb` | RAM ceiling. Capped by `per_image_caps`. |
| `name` | string | none | Human label for `sandbox::list`. |
| `network` | boolean | `false` | Enable guest networking. Relayed network payload then counts as liveness for the idle reaper — a sandbox serving traffic is not "idle". |
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

Returns: `{ stdout, stderr, exit_code, timed_out, duration_ms, success, stdout_truncated, stderr_truncated }`. A `*_truncated` flag is `true` when the adapter cut that stream at its output cap; a contract 1.0 adapter sends neither key.

#### `sandbox::list`

Enumerate active sandboxes. Empty payload (`{}`). Returns an array of `{ sandbox_id, image, name, status, created_at, last_exec_at }`.

#### `sandbox::stop`

Tear down a sandbox and reclaim resources.

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string (UUID) | required | Sandbox to stop. |
| `wait` | boolean | `false` | Block until the VM process has exited. |

Returns: `{ sandbox_id, stopped }`.

### One-shot (1)

#### `sandbox::run`

Boot a VM, run a code snippet, capture output, and auto-stop — in a single call. The fast path for agents; see the [Agent Quickstart](#one-call-workflow-recommended-sandboxrun) above for the full walkthrough. Recommended `timeoutMs`: `300_000`.

| Field | Type | Default | Description |
|---|---|---|---|
| `image` | string | required | Preset (`python`, `node`) or `custom_images` key. |
| `code` | string | required | Source written to `/tmp/run.{ext}` and executed. |
| `lang` | string | required | `node`, `python`, `shell`, or a literal interpreter binary path. No default. |
| `files` | object[] | `[]` | Extra files dropped in first, each `{ path, content }` (UTF-8). |
| `env` | string[] \| map | `[]` | Injected into the interpreter (both env shapes accepted). |
| `stdin` | string (base64) | none | Bytes piped to the interpreter's stdin. |
| `timeout_ms` | number | `300_000` | Per-run deadline. |
| `keep_sandbox` | boolean | `false` | Keep the VM alive after the run and return its `sandbox_id`. |

Returns: `{ stdout, stderr, exit_code, timed_out, duration_ms, success, stdout_truncated, stderr_truncated, sandbox_id? }`. The `*_truncated` flags are as for `sandbox::exec`. `sandbox_id` is present only when `keep_sandbox: true`; otherwise the VM is stopped on success and on failure.

### Catalog (1)

#### `sandbox::catalog::list`

List the images this engine can boot — bundled presets plus operator-registered `custom_images`. Call it before `sandbox::create` / `sandbox::run` when you don't already know what's available (closes the `S100` "image not in catalog" loop). Empty payload (`{}`).

Returns: `{ images: [ { name, oci_ref, kind } ] }` where `name` is the value you pass to `image`, `oci_ref` is the pulled reference, and `kind` is `"preset"` or `"custom"`. Presets come first; custom entries follow, sorted by name.

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

Write a file into the sandbox. The body is given by exactly one of `content` or `content_b64`:

- `content: "<utf-8 string>"` — a bare JSON string written verbatim. The agent-natural form for source/text.
- `content_b64: "<base64>"` — inline binary (decoded before write).
- `content: <StreamChannelRef>` — an object channel handle for large/streaming uploads from a programmatic caller (channel-paced, no envelope timeout cap).

| Field | Type | Default | Description |
|---|---|---|---|
| `sandbox_id` | string | required | Sandbox to operate in. |
| `path` | string | required | Destination path inside the guest. |
| `mode` | string | `"0644"` | Octal permissions for the new file. |
| `parents` | boolean | `false` | Create missing parent directories. |
| `content` | string \| StreamChannelRef | one of `content`/`content_b64` | UTF-8 string (inline) or a channel handle (streaming). |
| `content_b64` | string | one of `content`/`content_b64` | Base64-encoded inline binary body. |

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
| `pattern` | string | required | Regex (Rust `regex` crate syntax). |
| `recursive` | boolean | `true` | Walk subdirectories. |
| `ignore_case` | boolean | `false` | Case-insensitive match. |
| `include_glob` | string[] | `[]` | Gitignore-style include filter. |
| `exclude_glob` | string[] | `[]` | Gitignore-style exclude filter. |
| `max_matches` | number | `10000` | Stop after N matches. |
| `max_line_bytes` | number | `4096` | Truncate any single line longer than this. |

Returns: `{ matches: FsMatch[], truncated }` where each match has `{ path, line, content }` and `line` is 1-based.

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

## Errors

Adapter errors reach the caller untouched, as the typed `SandboxError` wire payload
(`{type, code, message, docs_url, fix, fix_note, retryable}`) behind the SDK's `handler error: `
prefix. Every code, its meaning and its fix is documented at
https://iii.dev/docs/creating-workers/sandboxes. The router adds four:

- `S301`: the bound adapter is not connected (retryable); the message names `compose::add iii-sandbox` as the fix.
- `S302`: the adapter speaks another contract major than the router.
- `S303`: no adapter is configured; the engine is not managed by a Compose file and has no `sandbox:` binding.
- `S304`: the connected adapter does not register the called operation (not retryable).
