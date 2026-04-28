# iii-worker integration tests

Two test tiers cover the `sandbox::*` trigger surface plus the broader worker.
Pick the right tier when adding a test.

## Tier A — in-process integration (default `cargo test`)

Hermetic. No libkrun, no shell socket, no III WebSocket. Each handler is
called directly with `Fake*` adapters from `tests/common/sandbox_fakes.rs`.
Runs in every PR; target runtime <1s combined.

| File | Surface | Tests |
|---|---|---|
| `sandbox_lifecycle_integration.rs` | `sandbox::{create,exec,stop,list}` | ~21 |
| `sandbox_workflow_integration.rs` | cross-handler scenarios + state transitions | 4 |
| `sandbox_fs_integration.rs` | `sandbox::fs::*` (10 triggers) | ~10 |
| `sandbox_error_codes_integration.rs` | wire ABI pin against `tests/fixtures/sandbox_error_codes.json` | 3 |

Run:
```
cargo test -p iii-worker
# or, faster:
cargo nextest run -p iii-worker
```

The `tests/common/sandbox_fakes.rs` module exposes `FakeShellRunner`,
`FakeVmStopper`, `FakeVmLauncher`. Each supports a configurable response,
typed error injection, and a `blocking()` mode that holds the call open
until a `oneshot` resolves — used by concurrency tests to verify the
registry mutex is never held across an adapter `await`.

## Tier C — real microVM end-to-end

Behind `#[ignore]` + env-var gates so they don't run in default CI.
Drives the production adapters (`IiiWorkerLauncher`, `ShellProtoRunner`,
`SignalStopper`) against a live libkrun guest.

| File | Surface | Tests |
|---|---|---|
| `vm_integration.rs` | `vm_boot.rs` arg construction (uses `--features integration-vm`) | many |
| `vm_lifecycle_integration.rs` | host↔guest lifecycle (SIGTERM, meminfo, rlimit) | 3 stubs |
| `sandbox_integration_e2e.rs` | `sandbox::*` trigger surface end-to-end | 3 stubs |

Host requirements:
- Linux + KVM, or macOS with Hypervisor.framework entitlements
- A cross-compiled `iii-init` binary in the rootfs
- Rootfs with `/bin/sh`, `/bin/echo`, `/bin/cat` (Alpine works)
- For `vm_integration.rs` only: build with `--features integration-vm`

Required env:
```
export III_VM_INTEGRATION_ROOTFS=/path/to/built/rootfs
# Optional, for the meminfo override test:
export III_VM_INTEGRATION_BUN_ROOTFS=/path/to/bun-rootfs
```

Run:
```
cargo test --test sandbox_integration_e2e -- --ignored
cargo test --test vm_lifecycle_integration -- --ignored
cargo test -p iii-worker --features integration-vm --test vm_integration
```

When a tier-C test flakes, fix the root cause — do not silently extend
the `#[ignore]` list. The whole point of tier-C is to catch bugs that
fakes can't see (URL rewrite, real-kernel networking, real signal
delivery), so an ignored test there is a hole in the safety net.

## Adding a new sandbox::* trigger

1. Add the typed `Request`/`Response` + `handle_*` in `sandbox_daemon/`.
2. Add a fake adapter constructor in `tests/common/sandbox_fakes.rs` if
   the trigger uses a new trait.
3. Add unit tests inline in the handler module (validation paths).
4. Add an in-process integration test in `sandbox_lifecycle_integration.rs`
   or `sandbox_workflow_integration.rs` — every error variant the handler
   can return must be exercised.
5. If the new trigger introduces a new `SandboxError` variant, add a row
   to `tests/fixtures/sandbox_error_codes.json` and update the
   exhaustiveness check in `sandbox_error_codes_integration.rs`.
6. Add a tier-C scenario in `sandbox_integration_e2e.rs` if the trigger
   has any host-kernel-visible behavior (networking, real fs semantics,
   resource limits).

## Cross-language error contract

`tests/fixtures/sandbox_error_codes.json` is the wire ABI for sandbox
error codes. Every Rust `SandboxErrorCode` variant must have a row.
The Node + Python SDKs receive `error.code` unchanged from the wire,
so the Rust-side contract pin is sufficient — but if SDKs ever start
mapping `code` to a typed exception class, that mapping must be
asserted against the same fixture (see `tests/fixtures/...json`'s
`_comment` field).
