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
  leaves it in `<project>/.iii/build/<container>/`.
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
    version: "0.5.0"

  state:
    worker: package://api.workers.iii.dev/state
    version: "0.23.0"

  # A registry bundle in a VM. compose::add writes start_after and scripts.
  # added by compose::add
  onboarding:
    worker: package://api.workers.iii.dev/onboarding
    version: "0.2.0"
    start_after: [state, sandbox-libkrun]
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

  # A registry binary in a VM: its Linux build, on the image the container names.
  # added by compose::add
  indexer:
    worker: package://api.workers.iii.dev/indexer
    version: "1.2.0"
    start_after: [sandbox-libkrun]
    runtime:
      base_image: docker.io/library/alpine:3.20
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

`0.5.0` stands for the first `sandbox-libkrun` release with contract 1.4: `worker.build`,
`worker.binary` and `worker.prune` (sections 6.6, 6.7).

`runtime.base_image`, a new container key, names the image a container's VM build starts from,
over the `runtime.base_image` of the worker's `iii.worker.yaml`. It must be an image reference
(`INVALID_BASE_IMAGE`) on a container that runs in a VM: its `scripts.run` is `iii sandbox run`
and its `scripts.build` is `iii sandbox build`, or either is still to be written (section 7.1).
A host `scripts.run`, or a `scripts.build` other than `iii sandbox build` (`""` included),
fails with `BASE_IMAGE_WITHOUT_VM`: unlike the manifest's key, the container's never keeps a
worker on the host (section 7.3). A binary package names no image, so a binary runs in a VM only
with it.

A `path://` whose manifest sets `runtime.base_image` runs on the host only when it declares
`scripts.build`, `""` when there is nothing to build:

```yaml
  link:
    worker: path://./link   # its iii.worker.yaml sets runtime.base_image
    scripts:
      build: ""
      run: node index.js
```

Without `scripts.build`, Compose rewrites it into the VM, as 0.26 ran it (section 7.3). A
`build:` key without a value counts as absent.

### 3.2 The project tree

```text
my-project/
├── worker-compose.yaml
├── worker-compose.lock
├── .iii/                    # hidden from VMs by name, as today
│   ├── project.ini          # the project's id, as today
│   ├── compose/             # Compose state and the project token; III_COMPOSE_STATE_DIR moves it
│   ├── workers/             # packages, one directory per container; like node_modules
│   │   ├── .gitignore       # "*", written by Compose
│   │   ├── .packages.json   # container -> { name, version, sha256 }
│   │   ├── .lock            # held while Compose copies packages
│   │   ├── onboarding/
│   │   ├── sandbox-libkrun/
│   │   └── state/
│   └── build/               # one directory per container that has a build
│       ├── .gitignore       # "*", written by Compose
│       ├── onboarding/
│       │   ├── image.erofs  # the image the run boots (VM containers)
│       │   ├── image.id     # its build_id, written by the adapter
│       │   └── build.json
│       ├── link/
│       └── api/
│           └── build.json   # host builds record build.json without an image
└── link/                    # a path:// worker, as today

~/.iii/cache/                # global: base images (docker.io/iiidev/node, .../python, ...)
~/.iii/compose/packages/     # global: extracted packages, unchanged; the source of .iii/workers/
~/.iii/sandbox/workers/      # adapter-owned: each VM's persistent data disk
```

- `.iii/workers/` and `.iii/build/` sit in the `.iii/` beside the compose file, also when
  `III_COMPOSE_STATE_DIR` moves Compose's state elsewhere: the adapter reads only under the
  compose file's directory and the package cache (its host roots). Being in `.iii/`, they are
  never copied or synced into a guest, `path://.` included, and the project root gets no
  directory of Compose's.
- `.iii/workers/` is rebuilt from `worker-compose.lock`, and `.iii/build/` by a build; both stay
  out of git like `node_modules`. Compose writes a `.gitignore` containing `*` in each, so a
  migrated project is covered without editing its own `.gitignore`, which does not ignore
  `.iii/` as a whole (it holds `project.ini`).
- `.iii/workers/<container>/` is a copy of the extracted package from the global cache. The
  cache layout does not change. `.iii/workers/.packages.json` records which artifact each
  directory holds; it lives outside the package directories, so no guest can write it.
- The image is one file, `image.erofs`, never a directory tree: a Linux tree on a
  case-insensitive filesystem (APFS) would be corrupted.

### 3.3 The flow

1. `iii call compose::add worker=onboarding` resolves the package, writes the container with
   `start_after` and `scripts` (section 7.1), and reconciles.
2. A container whose `scripts.build` has no `.iii/build/<container>/build.json` yet gets its
   first build when it starts (section 5.2), then runs.
3. `scripts.run` starts the worker. For a VM container, `iii sandbox run` boots
   `.iii/build/<container>/image.erofs`.
4. A restart, or an adapter restart, repeats `scripts.run` only.
5. A rebuild is explicit (`iii sandbox build <container>`, section 8.1), followed by a restart
   of that container. `compose::update` of a package rebuilds it (section 5.3).
6. `compose::remove` stops the container, has the adapter destroy the VM's runtime state while
   keeping its data disk (`prune=true` deletes that too, section 8.2), and deletes
   `.iii/build/<container>/` and `.iii/workers/<container>/`.

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
| Nothing to build | `scripts.build: ""` declares that there is no build step: nothing runs and no `build.json` is written |
| Failure | `BUILD_FAILED`. From `iii sandbox build`: the code and message of its last line (section 8.1). From any other command: code `EXIT_<status>` (or `TIMED_OUT`), message the last 20 lines of output |
| When | First build (section 5.2), `compose::build` (section 7.4), and a package update (section 5.3) |

`pre_run` keeps running before every start, after any build.

### 4.2 Who may use what

| Rule | Error |
| --- | --- |
| `scripts.run: iii sandbox run` only on a `bundle` package, a `binary` package, or a `path://` with `runtime.base_image` (in its manifest or its container) | `SANDBOX_RUN_NOT_A_VM_WORKER` |
| `scripts.run: iii sandbox run` on a `binary` package without the container's `runtime.base_image` | `SANDBOX_BINARY_WITHOUT_BASE_IMAGE` |
| The container's `runtime.base_image` with a `scripts.run` other than `iii sandbox run`, or a `scripts.build` other than `iii sandbox build` (`""` included) | `BASE_IMAGE_WITHOUT_VM` |
| `runtime.base_image` that is not an image reference (empty, or with whitespace) | `INVALID_BASE_IMAGE` |
| `scripts.build: iii sandbox build` only together with `scripts.run: iii sandbox run` | `SANDBOX_BUILD_WITHOUT_RUN` |
| `scripts.run: iii sandbox run` with a `scripts.build` other than `iii sandbox build` | `SANDBOX_RUN_WITHOUT_BUILD` |
| `iii sandbox run` on Windows | `SANDBOX_UNSUPPORTED_PLATFORM` (a bundle already fails at install with `BUNDLE_NEEDS_A_VM`) |
| `scripts.*` on a `binary` package on the host | Allowed: `scripts.run` replaces the published executable, run in today's working directory; `RUN_NOT_ALLOWED_FOR_PACKAGE` is gone |
| `--command <cmd>` on a bundle or a binary | refused by `iii sandbox run`: a registry package runs what it was published with |

