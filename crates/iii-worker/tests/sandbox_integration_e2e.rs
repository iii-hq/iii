// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.

//! Tier-C end-to-end tests for the sandbox::* trigger surface.
//!
//! Sister to `sandbox_lifecycle_integration.rs` (tier-A, hermetic) and
//! `sandbox_workflow_integration.rs` (tier-A workflow). This file boots
//! a real microVM through the production adapters (`IiiWorkerLauncher`,
//! `ShellProtoRunner`, `SignalStopper`) and drives the handlers end-to-end.
//!
//! Same gating pattern as `vm_lifecycle_integration.rs`:
//!   - `#[ignore]` so default `cargo test` does not boot a VM
//!   - env gate `III_WORKER_BINARY_OVERRIDE` (must point at the iii-worker
//!     binary built earlier in the CI job; production never sets it)
//!   - Run with: `cargo test --features integration-vm --test sandbox_integration_e2e -- --ignored`
//!
//! Host requirements:
//!   - Linux + KVM (or macOS with Hypervisor.framework entitlements)
//!   - `iii-worker` built with `--features integration-vm` and on a path
//!     exported via `III_WORKER_BINARY_OVERRIDE`
//!   - Network access to docker.io for the first `iiidev/python:latest` pull
//!     (~30 s on cold cache; subsequent runs hit `~/.iii/cache/`)
//!
//! All three test bodies are real drivers:
//!   - `sandbox_create_exec_stop_smoke` — happy-path lifecycle
//!   - `create_with_network_disabled_yields_no_host_egress` — egress isolation
//!   - `raii_guard_reaps_vm_on_panic_mid_scenario` — teardown contract under panic

#![cfg(feature = "integration-vm")]

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use uuid::Uuid;

use iii_worker::sandbox_daemon::adapters::{IiiWorkerLauncher, ShellProtoRunner, SignalStopper};
use iii_worker::sandbox_daemon::config::SandboxConfig;
use iii_worker::sandbox_daemon::create::{CreateRequest, handle_create};
use iii_worker::sandbox_daemon::exec::{ExecRequest, handle_exec};
use iii_worker::sandbox_daemon::list::{ListRequest, handle_list};
use iii_worker::sandbox_daemon::registry::SandboxRegistry;
use iii_worker::sandbox_daemon::stop::{StopRequest, handle_stop};

/// Returns Some(path) when `III_WORKER_BINARY_OVERRIDE` points at an
/// existing executable, otherwise prints a `[skip]` line and returns None.
/// The same env var is read by `IiiWorkerLauncher::boot()` to override
/// `current_exe()` (which in tests points at the test binary, not iii-worker).
fn worker_binary() -> Option<PathBuf> {
    match std::env::var("III_WORKER_BINARY_OVERRIDE") {
        Ok(s) if !s.is_empty() => {
            let path = PathBuf::from(&s);
            if path.is_file() {
                Some(path)
            } else {
                eprintln!(
                    "[skip] sandbox_integration_e2e: III_WORKER_BINARY_OVERRIDE={s} \
                     does not point at a file"
                );
                None
            }
        }
        _ => {
            eprintln!(
                "[skip] sandbox_integration_e2e: III_WORKER_BINARY_OVERRIDE not set; \
                 build iii-worker and export the path before running with --ignored"
            );
            None
        }
    }
}

/// RAII guard: tears down the sandbox on drop, even if the test panics.
/// Without this, a panicking test leaves a libkrun guest process alive
/// on the CI host until the runner image is recycled — eventual but not
/// immediate, and noisy.
struct SandboxGuard {
    id: String,
    registry: SandboxRegistry,
}

impl SandboxGuard {
    fn new(id: String, registry: SandboxRegistry) -> Self {
        Self { id, registry }
    }
}

impl Drop for SandboxGuard {
    fn drop(&mut self) {
        // Synchronous drop in async tests requires building a small
        // runtime to await the stop call. Failures are logged, not
        // propagated, because Drop cannot return errors.
        let id = self.id.clone();
        let registry = self.registry.clone();
        let stopper = SignalStopper;
        let res = std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("SandboxGuard: failed to build teardown runtime: {e}");
                    return;
                }
            };
            rt.block_on(async {
                let req = StopRequest {
                    sandbox_id: id.clone(),
                    wait: true,
                };
                if let Err(e) = handle_stop(req, &registry, &stopper).await {
                    eprintln!("SandboxGuard: teardown of {id} failed: {e}");
                }
            });
        })
        .join();
        if let Err(panic_payload) = res {
            eprintln!("SandboxGuard: teardown thread panicked: {panic_payload:?}");
        }
    }
}

