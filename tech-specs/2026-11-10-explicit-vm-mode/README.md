---
title: explicit VM mode — Compose runs VM workers through commands it can show
tagline: "A VM worker says so in worker-compose.yaml: scripts.build is iii sandbox build, scripts.run is iii sandbox run. The build runs locally in the sandbox adapter and leaves an image in the project; the run boots that image. Compose stops deciding on its own that a worker needs a VM."
date: 2026-11-10
tags: [compose, sandbox, cli, vm]
status: draft
---

# explicit VM mode — Compose runs VM workers through commands it can show

In 0.26, Compose picks the VM by itself. A registry `bundle`, or a `path://` worker whose
`iii.worker.yaml` sets `runtime.base_image`, runs in a microVM. Compose adds the
`sandbox-libkrun` container, keeps the start order on it in memory, and calls the adapter's
`worker::prepare` and `worker::start` itself. Nothing in `worker-compose.yaml` says that a
worker runs in a VM, how it is built, or how to run it elsewhere.

This spec makes the VM explicit:

```yaml
  onboarding:
    worker: package://api.workers.iii.dev/onboarding
    version: "0.2.0"
    start_after: [sandbox-libkrun]
    scripts:
      build: iii sandbox build
      run: iii sandbox run
```

- `iii sandbox build` asks the sandbox adapter to build an image of the worker, locally, and
  leaves it in `<project>/.build/<container>/`.
- `iii sandbox run` boots that image. Nothing is installed at boot.
- Compose starts both as processes, like any other command. It keeps no VM boot path of its
  own.

Every decision is listed in section 13 with the section that specifies it. "Container" below
always means a container key of `worker-compose.yaml`.

## 1. Goals and non-goals

Goals:

1. The file shows where and how each worker runs. Moving a worker between a VM, the host and
   Docker is an edit of `scripts`.
2. Compose stays a process supervisor. The sandbox adapter is a container like any other,
   reached through `start_after`.
3. Build and run are separate phases. A run never installs dependencies; a restart repeats the
   run only.
4. A worker that ran in a VM never starts on the host without being told to.
5. No Docker is required on the user's machine. The default adapter builds inside its own VM.

Non-goals (this phase):

- Pulling a prebuilt image of a worker. The build is always local.
- Building a `Dockerfile` (workers with `deploy: image`, such as `hermes`): phase 2, section 12.
- Windows: a bundle keeps failing at install with `BUNDLE_NEEDS_A_VM`, as in 0.26, and
  `iii sandbox run`/`build` fail with `SANDBOX_UNSUPPORTED_PLATFORM`.

## 2. What 0.26 does today

| Piece | 0.26 behavior | Where |
| --- | --- | --- |
| VM selection | A `bundle` package, or a `path://` with `runtime.base_image`, is a VM need; `engine.sandbox` for the project's namespace is a need too (`Need::Explicit`) | `crates/iii-compose/src/sandbox.rs` (`needs`, `vm_needs`) |
| `scripts.run` on a VM `path://` | Runs inside the VM in place of the manifest's `scripts.start` (`VmSpec::Local.run_override`) | `manifest.rs:239-259` |
| Explicit opt-in | `scripts.run: iii sandbox run` on a `path://` that lists the adapter in `start_after` runs as a host process (`runs_own_vm`) | `sandbox.rs:184-198` |
| `scripts` keys | `pre_run`, `pre_run_timeout`, `run`, `post_run`; unknown keys are refused | `config.rs` (`Scripts`, `RawScripts`) |
| Packages and `scripts.run` | Refused: `RUN_NOT_ALLOWED_FOR_PACKAGE` | `config.rs:677`, `error.rs:635` |
| Adapter | Compose writes the `sandbox-libkrun` container (`# added by compose::add`) at an exact version and installs it | `sandbox.rs` (`adapter_request`, `adapter_key`) |
| Start order | Every VM container starts after the adapter; the edge is in memory only | `sandbox.rs` (`wire`) |
| Boot | Compose calls `worker::prepare`, then `worker::start`, with `guest_env` | `sandbox.rs` (`start_vm`, `start_vm_until`, `prepare_vm`), called from `lifecycle.rs:1307`; `Child::Vm` / `RemoteVm` |
| Dependencies, `path://` | `scripts.install` runs in the guest on first boot (prepared marker `/var/.iii-prepared`) and persists on the VM's ext4 upper (overlay mode) | `sandbox-libkrun/src/cli/local_worker.rs` |
| Dependencies, bundle | `/workspace` is the package's directory in Compose's global cache, mounted read-write over virtiofs: what the install writes lands on the host | `local_worker.rs:130-155`, `bundle_download.rs` |
| Legacy mode | A cloned rootfs, no overlay upper | `sandbox-libkrun/src/cli/overlay.rs` |
| VM state | `~/.iii/sandbox/workers/<vm_id>/`, `vm_id` derived from the state key | `sandbox-libkrun/src/worker_vm.rs` |
| State key | `sha256(<state dir> \0 <container key>)` | `sandbox.rs` (`state_key`) |
| Packages | Extracted into a global cache, `~/.iii/compose/packages` (or `$III_COMPOSE_STATE_DIR/packages`); the archive itself is not kept | `state.rs` (`package_cache`), `registry.rs` |
| Base images | Pulled from an OCI registry into `~/.iii/cache/<slug>/`, turned into EROFS; a bundle without `runtime.base_image` gets an image chosen by its `kind` | `worker_manager/oci.rs`, `rootfs_cache.rs` |
| `iii sandbox` | `run` and `destroy`; hidden from `--help` since #2506 | `sandbox_cli.rs`, `engine/src/main.rs:187` |
| `iii sandbox run` env | `III_SANDBOX_TOKEN`, `III_SANDBOX_STATE_KEY`, `III_SANDBOX_ADAPTER`, `III_SANDBOX_NAMESPACE`, `III_WORKER_DIR`, `III_WORKER_ENV_KEYS`, `III_STOP_TIMEOUT_MS`. `III_PACKAGE_DIR` / `III_PACKAGE_NAME` exist as constants but nothing sets them | `sandbox.rs` (`layer_own_vm_env`), `sandbox_cli.rs` |
| Ids | `sandbox::*` goes through the engine router; `worker::*` is called directly as `<adapter>::worker::*`, and a missing one is `function_not_found` | `engine/src/workers/sandbox/router.rs`, `sandbox.rs` |
| Guest env | The container's `environment`/`env_file` plus the reserved `III_*`, on both paths since #2505 | `spawn.rs` (`guest_env`) |