A project needs the adapter when a container's `scripts.run` starts with `iii sandbox run`, or
when `engine.sandbox` names the project's own namespace (`Need::Explicit`, unchanged).

These rules apply after the rewrite of section 7.3: a missing `scripts.build`, a missing
adapter in `start_after`, a bundle without `scripts.run`, or a container with
`runtime.base_image` and no `scripts.run` is rewritten, not refused.

Which build of a binary package the lock resolves and installs depends on where it runs, so
the platform of that build is decided from the declaration alone, before the package kind is
known (`vm_binary::platform`, section 7.5): a VM's when the container's `scripts.run` is
`iii sandbox run`, or when it has no `scripts.run` and sets `runtime.base_image`; the host's
otherwise. This decides a build, not which containers run in a VM: a 0.26 bundle without
`scripts.run` reads as the host's here, has one archive for both, and is still rewritten into a
VM (section 7.3).

## 5. Build

### 5.1 What a build produces

| Container | `scripts.build` | Result |
| --- | --- | --- |
| `bundle`, `binary` with `runtime.base_image`, or `path://` with `runtime.base_image` | `iii sandbox build` | `.iii/build/<container>/image.erofs` + `build.json` |
| Host or Docker worker | any command | `.iii/build/<container>/build.json` |
| `deploy: image` (phase 2) | `iii sandbox build` | `image.erofs` from the worker's `Dockerfile` |

The image is a complete root filesystem for that container: the base image, the worker's code
for a bundle or a binary, and what `scripts.setup` and `scripts.install` installed. A full rootfs per
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
  `scripts.build` is set and non-empty and `.iii/build/<container>/build.json` is missing. This
  covers `compose::add`, `up`, and a migrated container.
- **Other kind:** a `build.json` of the other kind than `scripts.build` makes is rebuilt the same
  way: a host build (no `image`) where `scripts.build` is now `iii sandbox build`, or an image
  where it is now a host command. Only the kind is compared, never the command's text (section
  5.5). Logs: `<container>: its build is a host build, and scripts.build now builds a VM image;
  rebuilding`, or `<container>: its build is a VM image, and scripts.build now runs on the host;
  rebuilding`.
- **Timeout:** `scripts.build_timeout` (section 4.1), then the container's `startup_timeout`
  starts for the run.
- **Logs:** `<container>: no build yet; running scripts.build`, then the build output.
- **Operations:** `compose::add` and `compose::up` report the build as a progress step of the
  container (`building`), and their operation finishes after the run started or failed.
- **Failure:** the container fails with `BUILD_FAILED` (section 10), carrying `{ code, message }`
  as section 4.1 defines for VM and host builds. Nothing is rolled back, as for any failed
  start.

### 5.3 Rebuilds

- `iii sandbox build <container>` or `iii call compose::build worker=<container>` rebuilds; the
  running container keeps its current image until restarted.
- `compose::add` or `compose::update` that changes a container's package (`source` in
  `build.json` no longer matches the lock) rebuilds it before restarting it, and logs
  `<container>: package changed (<old> -> <new>); rebuilding`.

Three names build, one calling the next:

| Name | What it is | Who calls it |
| --- | --- | --- |
| `compose::build` | The project function (section 7.4): runs `scripts.build` of one container, or of every one that declares it, as an operation of the daemon | The operator; `iii sandbox build` outside Compose |
| `iii sandbox build` | The CLI (section 8.1): inside Compose, the `scripts.build` of a VM container, which writes `build.json`; outside, a client of `compose::build` | Compose, as `scripts.build`; the operator |
| `<adapter>::worker::build` | The contract operation (section 6.2): builds the image in a build VM and writes `image.erofs` | `iii sandbox build` inside Compose only |

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
    "adapter": { "name": "sandbox-libkrun", "version": "0.5.0" },
    "contract": { "major": 1, "minor": 4 }
  },
  "built_at": "2026-11-10T14:03:22Z"
}
```

A `path://` container records `"source": { "kind": "path", "dir": "./link" }` (the declared
path, relative to the compose file). A host build writes the same keys without `image`.

- Every hash is written `sha256:<hex>`; times are RFC 3339 UTC.
- `dependency_files` holds the hash of each of these files that exists at the root of the
  worker directory: `package.json`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`,
  `bun.lockb`, `requirements.txt`, `pyproject.toml`, `uv.lock`, `poetry.lock`, `Cargo.toml`,
  `Cargo.lock`, `go.mod`, `go.sum`.
- `image.base_image.declared` is the container's `runtime.base_image`, else the manifest's,
  or `null` when the adapter chose the image; `reference` and `digest` are what the adapter reports, `digest`
  `null` when it does not know it.
- **One writer per case.** For a VM build, `iii sandbox build` writes `build.json` after
  `worker::build` returned, which happens only after the adapter renamed the new image into
  place. For a host build, Compose writes it after `scripts.build` exited 0.
- **Atomic writes:** a temporary file beside it, then rename; leftovers are swept. A `build.json` whose `build_id` does not
  match the image fails the run with `S508` (section 6.5); the fix is a rebuild.
- **Stable source.** Each writer reads the container's `source` (`build_record::source`) before the
  build and again before it writes `build.json`: `iii sandbox build` around `worker::build`, Compose
  around a host `scripts.build`. An `iii compose build` in another process may replace
  `.iii/workers/<container>/` meanwhile: it holds `.iii/workers/.lock` only while it copies, not for
  the length of a build. When the two reads differ, or the second finds no package (its directory is
  being replaced, section 7.5), the build records nothing (`build.json` is not written) and fails
  with `BUILD_INPUT_CHANGED`, exit 1 for `iii sandbox build`; building again records the new
  package. The adapter has already put a VM build's image in place: the next start rebuilds, as the
  old `build.json` names another source, or fails with `S508`.

### 5.5 A build that is out of date

Before `scripts.run` (on `up`, a restart, or a crash replacement), Compose compares the
container's current inputs with `build.json`; a container without `build.json` is not checked: `source`, `dependency_files`, and
`image.base_image.declared`. A difference in `dependency_files` or `base_image.declared` only
logs, once per start, one line per kind of change:

```text
link: dependencies changed since the last build (package.json); run `iii sandbox build link`
link: base image changed since the last build (docker.io/iiidev/node -> docker.io/iiidev/python); run `iii sandbox build link`
```

and the container runs on its current build. A change of `source` is a package update and
rebuilds (section 5.3). A change of code alone (outside the dependency files) is not a change:
a `path://` worker's code keeps syncing live into the guest (section 9.3).

## 6. Sandbox contract 1.3 and 1.4

