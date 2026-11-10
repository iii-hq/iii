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
`worker::prepare` and `worker::start` directly. Nothing in `worker-compose.yaml` says that a
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
- Compose runs both like any other command. It no longer has a VM code path of its own.

The decisions behind each section are listed in section 12.

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
- Building a `Dockerfile` (workers with `deploy: image`, such as `hermes`). That is phase 2,
  section 11.
- A different execution model for host or Docker workers. They keep `scripts.run` as today.

## 2. What 0.26 does today

| Piece | 0.26 behavior | Where |
| --- | --- | --- |
| VM selection | A `bundle` package, or a `path://` with `runtime.base_image`, is a VM need | `crates/iii-compose/src/sandbox.rs` (`vm_needs`) |
| Adapter | Compose writes the `sandbox-libkrun` container (`# added by compose::add`) and installs it | `sandbox.rs` (`needs`, `adapter_key`) |
| Start order | Every VM container starts after the adapter; the edge is in memory only | `sandbox.rs` (`wire`) |
| Boot | Compose calls `worker::prepare`, then `worker::start` with the spawn plan's env | `lifecycle.rs` (`start_vm_until`), `Child::Vm`, `RemoteVm` |
| Dependencies | `scripts.install` runs in the guest on first boot and persists on the VM's ext4 upper | `sandbox-libkrun/src/cli/local_worker.rs` (prepared marker), `upper.rs` |
| VM state | `~/.iii/sandbox/workers/<vm_id>/`, `vm_id` derived from the state key | `sandbox-libkrun/src/worker_vm.rs` |
| State key | `sha256(<state dir> \0 <container key>)` | `sandbox.rs` (`state_key`) |
| Packages | Extracted in a global cache, `~/.iii/compose/packages` (or `$III_COMPOSE_STATE_DIR/packages`) | `state.rs` (`package_cache`) |
| Base images | Pulled from an OCI registry into `~/.iii/cache/<slug>/`, turned into EROFS | `sandbox-libkrun/src/cli/worker_manager/oci.rs`, `rootfs_cache.rs` |
| Explicit opt-in | `scripts.run: iii sandbox run` on a `path://` container that lists the adapter in `start_after`; hidden from `--help` since #2506 | `sandbox.rs` (`runs_own_vm`), `sandbox_cli.rs` |
| Packages and `scripts.run` | Refused: `RUN_NOT_ALLOWED_FOR_PACKAGE` | `config.rs` |
| Guest env | The container's `environment`/`env_file` plus the reserved `III_*`, on both paths since #2505 | `spawn.rs` (`guest_env`), `sandbox.rs` (`layer_own_vm_env`) |

