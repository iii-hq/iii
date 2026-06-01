// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Deferred integration tests for the VM boot + shutdown lifecycle.
//!
//! These tests exercise host↔guest behaviour that cannot be covered by
//! the pure-function unit tests in `vm_boot.rs` or `iii-init::mount`.
//! They require booting a real microVM and observing guest behaviour,
//! which means:
//!
//!   - Linux host (KVM) or macOS with Hypervisor.framework entitlements
//!   - A built `iii-init` binary cross-compiled for the guest arch
//!   - A rootfs pulled via the OCI path
//!   - ~30-60s runtime per test (VM boot + guest process + teardown)
//!
//! All tests are `#[ignore]`'d so `cargo test` in CI doesn't spin up
//! VMs by default. Each test checks env gates at entry and returns
//! with a visible skip message when its prerequisites aren't met, so
//! `cargo test -- --ignored` on a bare host prints which gaps
//! remain uncovered rather than panicking with `todo!()`. Run with
//! `cargo test --test vm_lifecycle_integration -- --ignored` on a
//! capable host.
//!
//! Env gates:
//!   - `III_VM_INTEGRATION_ROOTFS` must point at a prebuilt rootfs
//!     directory with `/bin/sh` and `/bin/sleep`.
//!   - `III_VM_INTEGRATION_BUN_ROOTFS` adds a bun-enabled rootfs path
//!     for the meminfo-override test.

use std::path::PathBuf;

/// Return the rootfs path, or emit a visible [skip] and return None.
fn integration_rootfs(env_var: &str) -> Option<PathBuf> {
    match std::env::var(env_var) {
        Ok(s) if !s.is_empty() => {
            let path = PathBuf::from(&s);
            if path.exists() {
                Some(path)
            } else {
                eprintln!("[skip] vm_lifecycle_integration: {env_var}={s} missing");
                None
            }
        }
        _ => {
            eprintln!("[skip] vm_lifecycle_integration: {env_var} not set");
            None
        }
    }
}

#[test]
#[ignore = "vm-integration: requires KVM + guest rootfs"]
fn exit_handle_sigterm_triggers_clean_shutdown() {
    // Goal: prove that SIGTERM delivered to a live __vm-boot process
    // drives the VMM's clean shutdown path (on_exit observers fire,
    // pidfile is removed, control socket is removed) instead of
    // tearing the process down mid-guest-write.
    //
    // Shape:
    //   1. Spawn `iii-worker __vm-boot --rootfs <fixture> --exec /bin/sleep
    //      --arg 60 --pid-file <tmp> --control-sock <tmp>`.
    //   2. Wait until the control socket exists (boot complete).
    //   3. kill(child_pid, SIGTERM).
    //   4. Assert child exits within N seconds (deadline <= 5s).
    //   5. Assert pidfile and control socket are both gone.
    //
    // Gap: Gap #1 (ExitHandle) from the integration plan.
    let Some(rootfs) = integration_rootfs("III_VM_INTEGRATION_ROOTFS") else {
        return;
    };
    eprintln!(
        "[todo] vm_lifecycle_integration: exit_handle_sigterm_triggers_clean_shutdown \
         has a rootfs at {} but the driver body is not yet implemented. \
         Tracked as Gap #1 (ExitHandle).",
        rootfs.display()
    );
}

#[test]
#[ignore = "vm-integration: requires KVM + bun rootfs"]
fn meminfo_override_caps_memtotal_for_guest_reader() {
    // Goal: prove that `override_proc_meminfo` actually bind-mounts
    // a capped `/proc/meminfo` over the real one and that guest
    // userspace (bun, cat) observes the cap.
    //
    // Shape:
    //   1. Spawn __vm-boot with --ram 2048 but III_WORKER_MEM_BYTES
    //      pinned to 384 MiB via cgroup.
    //   2. Run `cat /proc/meminfo | head -1` inside the guest.
    //   3. Assert MemTotal value ≈ 384 * 1024 kB (±1%).
    //   4. Also run a bun snippet: `bun -e
    //      'console.log(require("os").totalmem())'` and assert
    //      the returned bytes match the cap.
    //
    // Gap: Gap #2 (Bun meminfo) from the integration plan.
    let Some(rootfs) = integration_rootfs("III_VM_INTEGRATION_BUN_ROOTFS") else {
        return;
    };
    eprintln!(
        "[todo] vm_lifecycle_integration: meminfo_override_caps_memtotal_for_guest_reader \
         has a rootfs at {} but the driver body is not yet implemented. \
         Tracked as Gap #2 (Bun meminfo).",
        rootfs.display()
    );
}

#[test]
#[ignore = "vm-integration: requires KVM + rootfs"]
fn nofile_rlimit_applies_to_guest_worker_process() {
    // Goal: prove that `--nofile-limit N` passes through libkrun's
    // KRUN_RLIMITS env and is visible to the guest worker before
    // iii-init re-raises it (belt-and-suspenders path).
    //
    // Shape:
    //   1. Spawn __vm-boot with --nofile-limit 16384.
    //   2. Worker runs `sh -c "ulimit -n"` and writes to stdout log.
    //   3. Assert log reports 16384.
    //
    // Gap: Gap #3 (rlimit) from the integration plan.
    let Some(rootfs) = integration_rootfs("III_VM_INTEGRATION_ROOTFS") else {
        return;
    };
    eprintln!(
        "[todo] vm_lifecycle_integration: nofile_rlimit_applies_to_guest_worker_process \
         has a rootfs at {} but the driver body is not yet implemented. \
         Tracked as Gap #3 (rlimit).",
        rootfs.display()
    );
}