All changes are additive. `CONTRACT_MAJOR` stays 1; `CONTRACT_MINOR` becomes 3, then 4 with
section 6.7. Unknown fields are ignored by both sides, as today; a new `WorkerSource` kind is
not a field, and a 1.3 peer refuses the `binary` tag (section 6.7).

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
    /// Absolute host directory of the build: `<project>/.iii/build/<container>`. Must be under the
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
  files in `output_dir` and flushed to disk, then published in order: the adapter removes the
  previous `image.id`, renames the new `image.erofs` into place and renames the new `image.id`
  last, flushing `output_dir` after the removal and between the two renames. Until the last
  rename `output_dir` holds no `image.id`, so a crash, a failed flush or a failed rename there
  leaves an image without an id, which `worker::start` refuses with `S508` until the next
  build; it never leaves the new image beside the previous build's id. A filesystem that
  cannot flush a directory gets a warning, not a failed build. A start that read `image.id`
  before the removal may still boot the newer image under the older id, a race the adapter
  accepts. The publication assumes one writer per `output_dir` at a time. One build per
  `state_key` runs at a time (`S509`), but nothing serializes two VMs whose builds share a
  directory: a container's build directory sits beside the compose file in every namespace,
  so building one container in two namespaces at once is unsupported.
- **The running VM is not touched.** A build may run while the VM of the same `state_key`
  runs; it uses its own build VM and swaps `image.erofs` by rename, so the running VM keeps
  the file it booted.
- **One build per `state_key` at a time:** a second one gets `S509`.
- **Another VM's build directory:** a VM's build directory is the last `output_dir`, or
  `worker::start` `build.build_dir` (section 6.3), that a scoped token of its `state_key`
  presented. The adapter records it per VM, outside every guest, in
  `~/.iii/sandbox/workers/<vm_id>/build-dir`, with an HMAC of the project token over the state
  key and the canonical directory, flushed to disk; the record survives an adapter restart,
  the VM's own `worker::destroy` deletes it, and a record another project token wrote counts
  for nothing. With a scoped token, a directory another VM of the same project token has
  recorded gets `S505`, before any file is created or written there and before `image.id` is
  read; the message names that VM's record and the way out (destroy that VM, or delete the
  record). The guarantee covers those directories only: the token Compose hands one container
  can neither replace nor boot an image in a directory another VM of the same project token
  has recorded. Two paths are one directory when they are equal or, both existing, share
  device and inode, so a second spelling (a bind mount, another case on a case-insensitive
  volume) does not get around it; the check never reads a directory's name. Presenting
  another directory moves the VM there and releases the previous one, so Compose may move
  build directories. Claims hold an flock on `~/.iii/sandbox/workers`: where the filesystem
  can lock a directory, two adapter processes never give one directory to two VMs; elsewhere
  (NFS) the adapter logs a warning and serializes claims within one adapter process only. A
  scoped token takes any directory no VM of the same project token has recorded at that
  moment: the directory of a destroyed VM that stayed on disk, the new path after a move until
  its container builds or starts, every build directory of another namespace (a container's
  build directory sits beside the compose file in every namespace, and each namespace has its
  own project token), and, on a filesystem that cannot flush a directory, a directory whose
  record a power failure lost. Compose builds or starts each container as it comes up, which
  records its directory. The project token is not checked and records nothing. Contract 1.3
  does not change: no new field, fixture or S-code.
- **Timeout and cancel:** the build lives as long as its caller reads `logs`. The adapter
  writes to the channel at least every 5 s (a one-byte binary frame, as `reader_gone` does today; readers take records from text frames
  only). When
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

With a scoped token, a `build_dir` another VM of the same project token has recorded gets
`S505` (section 6.2), checked before `image.id` is read.

### 6.4 `WorkerSource::Bundle.host_path`

Now the container's directory in `.iii/workers/` (section 7.5), no longer the global cache. The
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
  0.4.0). Compose's `ADAPTER_RANGE` moves to `^0.4`, and to `^0.5` with contract 1.4 (section
  6.7).

### 6.7 Contract 1.4: binary workers, and a destroy that keeps the data

`CONTRACT_MINOR` becomes 4. Two features, advertised in `AdapterInfo.features`:

```rust
/// `worker::build` of a `WorkerSource::Binary` with `WorkerBuildRequest::base_image`, and
/// `worker::start` booting it with `build`. Since contract 1.4.
pub const FEATURE_BINARY: &str = "worker.binary";
/// `worker::destroy` keeps the VM's data unless `WorkerDestroyRequest::prune` is set.
/// Since contract 1.4.
pub const FEATURE_PRUNE: &str = "worker.prune";

pub enum WorkerSource {
    // ... Bundle and Local unchanged ...
    /// A `binary` package's Linux build, run as the worker in the VM. Since contract 1.4.
    Binary {
        /// Manifest name, not the container key.
        package: String,
        /// The container's package directory, where Compose put the binary's Linux build for
        /// the host's architecture. `worker::build` copies it into the image.
        host_path: String,
        /// Path of the executable, relative to `host_path`.
        executable: String,
    },
}

pub struct WorkerBuildRequest {
    // ... unchanged fields ...
    /// The image the build starts from, instead of the manifest's `runtime.base_image`.
    /// Since contract 1.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_image: Option<String>,
}

pub struct WorkerDestroyRequest {
    pub auth: String,
    pub state_key: String,
    /// Delete the VM's data too. Since contract 1.4.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prune: bool,
}

pub struct WorkerDestroyResponse {
    pub destroyed: bool,
    /// Absolute host paths of the VM's data that stayed. Since contract 1.4.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<String>,
}
```

- **Binary source:** `worker.binary` is advertised only together with `worker.build`. Only
  `worker::build` and a `worker::start` with `build` take a binary source; `worker::prepare`
  and a `worker::start` without `build` refuse it with `S501`. `executable` is relative, has
  normal components only, stays inside `host_path`, is not a symlink and names a regular
  executable file; the guest runs `/workspace/<executable>` with no arguments. `host_path` may
  hold an `iii.worker.yaml`; if it does, its name must equal `package`, it may not set
  `scripts.setup`, and its `scripts.install` and `env` apply. An `executable` or a manifest
  that breaks these rules gets `S501`.
- **`base_image`:** `None` keeps the manifest's. A binary package names no base image, so a
  binary build without one gets `S001`; an implausible reference gets `S001` with any source.
- **Destroy without `prune`:** what the guest wrote stays; the VM's runtime state, its logs
  and its binding to a build directory go, so that directory is free again (sections 6.2,
  7.7). A later `worker::start` with `build` of the same `state_key` boots on the kept data
  disk. With `prune`, everything goes, as in 1.3.

  In `sandbox-libkrun`, which advertises `worker.prune` only in its overlay mode, the VM's
  directory `~/.iii/sandbox/workers/<vm_id>/` keeps the data disk (`upper.ext4`) and the
  pre-build copies (`upper.ext4.pre-build[.<n>]`, `root.pre-build[.<n>]`, section 7.3), keeps or
  writes the `build-mode` seal, and loses `state.json`, the state-key file, `console.log*`,
  `root/`, scratch and the `build-dir` record; a VM with no data goes whole, seal included.
  Every destroy without `prune` first moves the disk of a start without `build`
  (`root/upper.ext4`, a 0.26 disk among them) or a legacy clone (`root/`) to the next free
  pre-build name, so it never deletes what a guest wrote. A later `worker::start` with `build`
  boots on the kept data disk; one without `build` begins with a new disk, and what the
  previous one held stays under the pre-build names. Its legacy mode does not advertise
  `worker.prune` but keeps the data the same way; a caller that wants it gone sends
  `prune: true`.