`iii sandbox run` (#2457) already boots the VM through `worker::*` with a token scoped to that
VM, prints the guest's output as its own, turns SIGTERM and SIGINT into `worker::stop`,
reattaches to a running instance and exits with the VM's code.

## 3. The model

### 3.1 `worker-compose.yaml`

```yaml
namespace: default
engine:
  url: ws://127.0.0.1:49134

containers:
  # added by compose::add
  sandbox-libkrun:
    worker: package://api.workers.iii.dev/sandbox-libkrun
    version: "0.4.0"

  state:
    worker: package://api.workers.iii.dev/state
    version: "0.23.0"

  # A registry bundle in a VM. compose::add writes start_after and scripts.
  # added by compose::add
  onboarding:
    worker: package://api.workers.iii.dev/onboarding
    version: "0.2.0"
    start_after: [sandbox-libkrun, state]
    scripts:
      build: iii sandbox build
      run: iii sandbox run
    environment:
      OPENAI_API_KEY: ${OPENAI_API_KEY}   # the guest gets only what is declared

  # A local worker in a VM: the base image comes from runtime.base_image in its iii.worker.yaml.
  link:
    worker: path://./link
    start_after: [sandbox-libkrun]
    scripts:
      build: iii sandbox build
      run: iii sandbox run

  # The same kind of worker on the host: moving it is an edit of scripts.
  api:
    worker: path://./api
    scripts:
      build: npm ci && npm run build
      run: node dist/index.js
```

`0.4.0` stands for the first `sandbox-libkrun` release with `worker.build` (section 6.6).

### 3.2 The project tree

```text
my-project/
├── worker-compose.yaml
├── worker-compose.lock
├── .iii/                    # Compose state and the project token (hidden from VMs, as today)
├── .workers/                # packages, one directory per container; like node_modules
│   ├── .gitignore           # "*", written by Compose
│   ├── .packages.json       # container -> { name, version, sha256 }
│   ├── onboarding/
│   ├── sandbox-libkrun/
│   └── state/
├── .build/                  # one directory per container that has a build
│   ├── .gitignore           # "*", written by Compose
│   ├── onboarding/
│   │   ├── image.erofs      # the image the run boots (VM containers)
│   │   ├── image.id         # its build_id, written by the adapter
│   │   └── build.json
│   ├── link/
│   └── api/
│       └── build.json       # host builds record build.json without an image
└── link/                    # a path:// worker, as today

~/.iii/cache/                # global: base images (docker.io/iiidev/node, .../python, ...)
~/.iii/compose/packages/     # global: extracted packages, unchanged; the source of .workers/
~/.iii/sandbox/workers/      # adapter-owned: each VM's persistent data disk
```

- `.workers/` is rebuilt from `worker-compose.lock`, and `.build/` by a build; both stay out of
  git like `node_modules`. Compose writes a `.gitignore` containing `*` in each, so a migrated
  project is covered without editing its own `.gitignore`.
- `.workers/<container>/` is a copy of the extracted package from the global cache. The cache
  layout does not change. `.workers/.packages.json` records which artifact each directory
  holds; it lives outside the package directories, so no guest can write it.
- The image is one file, `image.erofs`, never a directory tree: a Linux tree on a
  case-insensitive filesystem (APFS) would be corrupted.

### 3.3 The flow

1. `iii call compose::add worker=onboarding` resolves the package, writes the container with
   `start_after` and `scripts` (section 7.1), and reconciles.
2. A container whose `scripts.build` has no `.build/<container>/build.json` yet gets its first
   build when it starts (section 5.2), then runs.
3. `scripts.run` starts the worker. For a VM container, `iii sandbox run` boots
   `.build/<container>/image.erofs`.
4. A restart, or an adapter restart, repeats `scripts.run` only.
5. A rebuild is explicit (`iii sandbox build <container>`, section 8.1), followed by a restart
   of that container. `compose::update` of a package rebuilds it (section 5.3).
6. `compose::remove` stops the container, has the adapter destroy the VM's data disk, and
   deletes `.build/<container>/` and `.workers/<container>/`.

## 4. Scripts and validation

### 4.1 `scripts.build`

A new key, beside `pre_run`, `run` and `post_run`:

| Property | Value |
| --- | --- |
| Type | A shell command, run like `scripts.run` (`sh -c` on Unix) |
| Working directory | The container's working directory, as for `scripts.run` |
| Env | The container's env, as for `scripts.run`; for `iii sandbox build`, the guest of the build VM gets `guest_env` (section 9.4) |
| Timeout | `scripts.build_timeout`, default `20m`; outside `startup_timeout`; passed to `iii sandbox build` as `III_BUILD_TIMEOUT_MS` |
| Output | Captured into the container's logs (`compose::logs`), each line prefixed with `[build] ` |
| Failure | `BUILD_FAILED`. From `iii sandbox build`: the code and message of its last line (section 8.1). From any other command: code `EXIT_<status>` (or `TIMED_OUT`), message the last 20 lines of output |
| When | First build (section 5.2), `compose::build` (section 7.4), and a package update (section 5.3) |

`pre_run` keeps running before every start, after any build.

### 4.2 Who may use what

| Rule | Error |
| --- | --- |
| `scripts.run: iii sandbox run` only on a `bundle` package, or on a `path://` whose manifest sets `runtime.base_image` | `SANDBOX_RUN_NOT_A_VM_WORKER` |
| `scripts.build: iii sandbox build` only together with `scripts.run: iii sandbox run` | `SANDBOX_BUILD_WITHOUT_RUN` |
| `scripts.run: iii sandbox run` needs `scripts.build: iii sandbox build` | `SANDBOX_RUN_WITHOUT_BUILD` |
| `iii sandbox run` needs the adapter's container key in `start_after`, in the file | `SANDBOX_ADAPTER_NOT_IN_START_AFTER` |
| A `bundle` package needs a `scripts.run` | `VM_SCRIPTS_MISSING` |
| `iii sandbox run` on Windows | `SANDBOX_UNSUPPORTED_PLATFORM` (a bundle already fails at install with `BUNDLE_NEEDS_A_VM`) |
| `scripts.run`/`scripts.build` on a `binary` package | `RUN_NOT_ALLOWED_FOR_PACKAGE` (kept, now for binaries only) |
| `--command <cmd>` on a bundle | refused by `iii sandbox run`, as today |

A project needs the adapter when a container's `scripts.run` starts with `iii sandbox run`, or
when `engine.sandbox` names the project's own namespace (`Need::Explicit`, unchanged).

These rules apply once the project's migration is done. A file whose migration is due
(section 7.3) is migrated first, not refused.

## 5. Build

### 5.1 What a build produces

| Container | `scripts.build` | Result |
| --- | --- | --- |
| `bundle`, or `path://` with `runtime.base_image` | `iii sandbox build` | `.build/<container>/image.erofs` + `build.json` |
| Host or Docker worker | any command | `.build/<container>/build.json` |
| `deploy: image` (phase 2) | `iii sandbox build` | `image.erofs` from the worker's `Dockerfile` |

The image is a complete root filesystem for that container: the base image, the worker's code
for a bundle, and what `scripts.setup` and `scripts.install` installed. A full rootfs per
container repeats the base image on disk; it keeps the guest at the one lower layer iii-init
mounts today. Sharing the base as a second lower is an open question (section 14).

With `sandbox-libkrun`, a suggested implementation (the contract only fixes the result): boot a
build VM on the base image with a fresh upper, copy the worker's code in, run `scripts.setup`
and `scripts.install`, stream the merged root (without `/proc`, `/sys`, `/dev`, `/run`, `/tmp` and the virtiofs mount
points `/opt/iii` and `/mnt/host-src`) to the
host, and turn it into EROFS with the adapter's existing EROFS builder. Nothing runs on the
host.

### 5.2 First build

- **When:** when the container starts, after its `start_after` containers are ready, if
  `scripts.build` is set and `.build/<container>/build.json` is missing. This covers
  `compose::add`, `up`, and a migrated container.
- **Timeout:** `scripts.build_timeout` (section 4.1), then the container's `startup_timeout`
  starts for the run.
- **Logs:** `<container>: no build yet; running scripts.build`, then the build output.
- **Operations:** `compose::add` and `compose::up` report the build as a progress step of the
  container (`building`), and their operation finishes after the run started or failed.
- **Failure:** the container fails with `BUILD_FAILED` (section 10), carrying the S-code and
  message of the last line `iii sandbox build` printed (section 8.1). Nothing is rolled back,
  as for any failed start.

### 5.3 Rebuilds

- `iii sandbox build <container>` or `iii call compose::build worker=<container>` rebuilds; the
  running container keeps its current image until restarted.
- `compose::add` or `compose::update` that changes a container's package (`source` in
  `build.json` no longer matches the lock) rebuilds it before restarting it, and logs
  `<container>: package changed (<old> -> <new>); rebuilding`.

