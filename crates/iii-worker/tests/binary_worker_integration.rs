// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for binary worker download functions.
//! Covers requirements BIN-01 through BIN-04.

mod common;

use iii_worker::cli::binary_download::{
    archive_extension, binary_worker_path, binary_workers_dir, current_target,
    download_and_install_binary, extract_binary_from_targz, verify_sha256,
};
use iii_worker::cli::registry::BinaryInfo;
use sha2::{Digest, Sha256};
use std::sync::Mutex;

/// Serializes tests that mutate environment variables (HOME).
/// Each integration test file runs in its own process, so only intra-file locking is needed.
static ENV_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// Helper: create a tar.gz archive in memory containing one file.
// ---------------------------------------------------------------------------

fn make_targz(file_name: &str, content: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut archive = tar::Builder::new(&mut encoder);
        let mut header = tar::Header::new_gnu();
        header.set_path(file_name).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append(&header, content).unwrap();
        archive.finish().unwrap();
    }
    encoder.finish().unwrap()
}

// ===========================================================================
// Group 1: Architecture detection (BIN-01, BIN-02)
// ===========================================================================

/// BIN-01/BIN-02: current_target() returns a known platform triple.
#[test]
fn current_target_returns_known_triple() {
    let target = current_target();
    assert!(!target.is_empty(), "current_target() must not be empty");
    assert_ne!(target, "unknown", "current_target() must not be 'unknown'");

    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        assert_eq!(target, "aarch64-apple-darwin");
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        assert_eq!(target, "x86_64-apple-darwin");
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert_eq!(target, "x86_64-unknown-linux-gnu");
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        assert_eq!(target, "aarch64-unknown-linux-gnu");
    }
}

/// BIN-02: current_target() contains OS and architecture substrings.
#[test]
fn current_target_contains_os_and_arch() {
    let target = current_target();

    // Must contain the OS identifier
    let has_os = target.contains("apple-darwin") || target.contains("unknown-linux-gnu");
    assert!(
        has_os,
        "target '{}' should contain 'apple-darwin' or 'unknown-linux-gnu'",
        target
    );

    // Must contain the architecture identifier
    let has_arch = target.contains("x86_64") || target.contains("aarch64");
    assert!(
        has_arch,
        "target '{}' should contain 'x86_64' or 'aarch64'",
        target
    );
}

// ===========================================================================
// Group 2: URL construction (BIN-01)
// ===========================================================================

/// BIN-01: archive_extension returns "tar.gz" for non-Windows targets.
#[test]
fn archive_extension_non_windows() {
    assert_eq!(archive_extension("x86_64-unknown-linux-gnu"), "tar.gz");
    assert_eq!(archive_extension("aarch64-apple-darwin"), "tar.gz");
}

/// BIN-01: archive_extension returns "zip" for Windows targets.
#[test]
fn archive_extension_windows() {
    assert_eq!(archive_extension("x86_64-pc-windows-msvc"), "zip");
}

// ===========================================================================
// Group 3: Checksum verification (BIN-01 security, T-04-02 mitigation)
// ===========================================================================

/// BIN-01: verify_sha256 accepts correct hash-only format.
#[test]
fn verify_sha256_valid_hash_only() {
    let data = b"test data";
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hex_string = format!("{:x}", hasher.finalize());

    assert!(
        verify_sha256(data, &hex_string).is_ok(),
        "verify_sha256 should accept correct hash-only format"
    );
}

/// BIN-01: verify_sha256 accepts sha256sum format (hash + filename).
#[test]
fn verify_sha256_valid_sha256sum_format() {
    let data = b"test data";
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hex_string = format!("{:x}", hasher.finalize());
    let checksum_content = format!("{}  my-worker-aarch64-apple-darwin.tar.gz", hex_string);

    assert!(
        verify_sha256(data, &checksum_content).is_ok(),
        "verify_sha256 should accept sha256sum format"
    );
}

/// BIN-01 security (T-04-02): verify_sha256 rejects mismatched hash.
#[test]
fn verify_sha256_rejects_mismatch() {
    let data = b"test data";
    let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    let result = verify_sha256(data, wrong_hash);
    assert!(result.is_err(), "expected mismatch error");
    assert!(
        result.unwrap_err().contains("SHA256 mismatch"),
        "error should mention SHA256 mismatch"
    );
}

