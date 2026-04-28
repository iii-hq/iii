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
//! The first real test driver is `sandbox_create_exec_stop_smoke`. The other
//! two scenarios remain `[todo]` stubs pending iteration on a Linux+KVM
//! host that can run them end-to-end.

#![cfg(feature = "integration-vm")]

use std::path::PathBuf;
use std::time::Instant;

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

/// Verify network=false produces no host egress. Catches the
/// localhost-rewrite class of bugs hit on 2026-04-28 in vm_boot.rs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "sandbox-e2e: requires KVM + guest rootfs (integration-vm CI lane)"]
async fn create_with_network_disabled_yields_no_host_egress() {
    let Some(_bin) = worker_binary() else {
        return;
    };
    eprintln!(
        "[todo] sandbox_integration_e2e: create_with_network_disabled_yields_no_host_egress \
         driver body not yet implemented. Shape: bind a TCP listener on the host, \
         create with network=Some(false), exec `nc -w 2 <host_ip> <port>` inside the guest, \
         assert connect failed and the host listener saw no accept(). Build out after \
         sandbox_create_exec_stop_smoke is green in CI."
    );
}

/// Tier-C teardown contract: a panic mid-scenario must not leak the VM.
/// The test deliberately panics; outer harness asserts the VM was reaped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "sandbox-e2e: requires KVM + guest rootfs (integration-vm CI lane)"]
async fn raii_guard_reaps_vm_on_panic_mid_scenario() {
    let Some(_bin) = worker_binary() else {
        return;
    };
    eprintln!(
        "[todo] sandbox_integration_e2e: raii_guard_reaps_vm_on_panic_mid_scenario \
         driver body not yet implemented. Shape: spawn child process that creates a \
         sandbox via the SandboxGuard pattern then panics; parent verifies the VM pid \
         is gone (kill(pid, 0) returns ESRCH). Build out after the smoke is green."
    );
}