### 5.4 `build.json`

```json
{
  "schema": 1,
  "container": "onboarding",
  "source": { "kind": "package", "name": "onboarding", "version": "0.2.0", "sha256": "sha256:8c3406..." },
  "command": "iii sandbox build",
  "dependency_files": { "package.json": "sha256:2f1c...", "package-lock.json": "sha256:9a07..." },
  "image": {
    "file": "image.erofs",
    "build_id": "sha256:7d41...",
    "base_image": { "declared": null, "reference": "docker.io/iiidev/node:latest", "digest": "sha256:b1e2..." },
    "adapter": { "name": "sandbox-libkrun", "version": "0.4.0" },
    "contract": { "major": 1, "minor": 3 }
  },
  "built_at": "2026-11-10T14:03:22Z"
}
```

A `path://` container records `"source": { "kind": "path", "dir": "./link" }` (the declared
path, relative to the compose file).

- Every hash is written `sha256:<hex>`; times are RFC 3339 UTC.
- `dependency_files` holds the hash of each of these files that exists at the root of the
  worker directory: `package.json`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`,
  `bun.lockb`, `requirements.txt`, `pyproject.toml`, `uv.lock`, `poetry.lock`, `Cargo.toml`,
  `Cargo.lock`, `go.mod`, `go.sum`.
- `image.base_image.declared` is the manifest's `runtime.base_image`, or `null` when the
  adapter chose the image; `reference` and `digest` are what the adapter reports, `digest`
  `null` when it does not know it.
- **One writer per case.** For a VM build, `iii sandbox build` writes `build.json` after
  `worker::build` returned, which happens only after the adapter renamed the new image into
  place. For a host build, Compose writes it after `scripts.build` exited 0.
- **Atomic writes:** `build.json.tmp`, then rename. A `build.json` whose `build_id` does not
  match the image fails the run with `S508` (section 6.5); the fix is a rebuild.

### 5.5 A build that is out of date

Before `scripts.run` (on `up`, a restart, or a crash replacement), Compose compares the
container's current inputs with `build.json`: `source`, `dependency_files`, and
`image.base_image.declared`. A difference in `dependency_files` or `base_image.declared` only
logs, once per start, one line per kind of change:

```text
link: dependencies changed since the last build (package.json); run `iii sandbox build link`
link: base image changed since the last build (docker.io/iiidev/node -> docker.io/iiidev/python); run `iii sandbox build link`
```

and the container runs on its current build. A change of `source` is a package update and
rebuilds (section 5.3). A change of code alone (outside the dependency files) is not a change:
a `path://` worker's code keeps syncing live into the guest (section 9.3).

