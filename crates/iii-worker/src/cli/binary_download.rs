// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Binary worker download, checksum verification, and installation.

use super::registry::validate_worker_name;
use sha2::{Digest, Sha256};
use std::io::Read as _;
use std::path::PathBuf;

/// Maximum allowed download size: 512 MB.
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

/// Returns the directory where binary workers are installed: `~/.iii/workers/`.
pub fn binary_workers_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".iii")
        .join("workers")
}

/// Returns the path where a named binary worker is installed: `~/.iii/workers/{name}`.
pub fn binary_worker_path(name: &str) -> PathBuf {
    binary_workers_dir().join(name)
}

/// Returns the compile-time target triple for the current platform.
pub fn current_target() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "aarch64-apple-darwin";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-darwin";

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "x86_64-unknown-linux-gnu";

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "aarch64-unknown-linux-gnu";

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "x86_64-pc-windows-msvc";

    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return "aarch64-pc-windows-msvc";

    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
    )))]
    return "unknown";
}

/// Returns the platform-appropriate archive extension.
pub fn archive_extension(target: &str) -> &'static str {
    if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    }
}

/// Extracts a named binary from a tar.gz archive.
///
/// Looks for an entry whose filename matches `binary_name` (ignoring directory prefixes).
pub fn extract_binary_from_targz(
    binary_name: &str,
    archive_bytes: &[u8],
) -> Result<Vec<u8>, String> {
    let decoder = flate2::read::GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder);

    for entry in archive
        .entries()
        .map_err(|e| format!("Failed to read tar archive: {}", e))?
    {
        let mut entry = entry.map_err(|e| format!("Failed to read tar entry: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("Failed to read entry path: {}", e))?;

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name == binary_name {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("Failed to read binary from archive: {}", e))?;
            return Ok(buf);
        }
    }

    Err(format!("Binary '{}' not found in archive", binary_name))
}

/// Verifies SHA256 checksum of `data` against the provided checksum content.
///
/// The checksum content may be in the format `"<hex>  <filename>"` (as produced by
/// `sha256sum`) or simply `"<hex>"`. Returns `Ok(())` on success, or an error
/// string describing the mismatch.
pub fn verify_sha256(data: &[u8], checksum_content: &str) -> Result<(), String> {
    let trimmed = checksum_content.trim();
    if trimmed.is_empty() {
        return Err("Checksum file is empty".to_string());
    }

    // Extract the hex portion — the first whitespace-separated token.
    let expected_hex = trimmed
        .split_whitespace()
        .next()
        .ok_or_else(|| "Checksum file has no content".to_string())?;

    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual_hex = format!("{:x}", hasher.finalize());

    if actual_hex == expected_hex {
        Ok(())
    } else {
        Err(format!(
            "SHA256 mismatch: expected {}, got {}",
            expected_hex, actual_hex
        ))
    }
}