- **`WorkerDestroyResponse`:** `destroyed` is `true` when the destroy removed runtime state,
  or, with `prune`, anything, and `false` when there was nothing to remove. `kept` lists the
  absolute paths of the data that stayed, so a VM that held only data answers a destroy
  without `prune` with `destroyed: false` and `kept` naming that data.
- **Wire:** `prune` is sent only when `true`, like `WorkerLogsRequest.from_end`. A 1.3 request
  has neither field and reads as `base_image: None`, `prune: false`, and a 1.3 reply has no
  `kept`. `wire_examples.json` pins those shapes under `WorkerBuildRequest@1.3`,
  `WorkerDestroyRequest@1.3` and `WorkerDestroyResponse@1.3`, which round-trip as their types,
  so a peer that drops a `serde(default)` fails its fixture test.
- **Older adapters:** a 1.3 adapter ignores `base_image` and `prune`, so every destroy deletes
  everything, and it refuses the `binary` tag. Compose reads the features from `adapter::info`
  and refuses an adapter without them only where the difference matters: a destroy without
  `prune` (`worker.prune`), so a destroy never silently deletes data, and a binary build or run
  (`worker.binary`). A destroy with `prune` does the right thing on a 1.3 adapter too.
- **Versions:** `iii-sandbox-contract` gets the fields, the features, the wire examples and
  `CONTRACT_MINOR = 4`; `sandbox-libkrun` mirrors them in `src/wire.rs` and `tests/fixtures`
  and advertises both features from 0.5.0 (0.4.1 and 0.4.2 shipped without them). Compose's
  `ADAPTER_RANGE` moves to `^0.5`.

## 7. Compose

### 7.1 Writing the explicit mode

`compose::add`, and the migration (section 7.3), write for a VM container:

- `start_after` gains the adapter's container key, persisted in the file;
- `scripts.build: iii sandbox build`;
- `scripts.run: iii sandbox run`, or, when the container already had a `scripts.run` that 0.26
  ran inside the VM (`run_override`), `scripts.run: iii sandbox run --command '<that command>'`.
  The command is single-quoted, each `'` written as `'\''`, with a test.

Other keys the user set are kept. The in-memory edge (`wire`) is gone.

The same holds for a container that sets `runtime.base_image` and has no `scripts.run`, a binary
package or a `path://`: it asks for a VM, and gets these keys written.

`start_after` names the adapter's container key, whatever it is, appended after the keys
already there, which keep their order (and, in a block list, their comments).
`iii call compose::add worker=onboarding` in a project whose adapter container is
`sandbox-libkrun` and where `onboarding` already starts after `state` writes:

```yaml
  # added by compose::add
  onboarding:
    worker: package://api.workers.iii.dev/onboarding
    version: "0.2.0"
    start_after: [state, sandbox-libkrun]   # was [state]
    scripts:
      build: iii sandbox build
      run: iii sandbox run
```

With `engine.sandbox.adapter: my-sandbox` and the adapter in a container `vm-adapter`, the
entry is `vm-adapter`: a container is named by its key, never by the package it runs.

### 7.2 The adapter before the migration

Every 0.26 project pins `sandbox-libkrun` to a 0.3.x release, which lacks `worker.build`, and
0.4.x lacks the `worker.binary` and `worker.prune` of contract 1.4. `ADAPTER_RANGE` is `^0.5`.
In a project with a VM container (one whose `scripts.run` is `iii sandbox run`, or one the
migration of section 7.3 will rewrite), at the points where section 7.3 runs and before
migrating:

- a generated adapter container (`# added by compose::add`) outside `ADAPTER_RANGE` is updated
  to the newest release in range, with the lock;
- a project with no adapter container gets the generated one, at the newest release in range,
  as `compose::add` adds it today, so the `start_after` that section 7.1 writes names a declared
  container;
- an adapter container the operator declared outside the range fails with
  `SANDBOX_ADAPTER_TOO_OLD`, naming the container, its range and the command
  `iii call compose::update worker=<adapter>@latest` (or editing `version`);
- under `--frozen`, an adapter outside the range fails with `SANDBOX_ADAPTER_TOO_OLD` too.

A project without a VM container (ephemeral sandboxes only, `Need::Explicit`) keeps its
adapter version: 0.3.x serves those.

`iii sandbox run` and `iii sandbox build` require `worker.build` from the adapter
(`REQUIRED_FEATURES` in `sandbox_cli.rs`) and fail with `SANDBOX_BUILD_UNSUPPORTED` otherwise.
The features of contract 1.4 are checked where they matter, so an adapter the operator
declared outside the range still serves the rest:

- `iii sandbox build` and `iii sandbox run` of a binary package require `worker.binary`
  (`SANDBOX_BINARY_UNSUPPORTED`);
- a destroy that keeps the data (`iii sandbox destroy` and `compose::remove` without prune)
  requires `worker.prune` (`SANDBOX_PRUNE_UNSUPPORTED`), since an older adapter ignores
  `prune: false` and deletes everything. `iii sandbox destroy` exits 125 and deletes nothing;
  `compose::remove` removes the container and leaves the VM's state where it is, with a line
  that says so. A destroy with prune needs neither: every adapter deletes everything.

### 7.3 Migration on run

There is no implicit VM path. Instead, Compose rewrites the containers that 0.26 ran in a VM
and that the file does not place anywhere else. The check is per container and idempotent,
keyed on `scripts` alone; the lock format does not change for anything 0.26 wrote (the `vm`
marker of section 7.5 is only for a binary in a VM, which 0.26 never ran).

- **Which containers:**
  - a `bundle` package without `scripts.run` (0.26 refused `scripts.run` on packages);
  - a `path://` whose manifest sets `runtime.base_image` and that has no `scripts.build` (0.26
    refused the key), whatever its `scripts.run`: none, a command (`run_override`), or the
    0.26 opt-in `iii sandbox run`;
  - any container whose `scripts.run` is `iii sandbox run` and that lacks `scripts.build` or
    the adapter in `start_after`: only those pieces are added;
  - a container with `runtime.base_image` and no `scripts.run` (section 7.1). 0.26 had no such
    key, so only a file written for 0.27 has one; the rewrite is the same.
- **Moving to the host:** a `path://` whose manifest sets `runtime.base_image` stays on the
  host when it declares its own `scripts.build` (its build command, or `""` when there is
  nothing to build); a bundle, when its `scripts.run` is a host command. Both are edits of
  `scripts`, and later runs leave them alone, with or without a lock. This holds for the
  manifest's key only: a container's own `runtime.base_image` with a host `scripts.build` or
  `scripts.run` is refused with `BASE_IMAGE_WITHOUT_VM` (section 4.2).
- **Where it runs:** `compose::up` (and `iii compose --up`), `compose::add`,
  `compose::update`, `compose::restart`, `compose::build` and `iii compose build`. Read-only
  paths (`compose::validate`, `compose::ps`, `compose::status`, `iii compose logs`),
  `compose::down` and `compose::remove` never edit the file; `compose::validate` reports a due
  migration as a warning naming the containers.

