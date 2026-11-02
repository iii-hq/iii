# iii-loadtest

Capacity and resource benchmarks for the iii engine: how much load an engine
of a given size takes, what it costs in CPU and memory on the way, and where it
stops. Each run boots a microVM sized like the machine you would deploy on and
drives the engine with real SDK workers.

The criterion micro-benchmarks in `engine/benches/` time single code paths in
one process. This tool measures the whole engine under load, on bounded
resources.

## How a run works

```
host ─ iii-loadtest suite
        │   pins the engine's vCPUs to one half of the physical cores,
        │   the load generator's to the other
        └─ microVM (libkrun, KVM)  N + K vCPUs, M + L MiB
             iii-init → iii-loadtest run
               ├─ engine        vCPUs 0..N-1, cgroup memory.max = M MiB
               └─ load + SDK    vCPUs N..N+K-1  ── guest loopback ──► engine
```

- **The VM** is a libkrun microVM, the same technology `iii worker` uses for
  sandboxes. Its root filesystem holds only the engine, this tool, the host's
  shared libraries they load, `/bin/sh` and `iii-init`; nothing else competes
  with the engine. The VM has no network device: engine and load talk over the
  guest's own loopback, as two processes on one machine do, and nothing
  proxies their sockets.
- **The engine** gets `N` vCPUs (it is pinned to them, so it sizes its runtime
  to `N`) and a cgroup capped at `M` MiB with no swap. Past the cap the kernel
  OOM-kills it, and the run reports that: that is where the profile stops.
- **The load generator** runs in the same VM on `K` vCPUs of its own
  (`--load-vcpus`, default 6) with `L` MiB (`--load-ram`, default 2048), so it
  never takes the engine's CPU. It reports its own CPU use too, so a step the
  generator could not saturate is visible.
- **On the host**, the engine's vCPU threads are pinned to one half of the
  physical cores and the load generator's to the other (SMT siblings stay
  together), so the two never share a core.
- **The `host` profile** runs the same engine as a plain process, pinned the
  same way and not capped: the reference without a VM.

Every scenario gets a fresh engine (a new VM, a clean working directory). The
engine is sampled every 250 ms from `/proc`: its CPU time, RSS and peak RSS,
threads, how busy its CPUs were (kernel work on them included) and its
cgroup's memory.

## Scenarios

| scenario | what it measures |
|---|---|
| `rpc` | Worker-to-worker calls through the engine with nobody observing, at concurrency 1 to 2048, then with 1/16/256 KiB payloads. This is the routing baseline. |
| `fanout` | The `rpc` load at concurrency 64 while each called function gains 0 → 64 observers. Shows the cost per activation, whether the observed call slows down, and when activations are dropped. |
| `filters` | One function with 16 → 4000 observers, each filtering on its own `input.k`, so every call matches exactly one. Shows filter evaluation at scale (the per-function cap is 4096). |
| `observers` | Observers held by the engine, 1k → 64k, at rest: creation rate, memory per observer, and the memory after they are all disposed. |
| `source` | An event source, as a cron or database source does it: a private function whose handler returns the event, called in-process by its own worker, which delivers each call itself. First, floods: calls with 1 → 256 observers as fast as the worker makes them. Then a paced ladder over 16 observers (1k → 500k activations/s), up to the first rate that is not delivered in full. That rate is the sustained capacity the summary reports. |
| `match-keys` | An event source that matches observers by keys (`onObserverMatch`), as the database source does with its tables. 100 → 10,000 observers each take their own key; the worker calls the function with keys that match exactly one observer, then with a key nobody observes. The work per call stays one hash lookup, one filter evaluation and one report item, whatever the number of observers. The scenario raises the engine's `max_per_function` to 10240; `--observers max_per_function=…` overrides it. |
| `local` | A worker calling its own function in-process (local dispatch): unobserved, observed with a 1% filter match, and observed with every call matching. |
| `overload` | Observers whose target takes 20 ms, behind fast calls. Checks that queues stay at `max_queue_per_observer`, that the excess is dropped (`OVERFLOW`), and that the observed calls do not slow down. It runs with 4 observers at rising concurrency, then with 64 and 256 of them, each holding a full queue. The engine's memory then shows what a queued activation costs. |
| `connections` | Idle workers (raw WebSockets, 2 functions each), 100 → 10k: memory per connected worker, connect rate, the CPU an idle fleet costs, and what is left after they all disconnect. |

`iii-loadtest scenarios` prints this list.

## Running it

Requirements:
- Linux with `/dev/kvm` usable by your user.
- iii installed, which provides `~/.iii/lib/libkrunfw.so` and `~/.local/bin/iii-init`. Override them with `--krunfw` and `--init`.
- `ldd`.

```sh
cargo build --release -p iii -p iii-loadtest

# Smoke run: every scenario, short steps (a few minutes per profile)
target/release/iii-loadtest suite --quick --profiles host,2x1024

# The full matrix (about 10 minutes per profile)
target/release/iii-loadtest suite --profiles host,1x512,2x1024,4x2048,8x4096

# One scenario, another engine (compare releases), tracing off
target/release/iii-loadtest suite --scenarios rpc,fanout \
  --engine ~/.local/bin/iii --observability off
```