`iii sandbox run` (#2457) already does what the explicit run needs: it boots the VM through
`worker::*` with a token scoped to that VM, prints the guest's output as its own, turns SIGTERM
and SIGINT into `worker::stop`, reattaches to a running instance and exits with the VM's code.

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

  # A local worker in a VM: the image comes from runtime.base_image in its iii.worker.yaml.
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

`0.4.0` stands for the first `sandbox-libkrun` release with `worker.build` (section 5.4).

### 3.2 The project tree

```text
my-project/
├── worker-compose.yaml
├── worker-compose.lock
├── .iii/                    # Compose state and the project token (hidden from VMs, as today)
├── .workers/                # packages, one directory per container; like node_modules
│   ├── onboarding/
│   └── sandbox-libkrun/
├── .build/                  # one directory per container that has a build
│   ├── onboarding/
│   │   ├── image/           # the image the run boots (VM containers)
│   │   └── build.json
│   ├── link/
│   └── api/
│       └── build.json       # host builds record build.json without an image
└── link/                    # a path:// worker, as today

~/.iii/cache/                # global: base images only (docker.io/iiidev/node, .../python, ...)
~/.iii/compose/packages/     # global: downloaded package archives only
~/.iii/sandbox/workers/      # adapter-owned: each VM's persistent data disk
```

- `.workers/` and `.build/` are generated and stay out of git, like `node_modules`. They are
  rebuilt from `worker-compose.lock`. Templates and `iii project init` add both to
  `.gitignore`.
- `.workers/<container>/` holds the extracted package of that container. Compose writes a
  `.iii-package.json` beside it (`name`, `version`, `sha256`) and replaces the directory when
  the lock points at a different artifact.
- The global package cache keeps only the downloaded archives, so a second project does not
  download them again.
- `.build/` and `.workers/` are never copied or synced into a guest, like `.iii` today.

### 3.3 The flow

1. `iii call compose::add worker=onboarding` resolves the package, writes the container with
   `start_after` and `scripts` (section 6.1), and reconciles. It runs nothing from the package
   before the build.
2. A container with `scripts.build` and no `.build/<container>/build.json` gets its **first
   build** before its first run, during `compose::add` or `up`. Compose logs
   `<container>: no build yet; running scripts.build`.
3. `scripts.run` starts the worker. For a VM container, `iii sandbox run` boots
   `.build/<container>/image`.
4. A restart, or an adapter restart, repeats `scripts.run` only.
5. A rebuild is explicit: `iii sandbox build <container>` (section 4.2), then a restart of that
   container.
6. `compose::remove` stops the container, has the adapter destroy the VM's data disk, and
   deletes `.build/<container>/` and `.workers/<container>/`.

## 4. Build

### 4.1 What a build produces

| Container | `scripts.build` | Result |
| --- | --- | --- |
| `bundle` or `path://` with `runtime.base_image` | `iii sandbox build` | `.build/<container>/image` + `build.json` |
| Host or Docker worker | any command | `.build/<container>/build.json` (Compose records the run) |
| `deploy: image` (phase 2) | `iii sandbox build` | `.build/<container>/image` from the worker's `Dockerfile` |

For a VM container, `iii sandbox build` calls `worker::build` on the adapter (section 5.2). With
`sandbox-libkrun` the build runs in a build VM: it boots the base image from the global cache,
copies the worker's code in, runs the manifest's `scripts.install` (and `scripts.setup` when
present), and freezes the result as the container's image. Nothing runs on the host.

Another adapter may build differently, for example with Docker. That is the adapter's choice;
Compose and the CLI only call `worker::build`.

### 4.2 Commands

| Command | What it does |
| --- | --- |
| `scripts.build: iii sandbox build` | Run by Compose. Builds the container it was started for, from the env Compose hands it (section 7.2). |
| `iii sandbox build <container>` | Run by the user in the project directory. Asks the running Compose daemon (`compose::build`) to build that container. |
| `iii call compose::build [container=<key>]` | New Compose function: runs `scripts.build` of one container, or of every container that has one. Answers with one result per container. |
| `iii compose build` | Unchanged role: downloads packages and fills `.workers/` from the lock. It starts no engine, so it runs no VM build. |

A build never restarts the container. The user restarts it with
`iii call compose::restart worker=<key>` when ready.

### 4.3 `build.json`

```json
{
  "schema": 1,
  "container": "onboarding",
  "source": { "kind": "package", "name": "onboarding", "version": "0.2.0", "sha256": "8c3406..." },
  "command": "iii sandbox build",
  "dependency_files": { "package.json": "sha256:2f1c...", "package-lock.json": "sha256:9a07..." },
  "image": {
    "build_id": "sha256:7d41...",
    "base_image": { "reference": "docker.io/iiidev/node:latest", "digest": "sha256:b1e2..." },
    "adapter": { "name": "sandbox-libkrun", "version": "0.4.0" },
    "contract": "1.3"
  },
  "built_at": "2026-11-10T14:03:22Z"
}
```

- `source.kind` is `package` (with `name`, `version`, `sha256` from the lock) or `path` (with
  the worker directory relative to the compose file).
- `dependency_files` holds the sha256 of each of these files that exists at the root of the
  worker directory: `package.json`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`,
  `bun.lockb`, `requirements.txt`, `pyproject.toml`, `uv.lock`, `poetry.lock`, `Cargo.toml`,
  `Cargo.lock`, `go.mod`, `go.sum`.
- `image` is present for VM builds only. `build_id` is the adapter's id for the image, passed
  back on `worker::start`.

### 4.4 A build that is out of date

Before `scripts.run` (on `up`, a restart, or a crash replacement), Compose compares the
container's current inputs with `build.json`: `source`, `dependency_files`, and for VM builds
the base image reference. Any difference only logs, once per start:

```text
link: dependencies changed since the last build (package.json); run `iii sandbox build link`
```

and the container runs on its current build. A change of code alone (outside the dependency
files) is not a change: a `path://` worker's code keeps syncing live into the guest (section
8.2).