## 6. Sandbox contract 1.3

All changes are additive. `CONTRACT_MAJOR` stays 1; `CONTRACT_MINOR` becomes 3. Unknown fields
are ignored by both sides, as today.

### 6.1 Feature

```rust
/// `worker::build`, and `worker::start` booting the image it built. Since contract 1.3.
pub const FEATURE_BUILD: &str = "worker.build";
```

Advertised in `AdapterInfo.features`. An adapter advertises it only when it can boot a build
(overlay mode); `sandbox-libkrun` in legacy mode (`III_ROOTFS_MODE=legacy`, or a build without
an embedded iii-init) does not, and the `SANDBOX_BUILD_UNSUPPORTED` message names that case.

### 6.2 `worker::build`

New operation suffix `WORKER_BUILD = "worker::build"`, called directly as
`<adapter>::worker::build`. It is a constant beside the other `WORKER_*` suffixes and joins the
second assert of `operations_follow_daemon_order`. It is not added to `OPERATIONS`, the list of
ephemeral `sandbox::*` operations the router publishes.

```rust
/// Builds the image of one container's VM; long-running. Every call builds anew.
pub struct WorkerBuildRequest {
    /// The project token or the VM's scoped token.
    pub auth: String,
    /// The VM the build is for; a scoped token is checked against it.
    pub state_key: String,
    /// Container key, for logs and errors.
    pub worker_name: String,
    pub source: WorkerSource,
    /// Absolute host directory of the build: `<project>/.build/<container>`. Must be under the
    /// adapter's host roots. The adapter writes `image.erofs` there through a temporary file
    /// and a rename.
    pub output_dir: String,
    /// Env of the setup/install steps: `guest_env`, nothing from the host.
    pub env: BTreeMap<String, String>,
    /// Writer end of a channel for the build's output, one `LogRecord` JSON per text frame.
    /// Required: the build lives as long as the caller reads it.
    pub logs: StreamChannelRef,
}

pub struct WorkerBuildResponse {
    /// Digest of the image written, `sha256:<hex>`; passed back on `worker::start`.
    pub build_id: String,
    /// The base image the build started from.
    pub base_image: BaseImageRef,
    pub built_at_ms: u64,
}

pub struct BaseImageRef {
    pub reference: String,
    /// Resolved digest, `None` when the cached rootfs carries none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}
```

- **Always builds.** No input-based skip, so a rebuild picks up a moved `:latest` base.
- **Output files:** `image.erofs` and `image.id` (the `build_id`) are written as temporary
  files, then renamed: `image.erofs` first, `image.id` last.
- **The running VM is not touched.** A build may run while the VM of the same `state_key`
  runs; it uses its own build VM and swaps `image.erofs` by rename, so the running VM keeps
  the file it booted.
- **One build per `state_key` at a time:** a second one gets `S509`.
- **Timeout and cancel:** the build lives as long as its caller reads `logs`. The adapter
  writes to the channel at least every 5 s (an empty keepalive frame, which readers skip). When
  the channel closes or a write fails, it stops the build VM, deletes the temporary image,
  keeps the previous `image.erofs` and `image.id`, and releases the `state_key`, so a retry
  does not wait on `S509`. `iii sandbox build` enforces `III_BUILD_TIMEOUT_MS` by closing the
  channel and failing with `TIMED_OUT`; Compose stops a build by stopping that process.
- **No prepare:** the build pulls its base image itself, like `worker::prepare` did.

### 6.3 `worker::start` boots a build

```rust
pub struct WorkerStartRequest {
    // ... unchanged fields ...
    /// Boot the image `worker::build` wrote, without running `scripts.setup`/`scripts.install`.
    /// Since contract 1.3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<WorkerBuildRef>,
}

pub struct WorkerBuildRef {
    pub build_id: String,
    /// The `output_dir` the build was written to.
    pub build_dir: String,
}
```

With `build`, the caller does not call `worker::prepare`. The adapter compares `build_id`
with `build_dir/image.id` and refuses a mismatch or a missing file with `S508`; it does not
hash the image on start.

### 6.4 `WorkerSource::Bundle.host_path`

Now the container's directory in `.workers/` (section 7.5), no longer the global cache. The
adapter copies it into the image at build time; a run no longer mounts it.

### 6.5 New S-codes

| Code | Name | Category | When |
| --- | --- | --- | --- |
| `S507` | `BuildFailed` | execution | `scripts.setup`/`scripts.install` exited non-zero, or the image could not be written; the message carries the tail of the output |
| `S508` | `BuildMismatch` | validation | `worker::start.build.build_id` does not match the image |
| `S509` | `BuildInProgress` | validation (like `S502`) | A build for this `state_key` is already running |

Each gets a row in `tests/fixtures/sandbox_error_codes.json` and an anchor in
`docs/next/creating-workers/sandboxes.mdx` (`every_code_has_a_docs_anchor`).

