---
name: sandbox
description: Ephemeral microVMs from the iii-sandbox daemon. Authoring guide for the 14 sandbox::* triggers (create, exec, list, stop, plus 10 fs:: ops), the boot/env model, and the deploy pattern for running an iii worker inside a sandbox. Single self-contained skill — meant for system-prompt injection; do not re-fetch.
---

> **Callable ids:** `sandbox::create`, `sandbox::exec`, `sandbox::list`, `sandbox::stop`, `sandbox::fs::*` — pass these to `agent_trigger { function: "sandbox::create" }` (NOT the skill path from `directory::skills::list`; that's documentation, not a function id). **There is no `sandbox::run` trigger** — `iii sandbox run` is a CLI-only wrapper around `create + exec + stop`. From the agent loop, do the three calls yourself. Session `q4czk143` burned ~3 turns trying to call `sandbox::run` and getting `function_not_found`.

# When to use

Ephemeral microVMs owned by the `iii-sandbox` daemon. Each sandbox is scoped to the engine it runs inside. Reach for them to execute an LLM-generated patch against an untrusted codebase, spin up an ephemeral build worker, or provision an integration-test fixture.

| Question | Use this |
|----------|----------|
| Boot a fresh VM | `sandbox::create` |
| Run a command inside a VM | `sandbox::exec` |
| What's running right now? | `sandbox::list` |
| Tear a VM down | `sandbox::stop` |
| Read/write/list files inside a VM | `sandbox::fs::*` |
| One-shot "code in, stdout out" | `create` → `exec` → `stop` yourself (no `sandbox::run` trigger) |

# Fetch the live schema, don't trust this page

For exact parameter and response shapes, call:

```jsonc
// engine::functions::info { function_id: "sandbox::create" }
// → request_format / response_format / description JSON
```

**Caveat for `sandbox::fs::*`:** the daemon registers the filesystem ops with **empty schemas** (`request_schema: {}`, `response_schema: {}`). `engine::functions::info` won't tell you the shape. The README from `directory::registry::workers::info { name: "iii-sandbox" }` is the source of truth for those — read it once at the start of an authoring session.

The lifecycle ops (`create` / `exec` / `list` / `stop`) DO publish full schemas. This page is **only** for the cross-cutting behaviors that don't appear in either source — non-obvious traps that bite agents composing calls from JSON alone. Don't reconstruct field names or example payloads from this page; the engine drifts faster than the markdown.

# Functions

The daemon registers **14 triggers** (verified via `engine::functions::list { prefix: "sandbox::" }` on iii-sandbox 0.13.0). The CLI ships extra one-shot wrappers (`iii sandbox run`) that are NOT triggers — don't try to call them via `agent_trigger`.

- `sandbox::create` — boot a fresh microVM from a catalog image; returns the `sandbox_id` every other op uses.
- `sandbox::exec` — run a command inside a running sandbox. **Serialized per `sandbox_id`** — concurrent calls return S003.
- `sandbox::list` — enumerate sandboxes currently running on this engine.
- `sandbox::stop` — destroy a sandbox VM by `sandbox_id`.
- `sandbox::fs::ls` / `stat` / `mkdir` / `mv` / `rm` / `chmod` — directory and metadata ops inside a running sandbox.
- `sandbox::fs::grep` — content search across files under a path.
- `sandbox::fs::sed` — regex substitution applied to a file.
- `sandbox::fs::write` — write a file into a sandbox. `content` is a **`StreamChannelRef` only** — there is no inline-string or base64 form. From the agent loop you can't easily construct one; use `sandbox::exec` with a heredoc instead (see "Behaviors" below).
- `sandbox::fs::read` — read a file out; inline string under 1 MiB, `StreamChannelRef` object above.

For the actual catalog of bookable images on a given engine, read `sandbox.image_allowlist` from `directory::registry::workers::info { name: "iii-sandbox" }`'s `config` field. There's no `sandbox::catalog::list` trigger either.

# Reaching the host engine from inside a sandbox

Processes inside a sandbox can't see the host's `localhost`. To call back into the engine that spawned the sandbox, pass `network: true` AND set `III_ENGINE_URL` in the boot `env` — the daemon rewrites `localhost` / `127.0.0.1` URLs to the per-sandbox gateway IP at VM boot, so the URL you write is the URL that resolves on the host.

```json
{
  "image":   "node",
  "network": true,
  "env":     ["III_ENGINE_URL=ws://127.0.0.1:49134"]
}
```

Inside the guest, `$III_ENGINE_URL` resolves to e.g. `ws://100.96.0.1:49134` on slot 0. The daemon picks the slot; callers don't. `49134` is the engine's default listen port — change it here if your engine listens elsewhere.

Things to know:

- **`network` defaults to `false`.** Omit it and the rewrite is skipped: the URL keeps `localhost`, which inside the guest resolves to the guest's own loopback — never the host. This is the single most common silent failure of "but it works on my host".
- **The rewrite is generic.** ANY env value containing `://localhost:<port>` or `://127.0.0.1:<port>` is rewritten the same way. Use it to hook other host services (databases, dev servers, sibling workers), not just the engine.
- **Boot-only.** The rewrite runs once at `sandbox::create`. An `III_ENGINE_URL` passed in `sandbox::exec`'s `env` is NOT rewritten — the URL passes through verbatim and resolves to the guest's loopback. **Your worker's stdout will LIE to you here**: it will happily log `engine=ws://127.0.0.1:49134` and "Worker registered and ready!" because those messages fire before / regardless of the WS handshake. The only authoritative signal is `engine::functions::list { search: "<worker-name>" }` — if that stays empty for >5 s after launch, the URL wasn't rewritten. Session `c007a1a7` burned ~10 turns on this: worker logged "ready", functions::list stayed empty, web::fetch returned 404. Fix: stop the sandbox and recreate with the env at create time.
- **Canonical name is `III_ENGINE_URL`.** `III_URL` is accepted as an alias by some daemon code paths but isn't the name to standardize on.
- **Verify the rewrite worked.** After launch, the worker's startup log should show `engine=ws://100.96.0.x:49134` (gateway IP), NOT `127.0.0.1`. If it still shows `127.0.0.1`, the rewrite didn't happen — the URL was set on `exec`, not `create`, OR `network: true` was missing.

# Behaviors that aren't in the JSON schemas

The traps below are not visible from `engine::functions::info` and bite agents that compose calls from the schema alone.

### Image — `image` is a catalog NAME, not an OCI ref

`sandbox::create` accepts the *catalog key*, not an OCI string. Bundled presets are exactly `"node"` and `"python"`. Operators add more via `sandbox.custom_images` in `iii.config.yaml` (left side is the catalog name; right side is the OCI ref the daemon pulls).

| ❌ Wrong | ✅ Right |
|---|---|
| `{ "image": "ghcr.io/iii-hq/node:latest" }` | `{ "image": "node" }` |
| `{ "image": "node:20-alpine" }` | `{ "image": "node" }` |
| `{ "image": "docker.io/library/python:3.12" }` | `{ "image": "python" }` |

An OCI ref that's not also a registered catalog key returns **S100**. The error message names the allowed keys, so a wrong guess self-corrects on the next turn. To see the live set, read `config.image_allowlist` from `directory::registry::workers::info { name: "iii-sandbox" }`. An empty catalog with no presets denies every call — a deliberate fail-closed default.

### Error envelope shape

Every `sandbox::*` op returns errors as:

```json
{
  "type":      "validation|config|internal|transient|execution|filesystem|platform",
  "code":      "Sxxx",
  "message":   "...",
  "docs_url":  "https://.../README.md#Sxxx",
  "retryable": true,
  "fix":       "<short hint> | { context, sandbox_id, inner_code }",
  "fix_note":  "one-line explainer"
}
```

`type` is one of the seven categories above — NOT the literal string `"SandboxError"`. The `S` codes are independent of the `W` codes used by the `worker::*` surface; don't conflate them.

The `fix` field carries an actionable hint as a string (or sometimes an object when nested context is useful). Read it before retrying — the daemon usually tells you exactly what to change.

### `sandbox::exec` is serialized per `sandbox_id`

Only one exec runs at a time on a given sandbox. A concurrent call returns **S003** while the previous one is still in flight. Consequences:

- Don't try to "tail the log while the worker runs" with a second exec — use `sandbox::fs::read` on the log file, or boot a second sandbox.
- To run two commands in parallel, boot two sandboxes.

### `cmd` is one binary, not a shell line — use `bash -c` for `&&` / pipes / redirects

**Wrong** (S001 — `invalid request: cmd must be a single binary, not a shell line`):

```jsonc
{ "cmd": "cd /tmp && npm install iii-sdk" }   // ❌ && is shell syntax
{ "cmd": "node app.js > out.log" }            // ❌ > is shell syntax
```

**Right** — wrap shell syntax in `bash -c`:

```jsonc
{ "cmd": "bash", "args": ["-c", "cd /tmp && npm install iii-sdk"] }
{ "cmd": "bash", "args": ["-c", "node app.js > out.log"] }
```

Session `svyuv3id` burned one turn on this. The plain `cmd` field is `shlex`-parsed: quoting and whitespace word-splitting work (so `cmd: "node -v"` is fine — splits into `["node", "-v"]`), but **`&&`, `||`, `|`, `>`, `<`, `$VAR`, `$(…)`, `*.js` are not interpreted** — they're either literal text or rejected outright.

`sandbox::exec` accepts three command shapes; precedence: `argv` wins if non-empty; whitespace-bearing `cmd` is `shlex`-split when `args`/`argv` are empty; otherwise `cmd` + `args` is the classic POSIX pair. Setting `argv` together with `cmd` or `args` returns **S001** (ambiguous). Setting only `args` without `cmd` returns `serialization error: missing field \`cmd\`` — always supply `cmd`.

### Filesystem write semantics — agent loop can't call `fs::write` directly

`sandbox::fs::write` accepts `content` as a **`StreamChannelRef` only** — no inline-string, no base64. The agent loop has no way to construct a `StreamChannelRef` (it's an SDK channel handle, not a JSON shape you can write by hand). Trying `{ "content": { "data": "...", "type": "inline" } }` or `{ "content_b64": "..." }` returns `data did not match any variant of untagged enum WriteContent` (session `c007a1a7` burned ~5 turns on this).

**From the agent loop, write files via `sandbox::exec` with a heredoc:**

```jsonc
// sandbox::exec
{
  "sandbox_id": "<UUID>",
  "cmd": "sh",
  "args": ["-c", "cat > /tmp/worker.js << 'EOF'\n<file body>\nEOF"]
}
```

**Use `/tmp/<file>` as the worker path.** `/workspace` does NOT exist by default on the bundled `node` and `python` images — writing to it returns `cannot create /workspace/worker.js: Directory nonexistent`. `/tmp` exists, is world-writable, and survives across exec calls in the same sandbox. Session `q4czk143` lost a turn to this. If you really want `/workspace`, `mkdir -p /workspace` first.

Use a quoted heredoc delimiter (`<< 'EOF'`) so shell interpolation doesn't eat `$VAR` or backticks in the body.

**Do NOT escape `$` in a quoted heredoc.** Sessions `c007a1a7` and `acxrdx6g` both hit this: writing JS template literals as `\${todo.id}` (thinking the `$` needs escaping) lands `\${todo.id}` in the file literally — invalid JS at parse time, your worker won't boot. With `<< 'EOF'`, the shell ignores `$` already; write template literals exactly as you'd write them in a source file: `${todo.id}`, no backslash. The same goes for backticks — write them bare, not as `\``.

For files with binary or many special chars, base64-encode and pipe through `base64 -d`:

```jsonc
{
  "sandbox_id": "<UUID>",
  "cmd": "sh",
  "args": ["-c", "echo '<base64>' | base64 -d > /workspace/blob.bin"]
}
```

From worker code (running inside a sandbox or on the host with the SDK), `sandbox::fs::write` works normally — `iii.trigger(...)` constructs the channel handle for you.

### Filesystem read

`sandbox::fs::read` returns `content` as a **string inline under 1 MiB**, and as a `StreamChannelRef` object above that. Type-check before using. From the agent loop, prefer `sandbox::exec` with `cat <path>` for any non-trivial size — the `StreamChannelRef` path isn't drainable from the tool surface.

### `parents: true` is required

Intermediate directories aren't auto-created. `fs::write` returns an error carrying `fix: { "parents": true }` — merge and resubmit. Same for `mkdir` — pass `parents: true` for `mkdir -p` semantics.

### The node image is bare — don't reach for `ps`, `top`, `lsof`

The bundled `node` preset ships minimal coreutils. Tools that ARE there: `sh` (POSIX, not bash), `cat`, `echo`, `node`, `npm`, `mkdir`, `chmod`, `sleep`, `timeout`. Tools that are NOT there: `ps`, `top`, `pgrep`, `lsof`, `htop`. Don't troubleshoot worker liveness with `ps aux | grep node` — it'll just exit 127. Use `engine::functions::list { search: "<worker-name>" }` to confirm a worker is registered (the authoritative signal) and `cat /tmp/worker.log` to see its stdout.

### First exec after `sandbox::create` can hit transient S300 — retry once

The VM keeps booting after `sandbox::create` returns the `sandbox_id`. The first `sandbox::exec` against a freshly-booted VM occasionally fails with `S300 VM boot failed: shell connect: worker relay not accepting connections: connect(...): Connection refused`. This is **transient** — the relay socket hasn't bound yet. Wait ~1s and retry the same exec. Don't recreate the sandbox; that just resets the boot clock.

If the second retry also fails, then the VM really did fail to boot — check `sandbox::list` to confirm the sandbox is still `stopped: false`, and look at the error's `fix_note` for the platform reason (commonly: missing virtualization, see the iii-sandbox README's "Host requirements"). Session `q4czk143` hit S300 twice in a row on the first sandbox and unnecessarily recreated it.

### Run the worker in foreground first, then detach

A clean run order for authored workers, observed in session `acxrdx6g`:

0. **Install the SDK first** — `cd /tmp && npm init -y && npm install iii-sdk`. The `node` image doesn't ship `iii-sdk`. Skipping this step makes the worker fail at `require('iii-sdk')` with `Cannot find module`. Session `q4czk143` skipped this and burned turns on diagnosis.
1. **Use CommonJS by default in `.js` files** — `const { registerWorker } = require('iii-sdk')`, not `import { registerWorker } from 'iii-sdk'`. ESM works only if the package.json sets `"type": "module"` (or the file is `.mjs`); a bare `.js` ESM file fails with `Cannot use import statement outside a module`. The `npm init -y` above creates `"type": "commonjs"` so CJS is the safe default.
2. **Do NOT use TypeScript syntax in .js files.** Session `q4czk143` wrote `process.env.III_ENGINE_URL!` (the TS non-null-assertion operator) and Node parsed it as JS, throwing `SyntaxError: missing ) after argument list`. Plain JS doesn't have `!` as a type assertion — drop it.
3. **Foreground sanity check** — `timeout 5 node /tmp/worker.js 2>&1 || true`. Catches syntax errors, missing imports, bad `III_ENGINE_URL`, etc. Output is right there in the exec response instead of buried in a log file. The `|| true` lets the exec succeed even when timeout kills the process. (Or use `node --check /tmp/worker.js` for a parse-only check; doesn't catch import resolution but is faster.)
4. **Detached launch** — `nohup node /tmp/worker.js > /tmp/worker.log 2>&1 &` only after the foreground run prints "registered and ready" cleanly.
5. **Verify** — `engine::functions::list { search: "<worker-name>" }`. Within 1–2 s of detached launch, your function ids should appear. If they don't, read `/tmp/worker.log` — but if the foreground run was clean, the only remaining failure mode is the `III_ENGINE_URL` boot-rewrite trap (see "Reaching the host engine" above).

This five-step pattern collapses a typical 5–10 turn debug spiral into 5 turns: install deps, choose CJS, no TS syntax, foreground sanity, detach + verify. Sessions `svyuv3id` and `c007a1a7-2116` (both local Qwen3-35B) followed this pattern end-to-end and shipped a full CRUD-plus-HTML todo app on the first deployment attempt.