The only blocking case is a failed first build: the container fails with the build's error and
is reported like any failed start.

## 5. Sandbox contract 1.3

All changes are additive. `CONTRACT_MAJOR` stays 1; `CONTRACT_MINOR` becomes 3. Unknown fields
are ignored by both sides, as today.

### 5.1 Feature

```rust
/// `worker::build`, and `worker::start` booting the image it built. Since contract 1.3.
pub const FEATURE_BUILD: &str = "worker.build";
```

Advertised in `AdapterInfo.features`. Compose and `iii sandbox` call `worker::build` only on an
adapter that advertises it.

### 5.2 `worker::build`

New operation `WORKER_BUILD = "worker::build"`, registered as `<adapter>::worker::build`.

```rust
/// Builds the image of one container's VM; long-running. Idempotent for identical inputs.
pub struct WorkerBuildRequest {
    /// The project token or the VM's scoped token.
    pub auth: String,
    /// The VM the build is for; the scoped token is checked against it.
    pub state_key: String,
    /// Container key, for logs and errors.
    pub worker_name: String,
    pub source: WorkerSource,
    /// Absolute host directory the image is written to: `<project>/.build/<container>/image`.
    /// Must be under the adapter's host roots; the adapter replaces its content atomically.
    pub output_dir: String,
    /// Env of the install step: the container's `environment`/`env_file`, nothing from the host.
    pub env: BTreeMap<String, String>,
    /// Writer end of a channel for the build's output, one `LogRecord` JSON per text frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logs: Option<StreamChannelRef>,
}

pub struct WorkerBuildResponse {
    /// Opaque id of the image, stable for identical inputs; passed back on `worker::start`.
    pub build_id: String,
    /// The base image the build started from, with its resolved digest.
    pub base_image: BaseImageRef,
    pub built_at_ms: u64,
}

pub struct BaseImageRef {
    pub reference: String,
    pub digest: String,
}
```

Errors reuse the S-codes: `S501` for an `output_dir` or source outside the host roots, `S200`
for a failed install (its message carries the tail of the install output), `S304` from the
router for an adapter without the operation.

### 5.3 `worker::start` boots a build

```rust
pub struct WorkerStartRequest {
    // ... unchanged fields ...
    /// Boot the image `worker::build` wrote, without running `scripts.install`. Since contract
    /// 1.3; an adapter without `worker.build` ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<WorkerBuildRef>,
}

pub struct WorkerBuildRef {
    pub build_id: String,
    /// The `output_dir` the build was written to.
    pub image_dir: String,
}
```

The adapter refuses a `build_id` that does not match the image in `image_dir` (`S502` with a
message naming the expected and found ids), so a VM never boots a half-written or foreign image.

### 5.4 Versions

- `iii-sandbox-contract` gets the types above and `CONTRACT_MINOR = 3`, published before the
  adapter release that uses them.
- `sandbox-libkrun` advertises `worker.build` from its next minor release (written here as
  0.4.0). Compose's `ADAPTER_RANGE` moves to that release.
- An adapter without `worker.build`: `iii sandbox build` and `compose::build` fail with
  `SANDBOX_BUILD_UNSUPPORTED`, naming the adapter, its version and the release that adds the
  feature. `compose::update worker=sandbox-libkrun@latest` fixes it.

## 6. Compose

### 6.1 Writing the explicit mode

`compose::add` (and the migration, section 6.2) writes, for a container that runs in a VM
(a `bundle` package, or a `path://` whose manifest sets `runtime.base_image`):

- `start_after` gains the adapter's container key, persisted in the file (no in-memory edge);
- `scripts.build: iii sandbox build`;
- `scripts.run: iii sandbox run`.

Keys the user already set are kept. The adapter container itself is still written and
installed as in 0.26 when the project needs it; a container "needs" the adapter when its
`scripts.run` starts with `iii sandbox run`.

`scripts.run` and `scripts.build` become legal on `package://` containers;
`RUN_NOT_ALLOWED_FOR_PACKAGE` is removed.

### 6.2 Migration on run

There is no implicit VM path. When Compose loads a file and finds a container that 0.26 would
have run in a VM and that has no `scripts.run`, it migrates that container, and only that one:

1. writes `start_after` and `scripts` as in section 6.1, keeping comments and the rest of the
   file (`crates/iii-compose/src/edit.rs`);
