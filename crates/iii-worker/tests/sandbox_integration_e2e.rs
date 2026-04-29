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
//!   - `III_VM_INTEGRATION_ROOTFS` env gate; `[skip]` printed if missing
//!   - Run with: `cargo test --test sandbox_integration_e2e -- --ignored`
//!
//! Host requirements:
//!   - Linux + KVM (or macOS with Hypervisor.framework entitlements)
//!   - A built `iii-init` binary cross-compiled for the guest arch
//!   - A rootfs with `/bin/sh`, `/bin/echo`, `/bin/cat` available
//!   - `iii-worker` built with the `integration-vm` feature
//!
//! Gaps tracked here are post-merge work; the test bodies are skeleton
//! stubs that emit a `[todo]` message and exit cleanly so the test count
//! stays honest. A Linux contributor with libkrun set up can replace
//! each `[todo]` with the driver code without touching the gating.

use std::path::PathBuf;

fn integration_rootfs() -> Option<PathBuf> {
    match std::env::var("III_VM_INTEGRATION_ROOTFS") {
        Ok(s) if !s.is_empty() => {
            let path = PathBuf::from(&s);
            if path.exists() {
                Some(path)
            } else {
                eprintln!("[skip] sandbox_integration_e2e: III_VM_INTEGRATION_ROOTFS={s} missing");
                None
            }
        }
        _ => {
            eprintln!("[skip] sandbox_integration_e2e: III_VM_INTEGRATION_ROOTFS not set");
            None
        }
    }
}

/// Full-surface E2E: walk all 14 sandbox::* triggers in one workflow.
///
/// 1.  sandbox::create with image=alpine, network=false
/// 2.  sandbox::fs::mkdir /work
/// 3.  sandbox::fs::write /work/hello.txt = "world"
/// 4.  sandbox::fs::stat /work/hello.txt -> size=5
/// 5.  sandbox::fs::ls /work -> ["hello.txt"]
/// 6.  sandbox::fs::grep "wor" /work/hello.txt -> match
/// 7.  sandbox::fs::sed s/world/earth/ /work/hello.txt
/// 8.  sandbox::fs::read /work/hello.txt -> "earth"
/// 9.  sandbox::fs::chmod 0600 /work/hello.txt
/// 10. sandbox::fs::mv /work/hello.txt /work/h2.txt
/// 11. sandbox::fs::rm /work/h2.txt
/// 12. sandbox::exec ["sh","-c","echo done"] -> exit=0
/// 13. sandbox::list -> 1 sandbox, state=Running
/// 14. sandbox::stop
/// 15. sandbox::list -> empty
///
/// Implementation note: this test must use a `SandboxGuard` RAII helper
/// so that any panic mid-scenario still tears down the VM. Otherwise
/// a single panicking test cascades into CI-host resource exhaustion.
#[test]
#[ignore = "sandbox-e2e: requires KVM + guest rootfs (integration-vm CI lane)"]
fn full_surface_e2e_walks_all_14_triggers() {
    let Some(rootfs) = integration_rootfs() else {
        return;
    };
    eprintln!(
        "[todo] sandbox_integration_e2e: full_surface_e2e_walks_all_14_triggers \
         has a rootfs at {} but the driver body is not yet implemented. \
         Build out using IiiWorkerLauncher + ShellProtoRunner + SignalStopper, \
         install a SandboxGuard RAII helper for panic-safe teardown.",
        rootfs.display()
    );
}

/// Verify network=false produces no host egress. Catches the
/// localhost-rewrite class of bugs hit on 2026-04-28 in vm_boot.rs.
#[test]
#[ignore = "sandbox-e2e: requires KVM + guest rootfs (integration-vm CI lane)"]
fn create_with_network_disabled_yields_no_host_egress() {
    let Some(rootfs) = integration_rootfs() else {
        return;
    };
    eprintln!(
        "[todo] sandbox_integration_e2e: create_with_network_disabled_yields_no_host_egress \
         has a rootfs at {} but the driver body is not yet implemented. \
         Shape: create network=false, exec `nc -w 2 <host_ip> <listening_port>`, \
         assert connection refused or unreachable.",
        rootfs.display()
    );
}

/// Tier-C teardown contract: a panic mid-scenario must not leak the VM.
/// The test deliberately panics; outer harness asserts the VM was reaped.
#[test]
#[ignore = "sandbox-e2e: requires KVM + guest rootfs (integration-vm CI lane)"]
fn raii_guard_reaps_vm_on_panic_mid_scenario() {
    let Some(rootfs) = integration_rootfs() else {
        return;
    };
    eprintln!(
        "[todo] sandbox_integration_e2e: raii_guard_reaps_vm_on_panic_mid_scenario \
         has a rootfs at {} but the driver body is not yet implemented. \
         Shape: SandboxGuard wraps a created sandbox; spawn a child process \
         that runs the test logic + panics; parent verifies VM pid is gone.",
        rootfs.display()
    );
}
