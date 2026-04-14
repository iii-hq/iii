# TODOs

Deferred work surfaced by plan reviews. Promote items to open PRs when they become blocking or when capacity is available.

---

## Frontend test infrastructure

**What:** Scaffold Vitest + React Testing Library for `console/packages/console-frontend` and write tests for the Workers page (including the isolation badge renderer and `getIsolationBadgeClass` case-normalization).

**Why:** Today the console-frontend has no automated tests at all. The isolation badge plan (feat/console-workers-route-state) ships with typecheck-only coverage for the new helper and detail row. Every future UI change carries the same gap.

**Pros:** Regression net for the Workers page and future console features. Faster iteration (unit tests catch breakage in seconds vs. manual browser checks). Unblocks the DX review's recommendation for a Workers page screenshot test.

**Cons:** New test infra to maintain. Choice of framework (Vitest vs Jest) is a small bikeshed. Existing React components weren't written test-first, so setup may surface small refactors.

**Context:** Pre-existing gap in iii — surfaced by `/plan-eng-review` during the isolation-badge plan review (branch `feat/console-workers-route-state`). The plan explicitly scoped frontend coverage to typecheck-only because adding test infra would have doubled the PR size and was out of the original scope. Captured here so it doesn't fall through.

**Depends on / blocked by:** Nothing. Can be picked up independently by anyone touching `console/packages/console-frontend/`.

---

## SDK-side isolation auto-detection

**What:** Add runtime environment detection to all three SDKs (Node, Python, Rust) so `isolation` is populated automatically when the worker runs in docker, Kubernetes, a VM, or similar, without requiring the user to set `III_ISOLATION` manually.

Detection signals to check (in order):
- `/proc/1/cgroup` contents → `docker`, `containerd`, `kubepods` substrings
- `KUBERNETES_SERVICE_HOST` env var present → `kubernetes`
- `DOCKER_CONTAINER` / `container` env vars
- `/sys/hypervisor/type` or DMI data on Linux → `vm` / specific hypervisor name
- macOS/Windows equivalents where meaningful

**Why:** The v1 isolation plan (feat/console-workers-route-state) ships with env-var-only semantics: `III_ISOLATION` is read and forwarded, no heuristics. This works well for `iii worker dev` and `iii worker add <image>` (both auto-injected via the libkrun launcher), but a backend dev running their SDK inside docker or k8s without setting the env var will see no badge on the Workers page. Detection closes that gap in prod deployments.

**Pros:** Badge lights up in real production environments without any manual configuration. Matches the "zero-friction" DX principle the plan already committed to for `iii worker dev`. Prod debugging gets better automatically (ops engineer sees which workers are in k8s vs bare metal without reading deployment manifests).

**Cons:** 3× implementation (Node/Python/Rust) with language-specific heuristics. Potential false positives (e.g., rootless Docker, containers inside VMs). Heuristic maintenance burden as new runtimes appear. Cgroup format changed between cgroups v1 and v2.

**Context:** Deferred in v1 per the "explicit > clever, minimal diff" preference captured in the plan's Implementation notes. The wire contract (`isolation: Option<String>` field on `RegisterWorkerInput`) and the UI helper are already in place, so detection is a pure SDK-layer addition — no engine or frontend changes required. Surfaced by `/plan-devex-review` as an alternative approach, then re-surfaced by `/plan-eng-review` as a follow-up TODO.

**Depends on / blocked by:** The v1 isolation plan landing first. After that, each SDK can be updated independently in its own PR.
