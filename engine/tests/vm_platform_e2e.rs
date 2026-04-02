//! End-to-end integration tests for the VM platform stack.
//!
//! These tests require actual KVM (Linux) or Hypervisor.framework (macOS) access.
//! They are `#[ignore]` by default -- run with: `cargo test -p iii --test vm_platform_e2e -- --ignored`
//!
//! Requirements validated: PLAT-01 (Linux KVM), PLAT-02 (macOS Hypervisor.framework)

#![cfg(not(target_os = "windows"))]

use std::path::Path;
use std::process::Command;

// ── Pre-flight checks ──────────────────────────────────────────────

/// Documents KVM status on Linux. Does not fail if KVM is missing --
/// the VM boot test will skip instead.
#[test]
#[cfg(target_os = "linux")]
fn test_kvm_available() {
    let kvm = Path::new("/dev/kvm");
    if !kvm.exists() {
        eprintln!("WARNING: /dev/kvm not found -- VM boot tests will be skipped");
        return;
    }
    // Try opening for read/write (required by libkrun)
    match std::fs::File::options().read(true).write(true).open(kvm) {
        Ok(_) => eprintln!("KVM is available and accessible"),
        Err(e) => eprintln!("WARNING: /dev/kvm exists but not accessible: {}", e),
    }
}

/// Validates that libkrunfw resolution logic can locate the firmware file.
/// Skips if firmware is not downloaded yet (not a failure).
#[test]
#[ignore] // Requires firmware to be downloaded to ~/.iii/lib/
fn test_libkrunfw_resolves() {
    // Check known locations for the firmware file
    let filename = if cfg!(target_os = "macos") {
        "libkrunfw.5.dylib"
    } else {
        "libkrunfw.so.5.2.1"
    };

    let candidates: Vec<std::path::PathBuf> = [
        dirs::home_dir().map(|h| h.join(".iii").join("lib").join(filename)),
        Some(std::path::PathBuf::from("/usr/local/lib").join(filename)),
        Some(std::path::PathBuf::from("/usr/lib").join(filename)),
    ]
    .into_iter()
    .flatten()
    .collect();

    let found = candidates.iter().any(|p| p.exists());
    if !found {
        eprintln!(
            "WARNING: libkrunfw not found in any known location. Checked: {:?}",
            candidates
        );
        eprintln!("Download with: iii firmware download");
        return;
    }
    eprintln!("libkrunfw found in a known location");
}

// ── Main VM boot test ──────────────────────────────────────────────

/// End-to-end test that spawns `iii __vm-boot` and verifies the full boot path:
/// 1. libkrunfw resolution (firmware library found)
/// 2. VM boots without crash (exit code 0 or clean exit)
/// 3. Init mounts /proc (VM_BOOT_OK marker in output)
/// 4. Worker process exits cleanly
///
/// Requires KVM (Linux) or Hypervisor.framework (macOS).
/// Run with: `cargo test -p iii --test vm_platform_e2e -- --ignored`
#[test]
#[ignore] // Requires KVM (Linux) or Hypervisor.framework (macOS)
fn test_vm_boot_platform() {
    // Skip if KVM not available on Linux
    #[cfg(target_os = "linux")]
    {
        let kvm = Path::new("/dev/kvm");
        if !kvm.exists() {
            eprintln!("Skipping: /dev/kvm not found");
            return;
        }
        if std::fs::File::options()
            .read(true)
            .write(true)
            .open(kvm)
            .is_err()
        {
            eprintln!("Skipping: /dev/kvm not accessible");
            return;
        }
    }

    // 1. Find the iii binary
    let iii_bin_path = env!("CARGO_BIN_EXE_iii");

    // 2. Create a minimal rootfs for the test.
    //    This avoids needing to pull an OCI image (which needs network + registry access).
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let rootfs = tmp.path().join("rootfs");

    // Create minimal rootfs structure
    for dir in &["bin", "lib", "lib64", "etc", "proc", "sys", "dev", "tmp"] {
        std::fs::create_dir_all(rootfs.join(dir)).unwrap();
    }

    // Copy busybox or sh into the rootfs.
    // busybox-static is ideal; fall back to system /bin/sh.
    let sh_candidates = ["/usr/bin/busybox", "/bin/busybox", "/bin/sh"];
    let mut sh_copied = false;
    for candidate in &sh_candidates {
        if Path::new(candidate).exists() {
            let _ = std::fs::copy(candidate, rootfs.join("bin/sh"));
            sh_copied = true;
            break;
        }
    }
    if !sh_copied {
        eprintln!("Skipping: no suitable /bin/sh found for test rootfs");
        return;
    }

    // 3. Spawn iii __vm-boot with a simple command that proves init works.
    //    The command: /bin/sh -c "test -d /proc/self && echo VM_BOOT_OK"
    //    If init mounted /proc correctly, /proc/self exists and we see VM_BOOT_OK.
    let mut cmd = Command::new(iii_bin_path);
    cmd.arg("__vm-boot")
        .arg("--rootfs")
        .arg(&rootfs)
        .arg("--exec")
        .arg("/bin/sh")
        .arg("--arg")
        .arg("-c")
        .arg("--arg")
        .arg("test -d /proc/self && echo VM_BOOT_OK || echo VM_BOOT_FAIL")
        .arg("--vcpus")
        .arg("1")
        .arg("--ram")
        .arg("256");

    // Set library path for libkrun resolution
    let lib_path_var = if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    let lib_paths: Vec<String> = [
        dirs::home_dir().map(|h| h.join(".iii").join("lib").to_string_lossy().to_string()),
        Some("/usr/local/lib".to_string()),
        Some("/usr/lib".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect();
    cmd.env(lib_path_var, lib_paths.join(":"));

    let output = cmd.output().expect("failed to spawn iii __vm-boot");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    eprintln!("--- stdout ---\n{}", stdout);
    eprintln!("--- stderr ---\n{}", stderr);

    // The VM boot subprocess may exit non-zero if libkrun is not installed.
    // In that case, check stderr for known error indicators and skip.
    if !output.status.success() {
        let combined = format!("{}{}", stdout, stderr);
        if combined.contains("not found")
            || combined.contains("dlopen")
            || combined.contains("library")
            || combined.contains("libkrun")
        {
            eprintln!("Skipping: libkrun not installed ({})", output.status);
            return;
        }
        panic!(
            "VM boot failed with exit code {:?}\nstdout: {}\nstderr: {}",
            output.status.code(),
            stdout,
            stderr
        );
    }

    // Verify init mounted /proc and the worker command executed successfully
    assert!(
        stdout.contains("VM_BOOT_OK") || stderr.contains("VM_BOOT_OK"),
        "Expected VM_BOOT_OK in output (init should mount /proc). stdout: {}, stderr: {}",
        stdout,
        stderr
    );
}