/// Smoke test: drive `sandbox::create` -> `sandbox::list` -> `sandbox::exec`
/// -> `sandbox::stop` -> `sandbox::list` end-to-end against a real microVM.
///
/// Asserts:
///   - create returns a uuid sandbox_id
///   - list shows the new sandbox (image=python, not stopped, not exec_in_progress)
///   - exec("echo", "tier-c-smoke") returns exit_code=0 and stdout contains the marker
///   - stop returns stopped=true
///   - list is empty after stop
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "sandbox-e2e: requires KVM + iii-worker binary (integration-vm CI lane)"]
async fn sandbox_create_exec_stop_smoke() {
    let Some(bin) = worker_binary() else {
        return;
    };
    eprintln!(
        "sandbox_integration_e2e: using iii-worker binary at {}",
        bin.display()
    );

    // auto_install=true so the test self-heals if the rootfs cache is
    // empty. CI may pre-seed via a setup step to skip the ~30s OCI pull
    // on each run, but the test does not require it.
    let cfg = SandboxConfig {
        auto_install: true,
        image_allowlist: vec!["python".into()],
        ..Default::default()
    };
    let registry = SandboxRegistry::new();
    let launcher = IiiWorkerLauncher;
    let runner = ShellProtoRunner;
    let stopper = SignalStopper;

    // 1. create
    let t_create = Instant::now();
    let create_resp = handle_create(
        CreateRequest {
            image: "python".into(),
            cpus: Some(1),
            memory_mb: Some(512),
            name: None,
            network: Some(false),
            idle_timeout_secs: Some(60),
            env: vec![],
        },
        &cfg,
        &registry,
        &launcher,
        |e| eprintln!("create event: {e:?}"),
    )
    .await
    .expect("handle_create must succeed against a real rootfs + KVM");
    let id = create_resp.sandbox_id;
    eprintln!(
        "create: sandbox_id={id} in {} ms",
        t_create.elapsed().as_millis()
    );
    assert!(
        Uuid::parse_str(&id).is_ok(),
        "sandbox_id must be a valid uuid, got {id}"
    );

    // RAII teardown — runs even if the assertions below panic.
    let _guard = SandboxGuard::new(id.clone(), registry.clone());

    // 2. list — shows the new sandbox
    let listed = handle_list(ListRequest::default(), &registry).await;
    assert_eq!(listed.sandboxes.len(), 1);
    let summary = &listed.sandboxes[0];
    assert_eq!(summary.sandbox_id, id);
    assert_eq!(summary.image, "python");
    assert!(!summary.stopped);
    assert!(!summary.exec_in_progress);

    // 3. exec
    let t_exec = Instant::now();
    let exec_resp = handle_exec(
        ExecRequest {
            sandbox_id: id.clone(),
            cmd: "/bin/echo".into(),
            args: vec!["tier-c-smoke".into()],
            stdin: None,
            env: vec![],
            timeout_ms: Some(15_000),
            workdir: None,
        },
        &registry,
        &runner,
    )
    .await
    .expect("handle_exec must succeed once the VM is up");
    eprintln!("exec: {} ms", t_exec.elapsed().as_millis());
    assert_eq!(
        exec_resp.exit_code,
        Some(0),
        "echo must exit 0; stderr={:?}",
        exec_resp.stderr
    );
    assert!(
        exec_resp.stdout.contains("tier-c-smoke"),
        "stdout must contain the echo marker, got {:?}",
        exec_resp.stdout
    );
    assert!(!exec_resp.timed_out);

    // 4. stop (also triggered by guard if we panic above)
    let stop_resp = handle_stop(
        StopRequest {
            sandbox_id: id.clone(),
            wait: true,
        },
        &registry,
        &stopper,
    )
    .await
    .expect("handle_stop must succeed");
    assert!(stop_resp.stopped);

    // 5. list — empty
    let after = handle_list(ListRequest::default(), &registry).await;
    assert!(
        after.sandboxes.is_empty(),
        "list must be empty after stop, got {:?}",
        after.sandboxes
    );

    // Guard's drop is now a no-op (registry doesn't know about this id
    // anymore), which the guard tolerates with a logged warning. That's
    // the expected steady-state for the happy path.
}