### 6.6 Versions

- `iii-sandbox-contract` gets the types above, `FEATURE_BUILD`, the S-codes, wire examples in
  `tests/fixtures/wire_examples.json`, and `CONTRACT_MINOR = 3`.
- `sandbox-libkrun` does not depend on the crate: it keeps its own copy of the types
  (`src/wire.rs`) and of the fixtures (`tests/fixtures`), checked by its wire tests. Both are
  updated in KAN-283; no release order between the crate and the adapter is required.
- `sandbox-libkrun` advertises `worker.build` from its next minor release (written here as
  0.4.0). Compose's `ADAPTER_RANGE` moves to `^0.4`.

## 7. Compose

### 7.1 Writing the explicit mode

`compose::add`, and the migration (section 7.3), write for a VM container:

- `start_after` gains the adapter's container key, persisted in the file;
- `scripts.build: iii sandbox build`;
- `scripts.run: iii sandbox run`, or, when the container already had a `scripts.run` that 0.26
  ran inside the VM (`run_override`), `scripts.run: iii sandbox run --command '<that command>'`.
  The command is single-quoted, each `'` written as `'\''`, with a test.

Other keys the user set are kept. The in-memory edge (`wire`) is gone.

### 7.2 The adapter before the migration

Every 0.26 project pins `sandbox-libkrun` to a 0.3.x release, which lacks `worker.build`.
In a project with a VM container (one whose `scripts.run` is `iii sandbox run`, or one the
migration of section 7.3 will rewrite), at the points where section 7.3 runs and before
migrating:

- a generated adapter container (`# added by compose::add`) outside `ADAPTER_RANGE` is updated
  to the newest release in range, with the lock;
- an adapter container the operator declared outside the range fails with
  `SANDBOX_ADAPTER_TOO_OLD`, naming the container, its range and the command
  `iii call compose::update worker=<adapter>@latest` (or editing `version`);
- under `--frozen`, an adapter outside the range fails with `SANDBOX_ADAPTER_TOO_OLD` too.

A project without a VM container (ephemeral sandboxes only, `Need::Explicit`) keeps its
adapter version: 0.3.x serves those.

`iii sandbox run` and `iii sandbox build` require `worker.build` from the adapter
(`REQUIRED_FEATURES` in `sandbox_cli.rs`) and fail with `SANDBOX_BUILD_UNSUPPORTED` otherwise.

### 7.3 Migration on run

There is no implicit VM path. Instead, a project is migrated once.

- **When it is due:** `worker-compose.lock` is missing or at version 1. Compose 0.27 reads
  version 1 and writes version 2, also for a project without packages, so a migrated file is
  never migrated again and moving a worker to the host later stays an edit of `scripts`.
- **Where it runs:** `compose::up` (and `iii compose --up`), `compose::add`,
  `compose::update`, `compose::restart`, `compose::build` and `iii compose build`. Read-only
  paths (`compose::validate`, `compose::ps`, `compose::status`, `iii compose logs`),
  `compose::down` and `compose::remove` never edit the file; `compose::validate` reports a due
  migration as a warning naming the containers.
- **Which containers:** every container that 0.26 ran in a VM: a `bundle` package, or a
  `path://` whose manifest sets `runtime.base_image`, whatever its `scripts.run` (none, a
  command, or the 0.26 opt-in `iii sandbox run`). Only those change, and only with the
  pieces of section 7.1 they lack. A project with none of them only gets its lock rewritten.

For each of them:

1. Compose rewrites it as in section 7.1 (`edit.rs`, comments and the rest of the file kept),
   after the adapter step of section 7.2, and logs
   `<container>: migrated to the explicit VM mode (scripts.build/scripts.run: iii sandbox ...)`.
2. When the container starts: the adapter is ready (its `start_after`); a VM that a 0.26
   daemon left running (a `vm` record in `state.json`) is stopped with `worker::stop` and the
   record dropped; then the first build (section 5.2) and the run.
3. The data the 0.26 VM wrote is not deleted. The adapter tells the disks apart on the host:
   the first `worker::start` with `build` writes a seal file, `build-mode`, in the VM's
   directory. A start with `build` that finds an `upper.ext4` without the seal renames it to
   `upper.ext4.pre-build`, creates a new upper, writes the seal and logs the old disk's path.
   The 0.26 disk mixes installed dependencies with data, and would shadow the build's
   dependencies if kept as the upper. A legacy-mode directory (a cloned rootfs, no
   `upper.ext4`) is left in place, unused, and logged the same way. `iii sandbox destroy`
   removes all of it.

If the stop or the build fails, the container fails with that error and the file stays
migrated; the next `up` retries from step 2.

`--frozen` never edits the file or the lock. A project whose migration would only rewrite the
lock runs as is. A file whose migration would rewrite containers fails under `--frozen` with
`VM_SCRIPTS_MISSING`, naming the containers and asking for one `iii compose --up` without
`--frozen`.

### 7.4 `compose::build`

A new project function, a mutation like `compose::restart`:

```text
iii call compose::build [file=<path>] [worker=<container>]
```

- Request: `file` (as every project function), `worker` (a container key; omitted, every
  container with `scripts.build`), `operation_id` (optional, as for other mutations).
- It is asynchronous: it answers `{ operation_id, status: "accepted", requested }`, holds the project's
  mutation lock while it runs, and reports progress per container (`building`, `built`,
  `failed`) through `compose::on-operation-progress`.
- It starts the adapter first when the project is down, as `compose::remove` does.
- The finished operation carries
  `results: [{ worker, status: "built" | "failed", build_id?, error?: { code, message } }]`.