/// BIN-01 security (T-04-02): verify_sha256 rejects empty checksum content.
#[test]
fn verify_sha256_rejects_empty() {
    let data = b"test data";
    let result = verify_sha256(data, "");
    assert!(result.is_err(), "expected error for empty checksum");
    assert!(
        result.unwrap_err().contains("empty"),
        "error should mention 'empty'"
    );
}

// ===========================================================================
// Group 4: tar.gz extraction (BIN-01)
// ===========================================================================

/// BIN-01: extract_binary_from_targz finds binary by filename.
#[test]
fn extract_binary_from_targz_finds_by_name() {
    let archive = make_targz("my-worker", b"BINARY_PAYLOAD");
    let result = extract_binary_from_targz("my-worker", &archive);
    assert!(result.is_ok(), "should find binary in archive");
    assert_eq!(result.unwrap(), b"BINARY_PAYLOAD");
}

/// BIN-01: extract_binary_from_targz finds binary in nested path (ignores directory prefix).
#[test]
fn extract_binary_from_targz_nested_path() {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut archive = tar::Builder::new(&mut encoder);
        let mut header = tar::Header::new_gnu();
        header.set_path("release/my-worker").unwrap();
        header.set_size(7);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append(&header, b"PAYLOAD" as &[u8]).unwrap();
        archive.finish().unwrap();
    }
    let data = encoder.finish().unwrap();

    let result = extract_binary_from_targz("my-worker", &data);
    assert!(result.is_ok(), "should find nested binary by filename");
    assert_eq!(result.unwrap(), b"PAYLOAD");
}

/// BIN-01: extract_binary_from_targz returns error when binary not found.
#[test]
fn extract_binary_from_targz_not_found() {
    let archive = make_targz("other-binary", b"content");
    let result = extract_binary_from_targz("my-worker", &archive);
    assert!(result.is_err(), "should fail when binary not in archive");
    assert!(
        result.unwrap_err().contains("not found in archive"),
        "error should mention 'not found in archive'"
    );
}

// ===========================================================================
// Group 5: Path construction with HOME override (BIN-01, BIN-04)
// ===========================================================================

/// BIN-01/BIN-04: binary_workers_dir uses HOME env var for path construction.
///
/// Threat mitigation T-03-05: HOME is saved before override and restored
/// immediately after the function call, before any assertions.
#[test]
fn binary_workers_dir_uses_home() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let original_home = std::env::var("HOME").ok();
    // SAFETY: test-only, serialized via ENV_LOCK
    unsafe {
        std::env::set_var("HOME", tmp.path());
    }

    let result = binary_workers_dir();

    // Restore HOME immediately
    unsafe {
        if let Some(ref home) = original_home {
            std::env::set_var("HOME", home);
        }
    }

    assert!(
        result.starts_with(tmp.path()),
        "binary_workers_dir() should start with HOME ({}), got: {}",
        tmp.path().display(),
        result.display()
    );
    let result_str = result.to_string_lossy();
    assert!(
        result_str.contains(".iii"),
        "path should contain '.iii', got: {}",
        result_str
    );
    assert!(
        result.ends_with("workers"),
        "path should end with 'workers', got: {}",
        result.display()
    );
}

/// BIN-01/BIN-04: binary_worker_path appends worker name to base directory.
#[test]
fn binary_worker_path_appends_name() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let original_home = std::env::var("HOME").ok();
    // SAFETY: test-only, serialized via ENV_LOCK
    unsafe {
        std::env::set_var("HOME", tmp.path());
    }

    let result = binary_worker_path("image-resize");

    // Restore HOME immediately
    unsafe {
        if let Some(ref home) = original_home {
            std::env::set_var("HOME", home);
        }
    }

    assert!(
        result.ends_with("workers/image-resize"),
        "path should end with 'workers/image-resize', got: {}",
        result.display()
    );
}

// ===========================================================================
// Group 6: Executable permissions (BIN-03)
// ===========================================================================