For each rewritten container:

1. Compose rewrites it as in section 7.1 (`edit.rs`, comments and the rest of the file kept),
   after the adapter step of section 7.2, and logs
   `<container>: migrated to the explicit VM mode (scripts.build/scripts.run: iii sandbox ...)`.
2. When the container starts: the adapter is ready (its `start_after`); a VM that a 0.26
   daemon left running (a `vm` record in `state.json`) is stopped with `worker::stop` and the
   record dropped; then the first build (section 5.2) and the run.
3. The data the 0.26 VM wrote is not deleted. The adapter tells the disks apart on the host:
   the first `worker::start` with `build` writes a seal file, `<vm_id>/build-mode`, beside
   `root/` and out of the guest's reach. A start with `build` that finds an `upper.ext4`
   without the seal renames it to `upper.ext4.pre-build`, creates a new upper, writes the seal and logs the old disk's path.
   The 0.26 disk mixes installed dependencies with data, and would shadow the build's
   dependencies if kept as the upper. In a legacy-mode directory the cloned rootfs
   is `root/` itself, which the overlay boot reuses as its trampoline: it is renamed to
   `root.pre-build/`, a fresh `root/` is created, and the path is logged the same way.
   A destroy keeps all of it (the copies are `upper.ext4.pre-build[.<n>]` and
   `root.pre-build[.<n>]`, section 6.7); `iii sandbox destroy --prune` and
   `compose::remove prune=true` remove it.

Compose validates the result before writing. A rewrite that would break a rule of section 4.2
(a bundle whose `scripts.build` is not `iii sandbox build`, or `iii sandbox run` on a container
that cannot run in a VM) is refused with that rule's code, and the file is left untouched.

If the stop or the build fails, the container fails with that error and the file stays
migrated; the next `up` retries from step 2.

`--frozen` never edits the file. A file with a container to rewrite fails under `--frozen` with
`VM_SCRIPTS_MISSING`, naming the containers and asking for one `iii compose --up` without
`--frozen`. This check comes before the adapter checks of section 7.2: a file with a container
to rewrite and no adapter fails with `VM_SCRIPTS_MISSING`, and the `up` it asks for adds the
adapter too.

### 7.4 `compose::build`

A new project function, a mutation like `compose::restart`:

```text
iii call compose::build [file=<path>] [worker=<container>]
```

- Request: `file` (as every project function), `worker` (a container key; omitted, every
  container that declares `scripts.build`), `operation_id` (optional, as for other
  mutations). A container whose `scripts.build` is `""` is reported as `skipped`, named or
  not.
- It is asynchronous: it answers `{ operation_id, status: "accepted", requested }`, holds the project's
  mutation lock while it runs, and reports progress per container (`building`, `built`,
  `failed`) through `compose::on-operation-progress`.
- It starts the adapter first when the project is down, as `compose::remove` does.
- The finished operation carries
  `results: [{ worker, status: "built" | "failed" | "skipped", build_id?, error?: { code, message } }]`.
- It never restarts a container.
- `compose::cancel` of that operation and `compose::down` stop it: Compose stops the
  `iii sandbox build` processes, which closes their channels and aborts the builds (section
  6.2). `down` cancels a running `compose::build` first, then takes the lock.

### 7.5 Package directories

Installing or updating a package copies the extracted package from the global cache into
`<project>/.iii/workers/<container>/` and updates `.iii/workers/.packages.json`. `iii compose build`
(offline) does the same from the lock. `WorkerSource::Bundle.host_path` and `III_PACKAGE_DIR` point
at that directory. A bundle that runs on the host (its `scripts.run` is a host command) runs
`scripts.build` and `scripts.run` in `.iii/workers/<container>/` and gets `III_PACKAGE_DIR` and
`III_PACKAGE_NAME` too; binary packages keep today's working directory. When Compose copies a
container's package directory again (it was missing, or the record was unreadable), it removes that
container's host-build `build.json`, so the next start runs the first build again (section 5.2). The
compose file's directory is already a host root of the adapter.

A directory is replaced through a copy made beside it and a rename. Its `.packages.json` entry is
removed, and the file written, before the directory changes, and comes back once the copy is in
place: a run stopped in between leaves no entry, so the next run copies it again instead of
taking it for the package it replaced. An entry whose directory is gone leaves the file before
any copy starts.

The replaced tree stays aside, whole, as `.<container>.replaced-<id>`, until the next
materialization prunes it, whether its process still runs or not. This covers part of a host
bundle that runs through the swap: what it reads relative to its working directory stays the
old tree, which the rename moved. What it resolves through an absolute path (`III_PACKAGE_DIR`,
Node's `require`, Python's `sys.path`) reads the new tree, so a lazy import can mix the two
versions until the container restarts.

A binary package that runs in a VM (section 4.2) needs its Linux build, not the host's:

- **Lock.** Its `worker-compose.lock` entry carries `vm: true`. `resolved.artifacts` already
  holds every build the registry published; the marker says the container installs the Linux
  build of the host's architecture, `<arch>-unknown-linux-musl`, else `<arch>-unknown-linux-gnu`
  (`registry::vm_targets`), chosen on each host as a host build is, so one lock serves every
  machine. The registry is asked with those targets, so a package without a Linux build fails
  with `UNSUPPORTED_PLATFORM` naming them. A host binary and every bundle keep the entry they
  had, without the key, and a lock without it loads as before. An entry whose `vm` no longer
  matches the declaration (the binary moved between the host and a VM) is resolved again, is
  not attached meanwhile, and fails `--frozen` with `COMPOSE_LOCK_OUT_OF_DATE`.
- **Package directory.** `.iii/workers/<container>/` holds the Linux build, and
  `.packages.json` its digest, so a move between the host and a VM copies the directory again.
- **Source.** `iii sandbox build` and `iii sandbox run` send `WorkerSource::Binary` with that
  directory as `host_path` and its executable, the first regular file with an execute bit, root
  first, in name order, at most three directories down, never a symlink
  (`vm_binary::executable`). `iii sandbox build` sends the container's `runtime.base_image` as
  `WorkerBuildRequest.base_image`, which a binary source requires (section 6.7), and says
  which executable the guest runs in the build output:
  `<package>: the guest runs <executable> from <host_path>`.

`iii compose build` runs outside the daemon's mutation lock. When the compose file or
`worker-compose.lock` changed while it prepared (the daemon committed an update, say), it writes
nothing and fails with `BUILD_PLAN_STALE`, naming the one that changed; it checks again under
`.iii/workers/.lock` before it copies. The check and the write are not atomic across processes: a
lock the daemon commits between the two is still overwritten, and the later check, which compares
with the lock this build wrote, then passes.

### 7.6 What stays and what goes