/// Verify network=false produces no host egress. Catches the localhost-rewrite
/// class of bugs hit on 2026-04-28 in vm_boot.rs (which manifested by mangling
/// URLs into invalid forms when networking was disabled — a problem only
/// observable end-to-end with a real guest).
///
/// Strategy: with network=false, libkrun attaches no virtio-net device. From
/// inside the guest, every outbound socket connect must fail (no route, no
/// resolver, no interface). The python preset has `python` available so we
/// drive a small inline script that attempts a TCP connect to a public IP and
/// asserts the call returned a non-zero exit. We deliberately probe `1.1.1.1`
/// (Cloudflare) on port 443: it is globally reachable from any host with real
/// networking, so a success would mean network=false is broken.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "sandbox-e2e: requires KVM + guest rootfs (integration-vm CI lane)"]
async fn create_with_network_disabled_yields_no_host_egress() {
    let Some(bin) = worker_binary() else {
        return;
    };
    eprintln!(
        "sandbox_integration_e2e: using iii-worker binary at {}",
        bin.display()
    );

    let cfg = SandboxConfig {
        auto_install: true,
        image_allowlist: vec!["python".into()],
        ..Default::default()
    };
    let registry = SandboxRegistry::new();
    let launcher = IiiWorkerLauncher;
    let runner = ShellProtoRunner;

    let create_resp = handle_create(
        CreateRequest {
            image: "python".into(),
            cpus: Some(1),
            memory_mb: Some(512),
            name: None,
            network: Some(false), // the property under test
            idle_timeout_secs: Some(60),
            env: vec![],
        },
        &cfg,
        &registry,
        &launcher,
        |_| {},
    )
    .await
    .expect("handle_create with network=false must still boot the VM");
    let id = create_resp.sandbox_id;

    // RAII teardown — fires whether the assertion below passes, fails, or panics.
    let _guard = SandboxGuard::new(id.clone(), registry.clone());

    // Probe outbound connectivity. With network=false there is no virtio-net
    // device; the connect must fail. Short in-script timeout (3 s) so a
    // misconfiguration surfaces quickly instead of stalling the test.
    //
    // Exit semantics:
    //   0 = connect succeeded — network=false is BROKEN
    //   2 = connect raised an exception (expected: socket.timeout, OSError)
    //   any other = unexpected interpreter error
    let probe_script = r#"
import socket, sys
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.settimeout(3.0)
try:
    s.connect(('1.1.1.1', 443))
    print('UNEXPECTED_CONNECT_OK')
    sys.exit(0)
except (socket.timeout, OSError) as e:
    print(f'CONNECT_FAILED: {type(e).__name__}: {e}')
    sys.exit(2)
"#;

    let resp = handle_exec(
        ExecRequest {
            sandbox_id: id.clone(),
            cmd: "/usr/bin/python3".into(),
            args: vec!["-c".into(), probe_script.into()],
            stdin: None,
            env: vec![],
            timeout_ms: Some(15_000),
            workdir: None,
        },
        &registry,
        &runner,
    )
    .await
    .expect("handle_exec must reach the guest even with network=false");

    eprintln!(
        "probe stdout: {:?}\nprobe stderr: {:?}\nexit: {:?}",
        resp.stdout, resp.stderr, resp.exit_code
    );

    assert_ne!(
        resp.exit_code,
        Some(0),
        "guest with network=false must NOT reach 1.1.1.1; got exit=0 stdout={:?}",
        resp.stdout
    );
    assert!(
        !resp.stdout.contains("UNEXPECTED_CONNECT_OK"),
        "guest must not have egress; stdout={:?}",
        resp.stdout
    );
    assert!(
        resp.stdout.contains("CONNECT_FAILED"),
        "expected probe to print CONNECT_FAILED; got stdout={:?}, stderr={:?}",
        resp.stdout, resp.stderr
    );
}