2. asks the adapter to destroy that container's old VM state once (`worker::destroy` for its
   state key): in 0.26 the installed dependencies live on that disk and would shadow the new
   build's;
3. runs the first build (section 3.3), then starts it.

Compose logs, per container:

```text
onboarding: migrated to the explicit VM mode (scripts.build/scripts.run: iii sandbox ...)
```

`--frozen` never edits the file. A file that needs the migration fails under `--frozen` with
`VM_SCRIPTS_MISSING`, naming the containers and asking for one `iii compose --up` without
`--frozen`.

### 6.3 What is removed

- `Child::Vm`, `RemoteVm`, `start_vm`, `start_vm_until`, `prepare_vm` and the in-memory
  adapter edge (`wire`). VM containers are processes (`iii sandbox run`).
- The adapter-restart cascade of #2469 keeps its behavior through the processes: restarting the
  adapter restarts the containers whose `scripts.run` is `iii sandbox run`, without spending
  their `restart` attempts.

### 6.4 Package directories

Installing or updating a package extracts it into `<project>/.workers/<container>/` from the
archive in the global cache, and writes `.iii-package.json`. `WorkerSource::Bundle.host_path`
and `III_PACKAGE_DIR` point at that directory. The adapter's host roots already include the
compose file's directory, so no root changes.

## 7. CLI

### 7.1 `iii sandbox`