| Goes | Stays |
| --- | --- |
| `Child::Vm`, `RemoteVm`, `start_vm`, `start_vm_until`, `prepare_vm`, `prepare_own_vm`, `wire` | `own_vm_running`, `stop_own_vm`, `worker_status`, `worker_stop`: on restart, `down` and readiness they stop the VM of a dead `iii sandbox run` or let the supervised retry reattach, and avoid a false `CONTAINER_NAME_TAKEN` |
| `StartSpec::Vm` / `VmSpec` (`manifest.rs`) | `worker::destroy` from `compose::remove` and `iii sandbox destroy` |
| `check_info` (its only caller is `prepare_vm`); the CLI keeps its own `check_adapter` | `adapter_gone` and the adapter-restart cascade of #2469, now over processes: restarting the adapter restarts the containers whose `scripts.run` is `iii sandbox run`, without spending their `restart` attempts |
| | `vm_reconciliation`, only to stop a VM a 0.26 daemon left (section 7.3, step 2) |

A 0.26 `state.json` with a `vm` record is read once, its VM stopped, the record dropped.

### 7.7 A VM's `state_key` and its build directory

The adapter binds a build directory to the VM, that is the `state_key`, whose scoped token
recorded it (section 6.2), and a record counts only under the project token that wrote it. A
new state directory brings a new `state.json` and a new project token, so the old VMs'
records stop counting and no build gets `S505`. A VM's `state_key` can change while its token
survives: the same `state.json` reached through another spelling of `III_COMPOSE_STATE_DIR`,
or state moved or copied together with its token to another `III_COMPOSE_STATE_DIR`, while
the compose file, and so the container's build directory, stays where it is. The old VM's
record then still holds the container's build directory, and the new VM's build and start get
`S505` there; the message names the way out: destroy the old VM, or delete its record,
`~/.iii/sandbox/workers/<vm_id>/build-dir`. Moving a container's build directory needs
nothing: its next build or start records the new one and releases the old. Moving or copying
the whole project changes the build directory too, and nothing gets `S505`.

## 8. CLI

### 8.1 `iii sandbox build`

```text
iii sandbox build [<container>] [--file <path>] [--namespace <ns>] [--engine <url>]
```

- **Inside Compose** (the env of section 8.3 is set): builds its own container. It calls
  `adapter::info` (requires `worker.build`), then `worker::build` with the build output
  streamed to stdout, then writes `build.json` (section 5.4) from `build_record::source` and
  `build_record::declared_base_image` (section 11.2). A `source` that changed during the build
  fails it with `BUILD_INPUT_CHANGED` and records nothing (section 5.4).
- **Outside Compose:** calls `compose::build worker=<container>` on the project's daemon,
  found as `iii compose logs` finds it (`--file`, `--namespace`, `--engine`, `III_URL`, through
  `resolve_project_target`, section 11.2), prints
  the operation's progress and the container's build output, and waits for the result. No
  engine or no loaded project: the errors `iii compose logs` gives today (`ENGINE_CALL_FAILED`,
  `UNKNOWN_PROJECT`).
- **Exit codes:** 0 built; 1 the build failed (its S-code, `TIMED_OUT` or
  `BUILD_INPUT_CHANGED`); 125 `iii sandbox` itself failed (unreachable
  engine, adapter without the feature, bad env); 130 cancelled (`OPERATION_CANCELLED`). Outside Compose, Ctrl-C sends
  `compose::cancel` for its operation and waits for it before exiting.
- **Last line on failure**, parseable by Compose for `BUILD_FAILED`:
  `iii sandbox: build failed: <S-code or error code>: <message>`.

### 8.2 `iii sandbox run` and `destroy`

- `run` reads `build.json` from `III_BUILD_DIR`, calls `worker::start` with `build`, and skips
  `worker::prepare`. No `build.json`: `BUILD_MISSING`, exit 125.
- `destroy` keeps the VM's data: it deletes the runtime state and keeps the data disk and the
  copies of section 7.3 (section 6.7), then prints where the data stayed, from the reply's
  `kept` (`removed the VM state of <key>; kept its data in <paths>`), or that there was none.
  `destroy --prune` deletes everything, as in 0.26 (`removed the VM state and data of <key>`).
  `compose::remove` does the same, with `prune=true`
  (`iii call compose::remove worker=<c> prune=true`) for the second, and logs the same lines.
  Without prune both need `worker.prune` (section 7.2).
- **Kept data is reused.** A VM's `state_key` comes from the state directory and the container
  key, so a container added again under the same key, with any package, boots on the data disk
  a destroy without prune kept. Files the old worker wrote where the new build also writes
  hide the image's (section 9.1). To discard the data: `compose::remove prune=true` in the
  first place; or add the container again and remove it with `prune=true`; or delete the
  directory the destroy printed, `~/.iii/sandbox/workers/<vm_id>/`, while no VM of that key
  runs. The line `compose::remove` logs for the kept data says so.
- `build` and `run` of a binary package send `WorkerSource::Binary` and, for `build`,
  `base_image` (section 7.5), and need `worker.binary` (section 7.2).
- `iii sandbox` is visible in `iii --help` again (it was hidden for 0.26 by #2506), and the CLI
  reference is regenerated.

### 8.3 Env

Compose hands `iii sandbox build` and `iii sandbox run` the variables of section 2, plus:

- `III_BUILD_DIR` = `<project>/.iii/build/<container>`;
- `III_BUILD_TIMEOUT_MS` = `scripts.build_timeout`, for `iii sandbox build`;
- `III_BUILD_COMPOSE_FILE` = the same canonical path as `III_COMPOSE_FILE`, for
  `iii sandbox build` (`build_record::source` reads the declared `path://` there);
- `III_PACKAGE_DIR` = `<project>/.iii/workers/<container>` and `III_PACKAGE_NAME`, set for a
  bundle for the first time, and for every package container with scripts, a binary whose host
  `scripts.run` runs the build copied there included;
- `III_PACKAGE_KIND` = the package's kind, `bundle` or `binary`, with them: a binary in a VM
  boots as `WorkerSource::Binary`.

Constants in `sandbox_cli.rs`: `BUILD_DIR_ENV`, `BUILD_TIMEOUT_ENV`,
`BUILD_COMPOSE_FILE_ENV` and `PACKAGE_KIND_ENV` (new), `PACKAGE_DIR_ENV` and
`PACKAGE_NAME_ENV` (existing). `III_BUILD_DIR`, `III_BUILD_TIMEOUT_MS`,
`III_BUILD_COMPOSE_FILE` and `III_PACKAGE_KIND` join `NEVER_FORWARDED`, where the package pair
already is. All six join `spawn::RESERVED_ENV`, and the filter in `layer_own_vm_env` keeps these
six out of `III_WORKER_ENV_KEYS`; the reserved names it includes
today stay. The compose file gets its own name to keep two contracts apart: `III_COMPOSE_FILE`
belongs to every child and reaches the guest as one of those reserved names, while
`III_BUILD_COMPOSE_FILE` is an input of `iii sandbox build`, like the other build variables. The
guest still sees the path through `III_COMPOSE_FILE`; the split is not meant to hide it.

## 9. The guest

### 9.1 Data the worker writes

The VM's data disk is persistent and independent of the build. It keeps whatever the worker
writes (its home, databases, caches, and files it writes in `/workspace`, section 9.3) across
restarts and rebuilds, and a destroy without prune; only `iii sandbox destroy --prune` and
`compose::remove prune=true` reset it. It stays adapter-owned under
`~/.iii/sandbox/workers/<vm_id>/`.

The guest's root is the build image, read-only, with the data disk on top (overlay). A file
the worker modified inside a directory the build also writes (for example under
`node_modules`) keeps the worker's copy after a rebuild; `iii sandbox destroy --prune` clears
it.

### 9.2 Swapping the lower under a kept upper

overlayfs accepts a changed lower under an existing upper only without `index`, `metacopy`,
`redirect_dir` and `xino`. iii-init mounts the root with those off, and KAN-283 tests a rebuild
under an upper that already holds files.

### 9.3 Code and dependencies

- A bundle's code is in the image, and so is a binary's Linux build, run as
  `/workspace/<executable>` with no arguments; its `.iii/workers/<container>/` is the build's
  input only.
- A `path://` worker's code keeps syncing live into the guest. `.iii/` (and with it
  `.iii/build/` and `.iii/workers/`) and the dependency directories (`node_modules`, `.venv`,
  `target`, and the rest of the adapter's ignore list) are excluded from the copy and the sync,
  including for `path://.`. One list drives the run's copy, the live sync and the names the
  boot cleanup keeps: the adapter's ignore list plus `.iii`. The build's input copy uses the
  same list except `vendor`, which goes into the build (Go vendors source there; an install in
  the build rewrites it otherwise). Names that often hold source in nested directories
  (`build`, `out`, `obj`, `coverage`, `deps`) match only a directory at the worker's root;
  every other name, `target` and `dist` included, matches at any depth.
- The boot never deletes what the build installed or what the worker wrote in `/workspace`.
  Each copy records what it extracted in a manifest on the data disk, outside `/workspace`;
  the next boot removes only manifest entries the host no longer has. A disk without a
  manifest (written by an older adapter) skips that cleanup once. The cleanup never removes
  through a symlink, never touches names the following copy leaves out, and skips names with a
  newline or a backslash. Adding a dependency: edit `package.json`, run
  `iii sandbox build <container>`, restart the container.

### 9.4 Env

Unchanged from #2505: the guest gets `guest_env` (the container's `environment`/`env_file` and
the reserved `III_*` except the five of section 8.3), never the host's environment. The build VM
gets the same set.