- It never restarts a container.
- `compose::cancel` of that operation and `compose::down` stop it: Compose stops the
  `iii sandbox build` processes, which closes their channels and aborts the builds (section
  6.2). `down` cancels a running `compose::build` first, then takes the lock.

### 7.5 Package directories

Installing or updating a package copies the extracted package from the global cache into
`<project>/.workers/<container>/` and updates `.workers/.packages.json`. `iii compose build`
(offline) does the same from the lock. `WorkerSource::Bundle.host_path` and `III_PACKAGE_DIR`
point at that directory. The compose file's directory is already a host root of the adapter.

### 7.6 What stays and what goes

| Goes | Stays |
| --- | --- |
| `Child::Vm`, `RemoteVm`, `start_vm`, `start_vm_until`, `prepare_vm`, `prepare_own_vm`, `wire` | `own_vm_running`, `stop_own_vm`, `worker_status`, `worker_stop`: on restart, `down` and readiness they stop the VM of a dead `iii sandbox run` or let the supervised retry reattach, and avoid a false `CONTAINER_NAME_TAKEN` |
| `StartSpec::Vm` / `VmSpec` (`manifest.rs`) | `worker::destroy` from `compose::remove` and `iii sandbox destroy` |
| `check_info` (its only caller is `prepare_vm`); the CLI keeps its own `check_adapter` | `adapter_gone` and the adapter-restart cascade of #2469, now over processes: restarting the adapter restarts the containers whose `scripts.run` is `iii sandbox run`, without spending their `restart` attempts |
| | `vm_reconciliation`, only to stop a VM a 0.26 daemon left (section 7.3, step 2) |

A 0.26 `state.json` with a `vm` record is read once, its VM stopped, the record dropped.

## 8. CLI

### 8.1 `iii sandbox build`

```text
iii sandbox build [<container>] [--file <path>] [--namespace <ns>] [--engine <url>]
```

- **Inside Compose** (the env of section 8.3 is set): builds its own container. It calls
  `adapter::info` (requires `worker.build`), then `worker::build` with the build output
  streamed to stdout, then writes `build.json` (section 5.4) from `build_record::source` and
  `build_record::declared_base_image` (section 11.2).
- **Outside Compose:** calls `compose::build worker=<container>` on the project's daemon,
  found as `iii compose logs` finds it (`--file`, `--namespace`, `--engine`, `III_URL`, through
  `resolve_project_target`, section 11.2), prints
  the operation's progress and the container's build output, and waits for the result. No
  engine or no loaded project: the errors `iii compose logs` gives today (`ENGINE_CALL_FAILED`,
  `UNKNOWN_PROJECT`).
- **Exit codes:** 0 built; 1 the build failed; 125 `iii sandbox` itself failed (unreachable
  engine, adapter without the feature, bad env).
- **Last line on failure**, parseable by Compose for `BUILD_FAILED`:
  `iii sandbox: build failed: <S-code or error code>: <message>`.

### 8.2 `iii sandbox run` and `destroy`

- `run` reads `build.json` from `III_BUILD_DIR`, calls `worker::start` with `build`, and skips
  `worker::prepare`. No `build.json`: `BUILD_MISSING`, exit 125.
- `destroy` as today; it also removes `upper.ext4.pre-build` (section 7.3).
- `iii sandbox` is visible in `iii --help` again (it was hidden for 0.26 by #2506), and the CLI
  reference is regenerated.

### 8.3 Env

Compose hands `iii sandbox build` and `iii sandbox run` the variables of section 2, plus:

- `III_BUILD_DIR` = `<project>/.build/<container>`;
- `III_BUILD_TIMEOUT_MS` = `scripts.build_timeout`, for `iii sandbox build`;
- `III_PACKAGE_DIR` = `<project>/.workers/<container>` and `III_PACKAGE_NAME`, set for a
  bundle for the first time.

Constants in `sandbox_cli.rs`: `BUILD_DIR_ENV` and `BUILD_TIMEOUT_ENV` (new),
`PACKAGE_DIR_ENV` and `PACKAGE_NAME_ENV` (existing). `III_BUILD_DIR` and
`III_BUILD_TIMEOUT_MS` join `NEVER_FORWARDED`, where the package pair already is. All four
join `spawn::RESERVED_ENV`, and the filter in `layer_own_vm_env` keeps reserved names out of
`III_WORKER_ENV_KEYS`.

## 9. The guest

### 9.1 Data the worker writes

The VM's data disk is persistent and independent of the build. It keeps whatever the worker
writes (its home, databases, caches) across restarts and rebuilds; only `iii sandbox destroy`
and `compose::remove` reset it. It stays adapter-owned under
`~/.iii/sandbox/workers/<vm_id>/`.

The guest's root is the build image, read-only, with the data disk on top (overlay). A file
the worker modified inside a directory the build also writes (for example under
`node_modules`) keeps the worker's copy after a rebuild; `iii sandbox destroy` clears it.

### 9.2 Swapping the lower under a kept upper

overlayfs accepts a changed lower under an existing upper only without `index`, `metacopy`,
`redirect_dir` and `xino`. iii-init mounts the root with those off, and KAN-283 tests a rebuild
under an upper that already holds files.

### 9.3 Code and dependencies

- A bundle's code is in the image; its `.workers/<container>/` is the build's input only.
- A `path://` worker's code keeps syncing live into the guest. `.build/`, `.workers/` and the
  dependency directories (`node_modules`, `.venv`, `target`, and the rest of the adapter's
  ignore list) are excluded from the copy and the sync, including for `path://.` (lists in
  `local_worker.rs` and `source_watcher.rs`). Adding a dependency: edit `package.json`, run
  `iii sandbox build <container>`, restart the container.

### 9.4 Env

Unchanged from #2505: the guest gets `guest_env` (the container's `environment`/`env_file` and
the reserved `III_*`), never the host's environment. The build VM gets the same set.