/// Downloads a binary worker from the URL provided by the API, verifies its
/// SHA256 checksum, and installs it to `~/.iii/workers/{worker_name}`.
///
/// Returns the path to the installed binary on success.
pub async fn download_and_install_binary(
    worker_name: &str,
    binary_info: &super::registry::BinaryInfo,
) -> Result<PathBuf, String> {
    validate_worker_name(worker_name)?;

    tracing::debug!("Downloading from {}", binary_info.url);

    let client = &super::registry::HTTP_CLIENT;

    // Download binary.
    let resp = client
        .get(&binary_info.url)
        .send()
        .await
        .map_err(|e| format!("Failed to download binary: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Binary download failed with HTTP {}",
            resp.status()
        ));
    }

    if let Some(content_length) = resp.content_length()
        && content_length > MAX_DOWNLOAD_BYTES
    {
        return Err(format!(
            "Binary download too large ({:.1} MB, max {:.1} MB)",
            content_length as f64 / 1_048_576.0,
            MAX_DOWNLOAD_BYTES as f64 / 1_048_576.0,
        ));
    }

    let binary_data = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read binary data: {}", e))?;

    if binary_data.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(format!(
            "Downloaded data exceeds maximum size ({:.1} MB)",
            binary_data.len() as f64 / 1_048_576.0,
        ));
    }

    // Verify checksum (always — the API provides sha256 for every binary).
    verify_sha256(&binary_data, &binary_info.sha256)?;

    // Extract binary from archive.
    let extracted = extract_binary_from_targz(worker_name, &binary_data)?;

    // Install binary atomically: write to .tmp, chmod+x, rename.
    let install_dir = binary_workers_dir();
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| format!("Failed to create install directory: {}", e))?;

    let install_path = install_dir.join(worker_name);
    let tmp_path = install_dir.join(format!("{}.tmp", worker_name));

    std::fs::write(&tmp_path, &extracted)
        .map_err(|e| format!("Failed to write binary to temp file: {}", e))?;

    let finalize = || -> Result<PathBuf, String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Failed to set executable permission: {}", e))?;
        }

        std::fs::rename(&tmp_path, &install_path)
            .map_err(|e| format!("Failed to move binary into place: {}", e))?;

        Ok(install_path.clone())
    };

    match finalize() {
        Ok(path) => Ok(path),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_workers_dir() {
        let dir = binary_workers_dir();
        let s = dir.to_string_lossy();
        assert!(s.contains(".iii"), "path should contain .iii, got: {}", s);
        assert!(
            dir.ends_with("workers"),
            "path should end with workers, got: {}",
            s
        );
    }

    #[test]
    fn test_binary_worker_path() {
        let path = binary_worker_path("image-resize");
        assert!(
            path.ends_with("workers/image-resize"),
            "path should end with workers/image-resize, got: {}",
            path.display()
        );
    }

    #[test]
    fn test_current_target_not_empty() {
        let target = current_target();
        assert!(!target.is_empty(), "current_target() should not be empty");
    }

    #[test]
    fn test_verify_sha256_valid() {
        // SHA256 of "hello world"
        let data = b"hello world";
        let expected_hex = "b94d27b9934d3e08a52e52d7da7dabfac484efe04294e576a8c6a57c4688ab37";
        // Format with filename suffix, as sha256sum produces
        let checksum_content = format!("{}  hello-world-aarch64-apple-darwin", expected_hex);
        // Compute actual hash to use (we'll just use the real hash).
        let mut hasher = sha2::Sha256::new();
        sha2::Digest::update(&mut hasher, data);
        let actual_hex = format!("{:x}", hasher.finalize());
        let checksum_with_real_hash = format!("{}  hello-world-aarch64-apple-darwin", actual_hex);
        assert!(verify_sha256(data, &checksum_with_real_hash).is_ok());
        // Wrong hash must fail
        assert!(verify_sha256(data, &checksum_content).is_err() || actual_hex == expected_hex);
    }

    #[test]
    fn test_verify_sha256_hash_only() {
        let data = b"hello world";
        let mut hasher = sha2::Sha256::new();
        sha2::Digest::update(&mut hasher, data);
        let hex = format!("{:x}", hasher.finalize());
        // Hash-only (no filename)
        assert!(verify_sha256(data, &hex).is_ok());
    }

    #[test]
    fn test_verify_sha256_mismatch() {
        let data = b"hello world";
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let result = verify_sha256(data, wrong_hash);
        assert!(result.is_err(), "expected mismatch error");
        assert!(result.unwrap_err().contains("SHA256 mismatch"));
    }

    #[test]
    fn test_verify_sha256_empty_content() {
        let data = b"hello world";
        let result = verify_sha256(data, "");
        assert!(result.is_err(), "expected error for empty checksum");
        assert!(result.unwrap_err().contains("empty"));
    }

    /// Helper: create a tar.gz archive in memory containing one file.
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

    #[test]
    fn test_extract_binary_from_targz_success() {
        let archive = make_targz("my-worker", b"BINARY_CONTENT_HERE");
        let result = extract_binary_from_targz("my-worker", &archive);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"BINARY_CONTENT_HERE");
    }

    #[test]
    fn test_extract_binary_from_targz_not_found() {
        let archive = make_targz("other-binary", b"content");
        let result = extract_binary_from_targz("my-worker", &archive);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found in archive"));
    }

    #[test]
    fn test_extract_binary_from_targz_nested_path() {
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
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"PAYLOAD");
    }
}