## 10. Errors and logs

Compose and CLI codes:

| Code | When |
| --- | --- |
| `VM_SCRIPTS_MISSING` | `--frozen` on a file with containers to rewrite (section 7.3) |
| `SANDBOX_ADAPTER_TOO_OLD` | The adapter is outside `ADAPTER_RANGE` and Compose may not update it |
| `SANDBOX_BUILD_UNSUPPORTED` | The adapter does not advertise `worker.build` |
| `SANDBOX_RUN_NOT_A_VM_WORKER` | `iii sandbox run` on a container that is not a bundle, a binary, or a `path://` with `runtime.base_image` |
| `SANDBOX_BINARY_WITHOUT_BASE_IMAGE` | `iii sandbox run` on a binary package whose container sets no `runtime.base_image` |
| `SANDBOX_BINARY_UNSUPPORTED` | `iii sandbox build` or `run` of a binary package with an adapter that does not advertise `worker.binary` |
| `SANDBOX_PRUNE_UNSUPPORTED` | A destroy that keeps the data with an adapter that does not advertise `worker.prune` (section 7.2) |
| `BASE_IMAGE_WITHOUT_VM` | A container's `runtime.base_image` with a `scripts.run` other than `iii sandbox run`, or a `scripts.build` other than `iii sandbox build` |
| `INVALID_BASE_IMAGE` | `runtime.base_image` that is not an image reference |
| `SANDBOX_BUILD_WITHOUT_RUN` | `iii sandbox build` without `iii sandbox run` |
| `SANDBOX_RUN_WITHOUT_BUILD` | `iii sandbox run` with another `scripts.build` command |
| `SANDBOX_UNSUPPORTED_PLATFORM` | `iii sandbox run`, or `iii sandbox build` inside Compose, on Windows |
| `SANDBOX_ENV_INVALID` | `iii sandbox` inside Compose with an incomplete or invalid set of the variables of section 8.3 |
| `SANDBOX_INVALID_ARGUMENTS` | `iii sandbox` arguments that do not fit the mode (for example `--file` inside Compose) |
| `SANDBOX_CONTRACT_UNSUPPORTED` | The adapter speaks another contract major |
| `SANDBOX_FAILED` | Any other failure of `iii sandbox` itself (exit 125) |
| `BUILD_MISSING` | `iii sandbox run` without `build.json` |
| `BUILD_INPUT_CHANGED` | A build (`iii sandbox build` or a host `scripts.build`) whose source was replaced while it built (section 5.4); build again |
| `BUILD_PLAN_STALE` | Existing code, new trigger: `iii compose build` also fails when `worker-compose.lock` changed while it prepared (section 7.5) |
| `BUILD_FAILED` | A first build or a package rebuild failed; carries `{ code, message }` (section 4.1) |
| `RUN_NOT_ALLOWED_FOR_PACKAGE` | Gone: a binary package may set `scripts` (section 4.2) |

S-codes: `S507`, `S508`, `S509` (section 6.5). `S501` also answers a binary source the adapter
refuses: an `executable` or a manifest that breaks the rules of section 6.7, or a binary source
outside a build (`worker::prepare`, `worker::start` without `build`); a binary build without
`base_image`, or with an implausible one, is `S001`. Compose checks `base_image` first
(section 4.2), so it meets these only through a broken package. `S505` is reused, unchanged in
the fixtures:
besides a token that does not match, it answers a scoped token whose `output_dir` or
`build_dir` another VM of the same project token has recorded (section 6.2); the message
names that VM's record and how to free it.

Logs:

- `<c>: no build yet; running scripts.build`
- `<c>: package changed (<old> -> <new>); rebuilding`
- `<c>: its build is a host build, and scripts.build now builds a VM image; rebuilding`, and the
  reverse, `<c>: its build is a VM image, and scripts.build now runs on the host; rebuilding`
- ``<c>: dependencies changed since the last build (<files>); run `iii sandbox build <c>` ``
- ``<c>: base image changed since the last build (<old> -> <new>); run `iii sandbox build <c>` ``
- `<c>: migrated to the explicit VM mode (scripts.build/scripts.run: iii sandbox ...)`
- warning, when the rewrite wraps a command: ``<c>: scripts.run now runs inside the VM (iii sandbox run --command); to keep it on the host, set scripts.build ("" when there is nothing to build)``
- `compose::validate` warning: `migration due: <c>, <c>; the next up rewrites them`
- adapter: `<c>: kept the pre-build data disk at <path>`
- destroy (`iii sandbox destroy`, `compose::remove`): `removed the VM state of <key>; kept its
  data in <paths>`, `removed the VM state and data of <key>` with prune, `no VM state for <key>`
  when there was nothing to remove. `compose::remove` prefixes them with `<c>: ` and, when data
  stayed, ends with `; a container added again as <c> boots with it`; with an adapter without
  `worker.prune` it logs `<c>: kept VM <vm_id>: <adapter> <version> cannot keep its data (no
  worker.prune); its state stays in ~/.iii/sandbox/workers/<vm_id>, and a container added again
  as <c> boots with it` (section 7.2)

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
| KAN-281 Compose: `.iii/workers/`, `build.json` | agent | `crates/iii-compose/src/{registry,build}.rs`, new `crates/iii-compose/src/build_record.rs`, `crates/iii-compose/tests/project_cache.rs` |
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
pub const SCHEMA: u32 = 1; // the `schema` of build.json
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
pub fn source(compose_file: &Path, container: &str) -> Result<BuildSource>; // .packages.json beside it, or the file
pub fn declared_base_image(worker_dir: &Path) -> Result<Option<String>>; // runtime.base_image
pub fn dependency_files(worker_dir: &Path) -> Result<BTreeMap<String, String>>;
/// Every difference of section 5.5; empty when the build is up to date.
pub fn changes(record: &BuildRecord, compose_file: &Path, container: &str, worker_dir: &Path) -> Result<Vec<Change>>;
pub fn build_dir(compose_dir: &Path, container: &str) -> PathBuf; // <compose dir>/.iii/build/<container>
pub fn package_dir(compose_dir: &Path, container: &str) -> PathBuf; // <compose dir>/.iii/workers/<container>