/// Tier-C teardown contract: a panic inside a `SandboxGuard`-guarded scope
/// must reap the VM. Without this property a single test failure leaks a
/// libkrun guest process per failed run, which compounds into CI host
/// resource exhaustion.
///
/// Strategy:
///   1. Create a real sandbox; capture the libkrun child pid.
///   2. Inside `catch_unwind`, construct a `SandboxGuard` that owns the id
///      and immediately panic. The guard is dropped during stack unwinding.
///   3. After the unwind completes, verify (a) the libkrun pid is gone via
///      `kill(pid, 0) == ESRCH`, and (b) the registry no longer knows about
///      the sandbox. Both conditions must hold for the contract to be met.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "sandbox-e2e: requires KVM + guest rootfs (integration-vm CI lane)"]
async fn raii_guard_reaps_vm_on_panic_mid_scenario() {
    let Some(bin) = worker_binary() else {
        return;
    };
    eprintln!(
        "sandbox_integration_e2e: using iii-worker binary at {}",
        bin.display()
    );

    let cfg = SandboxConfig {
        auto_install: true,
        image_allowlist: vec!["python".into()],
        ..Default::default()
    };
    let registry = SandboxRegistry::new();
    let launcher = IiiWorkerLauncher;

    // 1. Create the sandbox we will deliberately abandon mid-test.
    let create_resp = handle_create(
        CreateRequest {
            image: "python".into(),
            cpus: Some(1),
            memory_mb: Some(512),
            name: None,
            network: Some(false),
            idle_timeout_secs: Some(60),
            env: vec![],
        },
        &cfg,
        &registry,
        &launcher,
        |_| {},
    )
    .await
    .expect("handle_create must succeed before testing teardown");
    let id = create_resp.sandbox_id;
    let id_uuid = Uuid::parse_str(&id).expect("sandbox_id must be a valid uuid");
    let vm_pid = registry
        .get(id_uuid)
        .await
        .expect("registry must contain the new sandbox")
        .vm_pid
        .expect("freshly-created sandbox must have a vm_pid");
    eprintln!("created sandbox {id} with vm_pid={vm_pid}");

    // 2. Trigger a panic inside a guard-protected scope. AssertUnwindSafe
    // is required because SandboxRegistry holds a Mutex (not UnwindSafe by
    // default); we know the cleanup path tolerates being entered during
    // unwind because Drop catches its own errors with eprintln.
    let registry_for_panic = registry.clone();
    let id_for_panic = id.clone();
    let panic_outcome = std::panic::catch_unwind(AssertUnwindSafe(move || {
        let _guard = SandboxGuard::new(id_for_panic, registry_for_panic);
        panic!("simulated test failure inside guarded scope");
    }));
    assert!(
        panic_outcome.is_err(),
        "the inner block was supposed to panic so we could observe cleanup"
    );

    // SandboxGuard::drop spawns a std thread and joins it, so the cleanup
    // is synchronous by the time catch_unwind returns. Sleep briefly anyway
    // because the kernel may need a tick to reap the child after SIGKILL.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 3a. The libkrun child must be gone. kill(pid, 0) returns -1/ESRCH
    // for a reaped process; returns 0 for a live process. Use a short loop
    // because some kernels delay marking the pid free for a few ms after
    // the SIGKILL is delivered.
    let alive_now = pid_alive(vm_pid);
    let mut attempts = 0;
    let mut still_alive = alive_now;
    while still_alive && attempts < 10 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        still_alive = pid_alive(vm_pid);
        attempts += 1;
    }
    assert!(
        !still_alive,
        "vm_pid {vm_pid} is still alive after panic + guard drop + {} ms grace;          the SandboxGuard contract is broken",
        500 + attempts * 100
    );

    // 3b. The registry must no longer track the sandbox.
    assert!(
        registry.get(id_uuid).await.is_err(),
        "registry must have removed the sandbox after the guard's stop call"
    );
}

/// Local copy of `adapters::pid_alive`. Kept inline so this test file does
/// not have to add a new pub item to the adapters module just for testing.
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) with signum 0 performs error checking only —
    // no signal is delivered, no side effects on the target process.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    true
}