Results go to `target/loadtest/<timestamp>/`:
- `report.md`: a capacity summary per profile, then every step of every scenario.
- `results.json`: the same data, complete, including latency percentiles and resources per step.
- `<profile>/<scenario>/`: the engine's `config.yaml` and `engine.log`, plus the guest `console.log` for VM profiles.

`iii-loadtest report <dir>` renders `report.md` again from `results.json`.

A scenario on a VM profile stops as soon as its engine dies: the run records `oom_killed` when the cgroup's
memory cap did it, and the rest of the suite goes on. Stopping the suite (Ctrl-C) takes down the VM or engine it
is running.

Useful options:
- `--step-secs`: measured seconds per load step (default 10).
- `--observers key=value`: an engine `observers:` setting, for example `--observers default_max_in_flight=256`.
- `--engine-log-level`: the engine's log level. The default, `info`, is what ships, and it logs every registration.
- `--observability off`: turns the engine's tracing and metrics off, to see what they cost. The default is as shipped: every span recorded, and traces kept in memory and on disk.
- `--engine-cpus` and `--load-cpus`: the host CPU split, for example `4-7,12-15`.
- `--port`: the engine's port. It only matters for `host`: the suite refuses a port that a running process's `III_URL` points at, since a stale worker would join the engine under test.

The building blocks are subcommands of their own:
- `rootfs` builds the guest root.
- `vm` boots a microVM and runs one command. It takes `--tsi` to bridge the guest's sockets to the host, which lets you reach a guest engine by hand, but every socket operation then goes through the VMM and costs several times more.
- `run` runs one scenario against a fresh engine on the current machine.

## In CI, and on iii.dev/benchmarks

`.github/workflows/loadtest.yml` runs the full matrix (`host,1x512,2x1024,4x2048`) on every `iii/v*` tag, every
Monday on `main`, and on demand. A manual run can pick profiles and scenarios, and `quick` checks the pipeline
without publishing. Each run:
1. Builds `iii`, `iii-loadtest` and the static guest init (`iii-init`, musl).
2. Runs the suite with the repository's `engine/firmware/libkrunfw-linux-x86_64.so`.
3. Puts `report.md` in the job summary and keeps `results.json`, the summary and every engine log as an artifact for 90 days.
4. Files the run on the `gh-pages` branch with `iii-loadtest publish` (`loadtest/runs/<id>.json`, plus `loadtest/index.json`, newest first).
5. Dispatches the website deploy, which rebuilds iii.dev/benchmarks from the published data (`website/scripts/generate-benchmarks.ts`).

It needs a GitHub-hosted larger runner: Linux x64 with 16 vCPUs, added to the repository by an admin (Settings → Actions → Runners → New runner). The workflow uses the label `iii-loadtest-16-cores`; set the `LOADTEST_RUNNER` repository variable to use another. The first step checks `/dev/kvm` and fails clearly if the runner has none.

`iii-loadtest export <dir>` turns a run's `results.json` into the summary the page reads. The summary holds the machine, the settings, every step in a compact shape, and per profile the numbers the page leads with, computed in one place (`src/export.rs`). `iii-loadtest publish <summary> --pages <gh-pages checkout>` files it.

## Reading the numbers

- **CPU** is in cores: 1.00 means one CPU busy for the whole step.
  - *engine* is the engine process.
  - *CPUs busy* is how busy the engine's CPUs were, including the guest kernel's work on them (loopback TCP, softirqs).
- **Saturation.** When *engine* approaches N and the rate stops growing with concurrency, the engine is the limit. If instead the load generator's CPU (in `results.json`) approaches K, the generator was the limit. Add `--load-vcpus` and rerun.
- **Memory.** RSS is the engine's resident memory. The cgroup peak (in `results.json`) adds the page cache it touched. Glibc keeps freed memory, so "after the load" shows what the engine holds on to, not only what it still uses.
- **Latency at low concurrency.** It is higher in a VM than on `host`, with the same engine: that is the cost of virtualization at low load. Throughput under load is the number to compare across sizes.
- **Per-observer delivery ceiling.** A single observer delivers at most about `max_in_flight / target latency` activations per second (64 in flight against a 2 ms round trip is about 32k/s). Past that, its queue (`max_queue_per_observer`) fills and the engine drops the rest with `OVERFLOW`. `fanout`, `local` and `source` show this ceiling.
- **Which build.** `--engine` defaults to `target/release/iii`, a glibc build like the one in the Docker image. On x86_64 Linux, `install.sh` installs the static musl build unless `III_USE_GLIBC` is set, and musl's allocator is slower under load. Pass that binary with `--engine` to measure it.

## Known quirk

With msb_krun 0.1.16 on x86_64, a guest whose RAM ends in the 32 MiB just below the 32-bit MMIO gap (3296–3328 MiB) boots nothing and exits 0 without a word. `vm` moves such sizes to 3329 MiB.