/// BIN-03: Verify that executable permissions (0o755) can be set and read back.
#[cfg(unix)]
#[test]
fn executable_permissions_roundtrip() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let binary_path = tmp.path().join("test-binary");
    std::fs::write(&binary_path, b"fake binary content").unwrap();

    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&binary_path, perms).unwrap();

    let metadata = std::fs::metadata(&binary_path).unwrap();
    let mode = metadata.permissions().mode();
    assert_eq!(
        mode & 0o755,
        0o755,
        "expected 0o755 permission bits, got: 0o{:o}",
        mode
    );
}

// ===========================================================================
// Group 7: Early validation in download_and_install_binary (BIN-04, T-04-03)
// ===========================================================================

/// BIN-04 (T-04-03): download_and_install_binary rejects empty worker name.
#[tokio::test]
async fn download_rejects_invalid_worker_name() {
    let info = BinaryInfo {
        url: "https://example.com/fake.tar.gz".to_string(),
        sha256: "abc".to_string(),
    };
    let result = download_and_install_binary("", &info).await;
    assert!(result.is_err(), "empty worker name should be rejected");
}

/// BIN-04 (T-04-03): download_and_install_binary rejects path traversal in worker name.
#[tokio::test]
async fn download_rejects_path_traversal_name() {
    let info = BinaryInfo {
        url: "https://example.com/fake.tar.gz".to_string(),
        sha256: "abc".to_string(),
    };
    let result = download_and_install_binary("../evil", &info).await;
    assert!(
        result.is_err(),
        "path traversal in worker name should be rejected"
    );
}

// ===========================================================================
// Group 8: Platform selection from BinaryWorkerResponse (API migration)
// ===========================================================================

/// Verify that looking up a platform key in a BinaryWorkerResponse binaries map
/// works correctly when the platform exists.
#[test]
fn binary_response_platform_lookup_found() {
    use std::collections::HashMap;

    let mut binaries = HashMap::new();
    binaries.insert(
        "aarch64-apple-darwin".to_string(),
        BinaryInfo {
            url: "https://example.com/worker-aarch64-apple-darwin.tar.gz".to_string(),
            sha256: "abc123".to_string(),
        },
    );
    binaries.insert(
        "x86_64-unknown-linux-gnu".to_string(),
        BinaryInfo {
            url: "https://example.com/worker-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            sha256: "def456".to_string(),
        },
    );

    let target = current_target();
    // On macOS aarch64 or Linux x86_64 this should find a match
    if target == "aarch64-apple-darwin" || target == "x86_64-unknown-linux-gnu" {
        let info = binaries.get(target).unwrap();
        assert!(!info.url.is_empty());
        assert!(!info.sha256.is_empty());
    }
}

/// Verify that looking up a non-existent platform key returns None.
#[test]
fn binary_response_platform_lookup_not_found() {
    use std::collections::HashMap;

    let mut binaries = HashMap::new();
    binaries.insert(
        "riscv64-unknown-linux-gnu".to_string(),
        BinaryInfo {
            url: "https://example.com/worker-riscv64.tar.gz".to_string(),
            sha256: "abc".to_string(),
        },
    );

    let target = current_target();
    assert!(
        binaries.get(target).is_none(),
        "current target '{}' should not be riscv64",
        target
    );
}

/// Verify that empty binaries map returns None for any platform.
#[test]
fn binary_response_empty_binaries_map() {
    use std::collections::HashMap;
    let binaries: HashMap<String, BinaryInfo> = HashMap::new();
    let target = current_target();
    assert!(binaries.get(target).is_none());
}

// ===========================================================================
// Group 9: Checksum verification with inline sha256 (API migration)
// ===========================================================================

/// Verify that verify_sha256 works with the inline sha256 string format
/// that the new API provides (just a hex string, no filename suffix).
#[test]
fn verify_sha256_inline_api_format() {
    use sha2::{Digest, Sha256};
    let data = b"binary content from API download";
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hex = format!("{:x}", hasher.finalize());

    // The API provides just the hex string — verify this works
    assert!(verify_sha256(data, &hex).is_ok());
}

/// Verify that checksum mismatch is detected with inline format.
#[test]
fn verify_sha256_inline_api_format_mismatch() {
    let data = b"binary content";
    let wrong_hex = "0000000000000000000000000000000000000000000000000000000000000000";
    let result = verify_sha256(data, wrong_hex);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("SHA256 mismatch"));
}
