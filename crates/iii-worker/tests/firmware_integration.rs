// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for firmware resolution chain and constants.
//!
//! Covers requirements FW-01 through FW-04:
//! - FW-01: Local firmware resolution (env var override, ~/.iii/lib/)
//! - FW-02: Embedded fallback behavior (documented, observational)
//! - FW-03: Download fallback URL construction (archive names, platform support)
//! - FW-04: All sources fail returns None

mod common;

use common::assertions::assert_paths_eq;
use iii_worker::cli::firmware::constants::{
    III_INIT_FILENAME, check_libkrunfw_platform_support, iii_init_archive_name,
    libkrunfw_archive_name, libkrunfw_filename,
};
use iii_worker::cli::firmware::resolve::{
    lib_path_env_var, resolve_init_binary, resolve_libkrunfw_dir,
};
use std::sync::Mutex;

/// Serializes tests that mutate environment variables (HOME, III_LIBKRUNFW_PATH, III_INIT_PATH).
/// Each integration test file runs in its own process, so only intra-file locking is needed.
static ENV_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// Group 1: Firmware resolution with local path (FW-01)
// ---------------------------------------------------------------------------

/// FW-01: resolve_libkrunfw_dir finds firmware in ~/.iii/lib/ when present.
///
/// Threat mitigation T-03-05: HOME is saved before override and restored
/// immediately after the function call, before any assertions.
#[test]
fn resolve_libkrunfw_dir_finds_local_firmware() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let lib_dir = tmp.path().join(".iii").join("lib");
    std::fs::create_dir_all(&lib_dir).unwrap();

    let filename = libkrunfw_filename();
    std::fs::write(lib_dir.join(&filename), b"fake-firmware").unwrap();

    let original_home = std::env::var("HOME").ok();
    // SAFETY: test-only, serialized via ENV_LOCK
    unsafe {
        std::env::set_var("HOME", tmp.path());
        std::env::remove_var("III_LIBKRUNFW_PATH");
    }

    let result = resolve_libkrunfw_dir();

    // Restore HOME immediately (T-03-05 mitigation)
    unsafe {
        if let Some(ref home) = original_home {
            std::env::set_var("HOME", home);
        }
    }

    assert!(
        result.is_some(),
        "expected Some when firmware exists in ~/.iii/lib/"
    );
    assert_paths_eq(&result.unwrap(), &lib_dir);
}

/// FW-01: resolve_init_binary finds iii-init in ~/.iii/lib/ when present.
#[test]
fn resolve_init_binary_finds_local_init() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let lib_dir = tmp.path().join(".iii").join("lib");
    std::fs::create_dir_all(&lib_dir).unwrap();

    let init_path = lib_dir.join(III_INIT_FILENAME);
    std::fs::write(&init_path, b"fake-init").unwrap();

    let original_home = std::env::var("HOME").ok();
    unsafe {
        std::env::set_var("HOME", tmp.path());
        std::env::remove_var("III_INIT_PATH");
    }

    let result = resolve_init_binary();

    unsafe {
        if let Some(ref home) = original_home {
            std::env::set_var("HOME", home);
        }
    }

    assert!(
        result.is_some(),
        "expected Some when iii-init exists in ~/.iii/lib/"
    );
    assert_paths_eq(&result.unwrap(), &init_path);
}

/// FW-01: resolve_libkrunfw_dir respects III_LIBKRUNFW_PATH env var override.
#[test]
fn resolve_libkrunfw_dir_env_var_override() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let filename = libkrunfw_filename();
    let file_path = tmp.path().join(&filename);
    std::fs::write(&file_path, b"fake-firmware").unwrap();

    let original_env = std::env::var("III_LIBKRUNFW_PATH").ok();
    unsafe {
        std::env::set_var("III_LIBKRUNFW_PATH", file_path.to_str().unwrap());
    }

    let result = resolve_libkrunfw_dir();

    unsafe {
        match original_env {
            Some(ref v) => std::env::set_var("III_LIBKRUNFW_PATH", v),
            None => std::env::remove_var("III_LIBKRUNFW_PATH"),
        }
    }

    assert!(
        result.is_some(),
        "expected Some with III_LIBKRUNFW_PATH override"
    );
    assert_paths_eq(&result.unwrap(), tmp.path());
}

// ---------------------------------------------------------------------------
// Group 2: Embedded fallback (FW-02)
// ---------------------------------------------------------------------------

/// FW-02: Documents the embedded firmware fallback behavior.
///
/// The `resolve_libkrunfw_dir` function does NOT directly check for embedded firmware --
/// that's handled by `ensure_libkrunfw` in the download module. This test verifies
/// the behavior when no local firmware exists: the result depends on whether firmware
/// is present in system paths (/usr/lib, /usr/local/lib, Homebrew on macOS).
///
/// Full embedded/download fallback is tested at a higher level via `ensure_libkrunfw`
/// which is out of scope for this phase.
#[test]
fn resolve_libkrunfw_dir_embedded_fallback_conditional() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let original_home = std::env::var("HOME").ok();
    let original_env = std::env::var("III_LIBKRUNFW_PATH").ok();
    unsafe {
        std::env::set_var("HOME", tmp.path());
        std::env::remove_var("III_LIBKRUNFW_PATH");
    }

    let result = resolve_libkrunfw_dir();

    unsafe {
        if let Some(ref home) = original_home {
            std::env::set_var("HOME", home);
        }
        match original_env {
            Some(ref v) => std::env::set_var("III_LIBKRUNFW_PATH", v),
            None => std::env::remove_var("III_LIBKRUNFW_PATH"),
        }
    }

    // The result depends on the host system:
    // - Some(path): system paths have libkrunfw installed (e.g., /usr/lib, Homebrew)
    // - None: no system-level libkrunfw (typical in CI/dev without VM support)
    match result {
        Some(path) => {
            let path_str = path.display().to_string();
            assert!(
                path_str.starts_with("/usr/") || path_str.starts_with("/opt/"),
                "expected system path, got: {path_str}"
            );
        }
        None => {
            // Expected when no system-level libkrunfw is installed
        }
    }
}