## 10. Errors and logs

Compose and CLI codes:

| Code | When |
| --- | --- |
| `VM_SCRIPTS_MISSING` | `--frozen` on a file whose migration would rewrite containers; a `bundle` without `scripts.run` |
| `SANDBOX_ADAPTER_TOO_OLD` | The adapter is outside `ADAPTER_RANGE` and Compose may not update it |
| `SANDBOX_BUILD_UNSUPPORTED` | The adapter does not advertise `worker.build` |
| `SANDBOX_RUN_NOT_A_VM_WORKER` | `iii sandbox run` on a container that is not a bundle or a `path://` with `runtime.base_image` |
| `SANDBOX_BUILD_WITHOUT_RUN` | `iii sandbox build` without `iii sandbox run` |
| `SANDBOX_ADAPTER_NOT_IN_START_AFTER` | `iii sandbox run` without the adapter in `start_after` |
| `SANDBOX_RUN_WITHOUT_BUILD` | `iii sandbox run` without `scripts.build: iii sandbox build` |
| `SANDBOX_UNSUPPORTED_PLATFORM` | `iii sandbox run` or `build` on Windows |
| `BUILD_MISSING` | `iii sandbox run` without `build.json` |
| `BUILD_FAILED` | A first build or a package rebuild failed; carries `{ code, message }` (section 4.1) |
| `RUN_NOT_ALLOWED_FOR_PACKAGE` | Kept, for `binary` packages only |

S-codes: `S507`, `S508`, `S509` (section 6.5).

Logs:

- `<c>: no build yet; running scripts.build`
- `<c>: package changed (<old> -> <new>); rebuilding`
- ``<c>: dependencies changed since the last build (<files>); run `iii sandbox build <c>` ``
- ``<c>: base image changed since the last build (<old> -> <new>); run `iii sandbox build <c>` ``
- `<c>: migrated to the explicit VM mode (scripts.build/scripts.run: iii sandbox ...)`
- `compose::validate` warning: `migration due: <c>, <c>; the next up rewrites them`
- adapter: `<c>: kept the pre-build data disk at <path>`

## 11. Delivery

### 11.1 Tickets and ownership

Parent ticket: KAN-277. Every file has one owner, except
`docs/next/creating-workers/sandboxes.mdx`: its S-code anchors belong to KAN-279, the rest to
KAN-285.

| Ticket | Owner | Files |
| --- | --- | --- |
| KAN-278 spec | orchestrator | `tech-specs/2026-11-10-explicit-vm-mode/` |
| KAN-279 contract | orchestrator | `crates/iii-sandbox-contract/**`; S-code anchors in `docs/next/creating-workers/sandboxes.mdx` |
| KAN-280 Compose: explicit mode, migration, `compose::build`, removal | agent | every file of `crates/iii-compose/src/` not listed for KAN-281 or KAN-282 (among them `sandbox`, `config`, `edit`, `daemon`, `lifecycle`, `project`, `manifest`, `spawn`, `state`, `error`, `remote`, `lib`, `cli`, `lockfile`, `report`, `logs`, `operation`, `ps`); `crates/iii-compose/tests/*.rs` except `project_cache.rs`; `engine/tests/compose_sandbox_e2e.rs` (including `worker::build` in its fake adapter) |
| KAN-281 Compose: `.workers/`, `build.json` | agent | `crates/iii-compose/src/{registry,build}.rs`, new `crates/iii-compose/src/build_record.rs`, `crates/iii-compose/tests/project_cache.rs` |
| KAN-282 CLI | agent | `crates/iii-compose/src/sandbox_cli.rs` (with its unit tests), `engine/src/main.rs`, `docs/next/cli-reference/` |
| KAN-283 `sandbox-libkrun` | agent | `iii-hq/workers`: `sandbox-libkrun/**` (including iii-init, `src/wire.rs`, fixtures) |
| KAN-285 docs | agent | `docs/next/**` except the CLI reference and the S-code anchors; the upgrade guide is `docs/next/upgrading/from-0-26-x.mdx` |
| KAN-286 templates | agent | `iii-hq/templates` |
| KAN-284 integration | orchestrator | integration branches, end-to-end runs |

### 11.2 Interfaces fixed between tickets

So KAN-280, KAN-281 and KAN-282 can run in parallel, the orchestrator first lands a scaffold
commit on `feat/explicit-vm-mode`: these signatures with unimplemented bodies, and the env
constants of section 8.3. Each ticket then fills in its own.