// crates/iii-compose/src/registry.rs (KAN-281)
/// Makes `.iii/workers/` match the lock: copies missing or changed packages from the cache, removes
/// the directories of containers no longer locked, rewrites `.iii/workers/.packages.json`.
pub async fn materialize_project(compose_dir: &Path, cache_root: &Path, lock: &PreparedLock) -> Result<()>;

// crates/iii-compose/src/sandbox.rs (KAN-280)
/// Sections 7.2 and 7.3, run before `apply_adapter_change`: updates a generated adapter outside
/// `ADAPTER_RANGE` to the newest release in range (7.2), then rewrites the containers 0.26 ran
/// in a VM (7.3). Also adds the adapter container when a container it rewrites needs one and
/// none is declared, as `adapter_change` would; `apply_adapter_change` keeps re-resolving
/// (`refresh`) and removing (`allow_remove`) it. Edits `edited`, `file` and `prepared` in place
/// and logs each rewrite; writes nothing and acquires nothing: when `adapter` is `Some`, the
/// caller runs `PreparedLock::install`, then writes the file and the lock, as after
/// `apply_adapter_change`. `None` when nothing changed. Not called under `--frozen`: there
/// `require_declared_adapter` (KAN-280) checks for a due migration first and refuses it with
/// `VM_SCRIPTS_MISSING`, whose `up` without `--frozen` also adds a missing adapter; only then
/// does it refuse a missing adapter (`SANDBOX_ADAPTER_NOT_DECLARED`) or an out-of-range one
/// (`SANDBOX_ADAPTER_TOO_OLD`).
pub(crate) struct Migration { pub containers: Vec<String>, pub adapter: Option<AdapterChange> }
pub(crate) async fn migrate(path: &Path, edited: &mut String, file: &mut ComposeFile, project_namespace: &str, prepared: &mut PreparedLock) -> Result<Option<Migration>>;

// crates/iii-compose/src/lockfile.rs (KAN-280)
impl PreparedLock { pub fn packages(&self) -> impl Iterator<Item = (&str, &ResolvedPackage)>; }

// crates/iii-compose/src/lib.rs (KAN-280)
pub struct ProjectTarget { pub engine_url: String, pub daemon_namespace: String, pub file: Option<PathBuf> }
/// The discovery `iii compose logs` does today, shared with `iii sandbox build`.
pub fn resolve_project_target(file: Option<&Path>, namespace: Option<String>, engine: Option<String>) -> Result<ProjectTarget>;
```

`.iii/workers/.packages.json`:

```json
{ "schema": 1, "containers": { "onboarding": { "name": "onboarding", "version": "0.2.0", "sha256": "sha256:8c3406..." } } }
```

- `materialize_project` runs after every package install: from `lifecycle.rs` (KAN-280) and
  `build.rs` (KAN-281).
- `iii compose build` (KAN-281) calls `sandbox::migrate` before `apply_adapter_change` and
  before installing, and installs when either returns an adapter change. `migrate` adds the
  adapter a rewritten container needs when none is declared, so the file it leaves parses (no
  `start_after` names a missing container), and `apply_adapter_change` then re-resolves or
  removes the adapter `migrate` added; it still adds the one an `engine.sandbox` need asks for.
  Under `--frozen` it calls `require_declared_adapter` instead, as today.
- `source` and `changes` take the compose file, not its directory: the `dir` of a `path://` is
  declared in that file, which can have any name (`--file`, `file=`). Directories come from
  `compose_file.parent()`. Inside Compose, `iii sandbox build` finds the file in
  `III_BUILD_COMPOSE_FILE` (section 8.3).
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
| 4 | `.iii/workers/` and `.iii/build/` per project, in the `.iii/` beside the compose file whatever `III_COMPOSE_STATE_DIR` says, out of git; base images and extracted packages stay in global caches | 3.2, 7.5 |
| 5 | `.iii/workers/` and `.iii/build/` are keyed by container | 3.2 |
| 6 | The first build is automatic when a container starts without one; rebuilds are explicit, except a package update | 5.2, 5.3 |
| 7 | A `path://` worker's code syncs live; its dependencies come from the build | 9.3 |
| 8 | The data the worker writes is persistent and independent of the build | 9.1 |
| 9 | An out-of-date build only logs | 5.5 |
| 10 | No implicit VM path: the containers 0.26 ran in a VM are rewritten when the project next runs | 7.3 |
| 11 | The migration keeps the 0.26 data disk as `upper.ext4.pre-build` instead of deleting it | 7.3 |
| 12 | Compose updates a generated adapter that is too old before migrating, only in projects with a VM container | 7.2 |
| 13 | Contracts 1.3 and 1.4, additive, with `sandbox-libkrun` 0.5 as the minimum (0.4 implements 1.3 only) | 6.6, 6.7 |
| 14 | `scripts` on packages: allowed for bundles and binaries; a binary runs in a VM with the container's `runtime.base_image` (decision 19) | 4.2 |
| 15 | Prebuilt images are out of scope | 1 |
| 16 | The rewrite is per container and idempotent, keyed on `scripts`. The lock gains one key, `vm`, only on the entry of a binary package in a VM, so a file that uses none of the new keys (`scripts.build`, `scripts.run` on a package, `runtime.base_image`) keeps a lock 0.26 loads, and still loads in 0.26 | 7.3, 7.5 |
| 17 | A build lives as long as its caller reads its output; stopping the caller cancels it | 6.2, 7.4 |
| 18 | A migrated VM project no longer loads in 0.26 (`scripts.build` is unknown there); going back means restoring the 0.26 file from git, as the upgrade guide says | 7.3, 11.1 |
| 19 | Contract 1.4: a binary package runs in a VM from a build with an explicit `base_image`; `worker::destroy` keeps the data unless `prune`; Compose refuses an adapter without `worker.prune` for a destroy that keeps the data, and without `worker.binary` for a binary build or run | 6.7 |

## 14. Open questions

1. Whether a manifest may extend the dependency file list of section 5.4.
2. Sharing the base image as a second lower layer instead of a full rootfs per container
   (section 5.1), which needs iii-init to mount two lowers.