// ---------------------------------------------------------------------------
// Group 3: Download fallback URL construction (FW-03)
// ---------------------------------------------------------------------------

/// FW-03: libkrunfw archive name contains correct platform identifier.
#[test]
fn libkrunfw_archive_name_contains_platform() {
    let name = libkrunfw_archive_name();
    if cfg!(target_os = "macos") {
        assert!(
            name.contains("darwin"),
            "macOS archive should contain 'darwin': {name}"
        );
    } else {
        assert!(
            name.contains("linux"),
            "Linux archive should contain 'linux': {name}"
        );
    }
    // Archive name does not contain the version string directly -- it uses os+arch only.
    // Verify it's a valid .tar.gz archive name.
    assert!(
        name.ends_with(".tar.gz"),
        "archive should end with '.tar.gz': {name}"
    );
}

/// FW-03: iii-init archive name contains musl target identifier.
#[test]
fn iii_init_archive_name_contains_musl_target() {
    let name = iii_init_archive_name();
    assert!(
        name.contains("iii-init"),
        "archive name should contain 'iii-init': {name}"
    );
    assert!(
        name.contains("musl"),
        "archive name should contain 'musl' (init binary is always musl-linked): {name}"
    );
}

/// FW-03: Platform support check succeeds on supported architectures.
#[test]
fn check_libkrunfw_platform_support_succeeds_on_supported() {
    let result = check_libkrunfw_platform_support();
    // Available firmware: darwin-aarch64, linux-x86_64, linux-aarch64
    // Missing: darwin-x86_64 (Intel Mac)
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    assert!(
        result.is_ok(),
        "darwin-aarch64 should be supported: {result:?}"
    );

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    assert!(
        result.is_ok(),
        "linux-x86_64 should be supported: {result:?}"
    );

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    assert!(
        result.is_ok(),
        "linux-aarch64 should be supported: {result:?}"
    );

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        assert!(result.is_err(), "darwin-x86_64 should NOT be supported");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("Intel Mac"),
            "error message should mention Intel Mac: {msg}"
        );
    }
}

// ---------------------------------------------------------------------------
// Group 4: All sources fail (FW-04)
// ---------------------------------------------------------------------------

/// FW-04: resolve_libkrunfw_dir returns None (or system path) when env var and HOME fail.
///
/// The function checks: env var (nonexistent file), ~/.iii/lib/ (empty tempdir),
/// adjacent to binary (in cargo target dir, skipped), system paths.
/// If system paths don't have libkrunfw (typical in CI/dev), result is None.
/// If system has it installed, result is Some pointing to a system path.
#[test]
fn resolve_libkrunfw_dir_returns_none_when_all_sources_fail() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let original_home = std::env::var("HOME").ok();
    let original_env = std::env::var("III_LIBKRUNFW_PATH").ok();
    unsafe {
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("III_LIBKRUNFW_PATH", "/nonexistent/libkrunfw");
    }

    let result = resolve_libkrunfw_dir();

    unsafe {
        if let Some(ref home) = original_home {
            std::env::set_var("HOME", home);
        }
        match original_env {
            Some(ref v) => std::env::set_var("III_LIBKRUNFW_PATH", v),
            None => std::env::remove_var("III_LIBKRUNFW_PATH"),
        }
    }

    // System path caveat: if system has libkrunfw installed, we get Some.
    match result {
        None => {} // Expected in most CI/dev environments
        Some(path) => {
            let path_str = path.display().to_string();
            assert!(
                path_str.starts_with("/usr/") || path_str.starts_with("/opt/"),
                "if found, should be a system path, got: {path_str}"
            );
        }
    }
}

/// FW-04: resolve_init_binary returns None when all sources fail.
#[test]
fn resolve_init_binary_returns_none_when_all_sources_fail() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let original_home = std::env::var("HOME").ok();
    let original_env = std::env::var("III_INIT_PATH").ok();
    unsafe {
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("III_INIT_PATH", "/nonexistent/iii-init");
    }

    let result = resolve_init_binary();

    unsafe {
        if let Some(ref home) = original_home {
            std::env::set_var("HOME", home);
        }
        match original_env {
            Some(ref v) => std::env::set_var("III_INIT_PATH", v),
            None => std::env::remove_var("III_INIT_PATH"),
        }
    }

    assert!(
        result.is_none(),
        "expected None when all iii-init sources fail, got: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// Group 5: Constants verification (supporting tests)
// ---------------------------------------------------------------------------

/// lib_path_env_var returns the correct platform-specific variable name.
#[test]
fn lib_path_env_var_is_platform_correct() {
    let var = lib_path_env_var();
    if cfg!(target_os = "macos") {
        assert_eq!(var, "DYLD_LIBRARY_PATH");
    } else {
        assert_eq!(var, "LD_LIBRARY_PATH");
    }
}

/// libkrunfw_filename returns the correct platform-specific library filename.
#[test]
fn libkrunfw_filename_is_platform_correct() {
    let name = libkrunfw_filename();
    assert!(
        name.contains("libkrunfw"),
        "filename should contain 'libkrunfw': {name}"
    );
    if cfg!(target_os = "macos") {
        assert!(
            name.ends_with(".dylib"),
            "macOS should end with '.dylib': {name}"
        );
    } else {
        // Linux: libkrunfw.so.5.2.1 -- ends with a version number
        assert!(name.contains(".so."), "Linux should contain '.so.': {name}");
    }
}