```rust
// crates/iii-compose/src/build_record.rs (KAN-281)
pub struct BuildRecord { /* the fields of section 5.4 */ }
pub enum BuildSource {
    Package { name: String, version: String, sha256: String },
    Path { dir: String }, // as declared, relative to the compose file
}
pub enum Change {
    Source { old: BuildSource, new: BuildSource },
    Dependencies(Vec<String>), // file names
    BaseImage { old: Option<String>, new: Option<String> },
}
pub fn read(build_dir: &Path) -> Result<Option<BuildRecord>>;
pub fn write(build_dir: &Path, record: &BuildRecord) -> Result<()>; // tmp + rename
pub fn source(compose_dir: &Path, container: &str) -> Result<BuildSource>; // .packages.json or the compose file
pub fn declared_base_image(worker_dir: &Path) -> Result<Option<String>>; // runtime.base_image
pub fn dependency_files(worker_dir: &Path) -> Result<BTreeMap<String, String>>;
/// Every difference of section 5.5; empty when the build is up to date.
pub fn changes(record: &BuildRecord, compose_dir: &Path, container: &str, worker_dir: &Path) -> Result<Vec<Change>>;
pub fn build_dir(compose_dir: &Path, container: &str) -> PathBuf; // <compose dir>/.build/<container>
pub fn package_dir(compose_dir: &Path, container: &str) -> PathBuf; // <compose dir>/.workers/<container>

// crates/iii-compose/src/registry.rs (KAN-281)
/// Makes `.workers/` match the lock: copies missing or changed packages from the cache, removes
/// the directories of containers no longer locked, rewrites `.workers/.packages.json`.
pub async fn materialize_project(compose_dir: &Path, cache_root: &Path, lock: &PreparedLock) -> Result<()>;

// crates/iii-compose/src/sandbox.rs (KAN-280)
/// Section 7.3: rewrites the containers whose migration is due; `None` when none is.
pub async fn migrate(path: &Path, edited: &mut String, file: &mut ComposeFile, prepared: &mut PreparedLock) -> Result<Option<Migration>>;

// crates/iii-compose/src/lib.rs (KAN-280)
pub struct ProjectTarget { pub engine_url: String, pub daemon_namespace: String, pub file: Option<PathBuf> }
/// The discovery `iii compose logs` does today, shared with `iii sandbox build`.
pub fn resolve_project_target(file: Option<&Path>, namespace: Option<String>, engine: Option<String>) -> Result<ProjectTarget>;
```

`.workers/.packages.json`:

```json
{ "schema": 1, "containers": { "onboarding": { "name": "onboarding", "version": "0.2.0", "sha256": "sha256:8c3406..." } } }
```

- `materialize_project` runs after every package install: from `lifecycle.rs` (KAN-280) and
  `build.rs` (KAN-281).
- `iii compose build` (KAN-281) calls `sandbox::migrate` before installing.
- `compose::build`: section 7.4 (KAN-280); the CLI calls it outside Compose (KAN-282).
- The first-build and out-of-date checks (KAN-280) call `build_record::changes`.

### 11.3 Process

For every ticket:

1. One agent implements it in its own worktree and branch, with its own `CARGO_TARGET_DIR`, and
   reports on the ticket.
2. A reviewer agent reviews it against this spec and the ticket's acceptance criteria and lists
   what must change.
3. The orchestrator weighs the review. What holds goes back to the same agent; what does not is
   answered on the ticket with the reason.
4. Steps 2 and 3 repeat until the reviewer is satisfied.
5. Only then a draft PR, in English.

Branches: in `iii`, every ticket's branch opens its draft PR into the integration branch
`feat/explicit-vm-mode`. `workers` (KAN-283) and `templates` (KAN-286) each work on one
`feat/explicit-vm-mode` branch with a draft PR into `main`. KAN-284 runs the three together and
opens the `iii` integration PR into `main`, draft first.

KAN-284 closes the phase with: `compose_sandbox_e2e` on real VMs; the Linkly tutorial
(`docs/next/tutorials/linkly/_workspace/run.sh all`); a 0.26 project migrating on its first
`up`, its old data disk kept as `upper.ext4.pre-build`; and the host-env leak check of #2505.

### 11.4 Order

1. KAN-278, then KAN-279, then the scaffold commit of section 11.2.
2. KAN-280, KAN-281, KAN-282, KAN-283 in parallel, against section 11.2; KAN-285 drafts.
3. KAN-286 once KAN-280 fixes the file format; KAN-285 final.
4. KAN-284.

## 12. Phase 2 (KAN-287)

- `worker::build` from a `Dockerfile`, inside the build VM, for `deploy: image` workers.
- `hermes`: published as source (`Dockerfile` and `src/`) instead of only an image on `ghcr.io`;
  its manifest declares `state` and `http`; its README stops advertising a `compose::add` that
  fails today.
- The `workers` release pipeline and the registry carry an `image` worker as source.

## 13. Decisions

| # | Decision | Section |
| --- | --- | --- |
| 1 | Explicit commands (`iii sandbox build` / `iii sandbox run`), not a `runner:` field | 3.1, 7.1 |
| 2 | The build is always local, through `worker::build`; the default adapter builds in its own VM; Docker is an adapter's choice | 5.1, 6.2 |
| 3 | The run boots the build; nothing is installed at boot | 6.3, 8.2 |
| 4 | `.workers/` and `.build/` per project, at the root, out of git; base images and extracted packages stay in global caches | 3.2, 7.5 |
| 5 | `.workers/` and `.build/` are keyed by container | 3.2 |
| 6 | The first build is automatic when a container starts without one; rebuilds are explicit, except a package update | 5.2, 5.3 |
| 7 | A `path://` worker's code syncs live; its dependencies come from the build | 9.3 |
| 8 | The data the worker writes is persistent and independent of the build | 9.1 |
| 9 | An out-of-date build only logs | 5.5 |
| 10 | No implicit VM path: old files migrate when they run, one container at a time | 7.3 |
| 11 | The migration keeps the 0.26 data disk as `upper.ext4.pre-build` instead of deleting it | 7.3 |
| 12 | Compose updates a generated adapter that is too old before migrating, only in projects with a VM container | 7.2 |
| 13 | Contract 1.3, additive, with `sandbox-libkrun` 0.4 as the minimum | 6.6 |
| 14 | `scripts` on packages: allowed for bundles, still refused for binaries | 4.2 |
| 15 | Prebuilt images are out of scope | 1 |
| 16 | The migration runs once per project, keyed on the lock version (2) | 7.3 |
| 17 | A build lives as long as its caller reads its output; stopping the caller cancels it | 6.2, 7.4 |

## 14. Open questions

1. Whether a manifest may extend the dependency file list of section 5.4.
2. Sharing the base image as a second lower layer instead of a full rootfs per container
   (section 5.1), which needs iii-init to mount two lowers.