`iii sandbox` is visible in `iii --help` again (it was hidden for 0.26 by #2506) with three
subcommands:

| Command | Behavior |
| --- | --- |
| `iii sandbox build [<container>]` | Inside Compose (env of section 7.2 present): calls `worker::build` for its container, writes `build.json`, streams the build output, exits 0 on success. Outside Compose: calls `compose::build container=<container>` on the project's daemon. |
| `iii sandbox run` | As today, plus `worker::start.build` from `build.json`. Fails with `BUILD_MISSING` if the container has no build. |
| `iii sandbox destroy` | As today: removes the VM's data disk. |

### 7.2 Context

The env Compose hands `iii sandbox build` is the one `iii sandbox run` already gets
(`III_SANDBOX_TOKEN`, `III_SANDBOX_STATE_KEY`, `III_SANDBOX_ADAPTER`, `III_SANDBOX_NAMESPACE`,
`III_WORKER_DIR` or `III_PACKAGE_DIR`/`III_PACKAGE_NAME`, `III_WORKER_ENV_KEYS`,
`III_STOP_TIMEOUT_MS`), plus `III_BUILD_DIR` = `<project>/.build/<container>`.

## 8. The guest

### 8.1 Data the worker writes

The VM's data disk is persistent and independent of the build. It keeps whatever the worker
writes (its home, databases, caches) across restarts and rebuilds. Only
`iii sandbox destroy` and `compose::remove` reset it. It stays where it is today, under
`~/.iii/sandbox/workers/<vm_id>/`, owned by the adapter.

The guest's root is the build image, read-only, with the data disk on top. A file the worker
modified inside a directory the build also writes (for example under `node_modules`) keeps the
worker's copy after a rebuild. `iii sandbox destroy` clears it.

### 8.2 Code and dependencies of a `path://` worker

The worker's code keeps syncing live into the guest, as today. Dependency directories
(`node_modules`, `.venv`, `target`, and the rest of the adapter's ignore list) are not synced:
they come from the build. Adding a dependency is: edit `package.json`, run
`iii sandbox build <container>`, restart the container.

### 8.3 Env

Unchanged from #2505: the guest gets the container's `environment`/`env_file` and the reserved
`III_*` variables, never the host's environment. The build's install step gets the same set.

## 9. Errors and logs

| Code / log | When |
| --- | --- |
| `VM_SCRIPTS_MISSING` | `--frozen` on a file whose VM containers need the migration |
| `SANDBOX_BUILD_UNSUPPORTED` | The adapter does not advertise `worker.build` |
| `BUILD_MISSING` | `iii sandbox run` without a build (should not happen through Compose, which builds first) |
| `BUILD_FAILED` | The first build failed; carries the adapter's S-code and message |
| log `<c>: no build yet; running scripts.build` | First build |
| log `<c>: dependencies changed since the last build (<files>); run \`iii sandbox build <c>\`` | Out-of-date build (section 4.4) |
| log `<c>: migrated to the explicit VM mode (...)` | Migration (section 6.2) |

Removed: `RUN_NOT_ALLOWED_FOR_PACKAGE`.

## 10. Delivery

### 10.1 Tickets and ownership

Every file has one owner. Parent ticket: KAN-277.

| Ticket | Owner | Files |
| --- | --- | --- |
| KAN-278 spec | orchestrator | `tech-specs/2026-11-10-explicit-vm-mode/` |
| KAN-279 contract | orchestrator | `crates/iii-sandbox-contract/**` |
| KAN-280 Compose: explicit mode, migration, removal | agent | `crates/iii-compose/src/{config,edit,daemon,lifecycle,project}.rs`, `sandbox.rs` (needs, wiring, removal of the VM path) |
| KAN-281 Compose: `.workers/`, `.build/`, `build.json` | agent | `crates/iii-compose/src/{registry,lockfile,state}.rs`, new `crates/iii-compose/src/build_record.rs` |
| KAN-282 CLI | agent | `crates/iii-compose/src/sandbox_cli.rs`, `engine/src/main.rs`, `docs/next/cli-reference/` |
| KAN-283 `sandbox-libkrun` | agent | `iii-hq/workers`: `sandbox-libkrun/**` |
| KAN-285 docs | agent | `docs/next/**` except the CLI reference |
| KAN-286 templates | agent | `iii-hq/templates` |
| KAN-284 integration | orchestrator | integration branches, end-to-end runs |

`compose::build` (section 4.2) belongs to KAN-280 (`daemon.rs`, `remote.rs`); the first-build
and out-of-date checks read `build.json` through the module KAN-281 owns.

### 10.2 Process

For every ticket:

1. One agent implements it in its own worktree and branch, with its own `CARGO_TARGET_DIR`, and
   reports on the ticket.
2. A reviewer agent reviews it against this spec and the ticket's acceptance criteria and lists
   what must change.
3. The orchestrator weighs the review. What holds goes back to the same agent; what does not is
   answered on the ticket with the reason.
4. Steps 2 and 3 repeat until the reviewer is satisfied.
5. Only then a draft PR, in English: into the integration branch `feat/explicit-vm-mode` for
   `iii`, into `main` for `workers` and `templates`.

The integration ticket (KAN-284) closes the phase with one run of everything together:
`compose_sandbox_e2e` on real VMs, the Linkly tutorial (`docs/next/tutorials/linkly/_workspace/run.sh all`),
a 0.26 project migrating on its first `up`, and the host-env leak check of #2505.

### 10.3 Order

1. KAN-278, then KAN-279: they fix the names every other ticket uses.
2. KAN-280, KAN-281, KAN-282, KAN-283 in parallel; KAN-285 drafts from this spec.
3. KAN-286 once KAN-280 fixes the file format; KAN-285 final.
4. KAN-284.

## 11. Phase 2 (KAN-287)

- `worker::build` from a `Dockerfile`, inside the build VM, for `deploy: image` workers.
- `hermes`: published as source (`Dockerfile` and `src/`) instead of only an image on `ghcr.io`;
  its manifest declares `state` and `http`; its README stops advertising a `compose::add` that
  fails today.
- The `workers` release pipeline and the registry carry an `image` worker as source.

## 12. Decisions

1. Explicit commands (`iii sandbox build` / `iii sandbox run`), not a `runner:` field.
2. The build is always local, through `worker::build`; the default adapter builds in its own
   VM. Docker is an adapter's choice, never a requirement.
3. The run boots the build; nothing is installed at boot.
4. `.workers/` and `.build/` are per project, at the root, out of git. Base images and package
   archives stay in global caches.
5. A `path://` worker's code syncs live; its dependencies come from the build.
6. The data the worker writes is persistent and independent of the build.
7. An out-of-date build only logs.
8. No implicit VM path: old files migrate when they run, one container at a time.
9. Contract 1.3, additive, with a minimum `sandbox-libkrun` release.
10. Prebuilt images are out of scope.

## 13. Open questions

1. First build on `compose::add`/`up` (section 3.3): proposed automatic, so adding a VM worker
   still starts it in one step.
2. `.workers/<container>/` versus `.workers/<package>@<version>/` when two containers share a
   package: proposed per container, the same key as `.build/`.
3. The dependency file list of section 4.3, and whether a manifest may extend it.
4. A worker file under a build-written directory that shadows a rebuilt one (section 8.1):
   documented, or cleared by the adapter on rebuild.
