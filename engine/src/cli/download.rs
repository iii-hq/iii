// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::path::Path;

use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};

use super::error::{DownloadError, ExtractError};
use super::github::ReleaseAsset;
use super::registry::BinarySpec;

/// Download an asset with a progress bar, verify checksum if available,
/// and extract the binary to the target path using atomic write.
pub async fn download_and_install(
    client: &reqwest::Client,
    spec: &BinarySpec,
    asset: &ReleaseAsset,
    checksum_url: Option<&str>,
    target_path: &Path,
) -> Result<(), DownloadAndInstallError> {
    // Download the asset with progress
    let archive_bytes =
        download_with_progress(client, &asset.browser_download_url, asset.size).await?;

    // Verify checksum if available
    if let Some(checksum_url) = checksum_url {
        verify_checksum(client, checksum_url, &archive_bytes, &asset.name).await?;
    } else {
        eprintln!(
            "  {} Checksum not available for {}, skipping verification",
            colored::Colorize::yellow("warning:"),
            spec.name
        );
    }

    // Extract binary from archive
    let binary_bytes = extract_binary(spec.name, &archive_bytes)?;

    // Atomic write: write to temp file, then rename
    atomic_write_binary(&binary_bytes, target_path)?;

    Ok(())
}

/// Download a file with a progress bar showing download progress.
async fn download_with_progress(
    client: &reqwest::Client,
    url: &str,
    total_size: u64,
) -> Result<Vec<u8>, DownloadError> {
    let response = client.get(url).send().await?;

    let total = if total_size > 0 {
        total_size
    } else {
        response.content_length().unwrap_or(0)
    };

    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template(
            "  [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .unwrap()
        .progress_chars("=> "),
    );

    let mut bytes = Vec::with_capacity(total as usize);
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        bytes.extend_from_slice(&chunk);
        pb.set_position(bytes.len() as u64);
    }

    pb.finish_and_clear();
    Ok(bytes)
}

/// Verify SHA256 checksum against a sidecar file.
async fn verify_checksum(
    client: &reqwest::Client,
    checksum_url: &str,
    data: &[u8],
    asset_name: &str,
) -> Result<(), DownloadError> {
    let checksum_response = client.get(checksum_url).send().await?;
    let checksum_text = checksum_response
        .text()
        .await
        .map_err(|e| DownloadError::Failed(format!("Failed to read checksum: {}", e)))?;

    // Checksum file format: "hash  filename" or just "hash"
    let expected = checksum_text
        .split_whitespace()
        .next()
        .ok_or_else(|| DownloadError::Failed("Empty checksum file".to_string()))?
        .to_lowercase();

    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual = format!("{:x}", hasher.finalize());

    if actual != expected {
        return Err(DownloadError::ChecksumMismatch {
            asset: asset_name.to_string(),
            expected,
            actual,
        });
    }

    Ok(())
}

/// Extract a binary from a tar.gz archive.
fn extract_binary(binary_name: &str, archive_bytes: &[u8]) -> Result<Vec<u8>, ExtractError> {
    #[cfg(not(target_os = "windows"))]
    {
        extract_from_targz(binary_name, archive_bytes)
    }
    #[cfg(target_os = "windows")]
    {
        extract_from_zip(binary_name, archive_bytes)
    }
}

/// Extract a binary from a tar.gz archive.
#[cfg(not(target_os = "windows"))]
fn extract_from_targz(binary_name: &str, archive_bytes: &[u8]) -> Result<Vec<u8>, ExtractError> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    use tar::Archive;

    let decoder = GzDecoder::new(archive_bytes);
    let mut archive = Archive::new(decoder);

    let exe_name = binary_name;

    for entry in archive
        .entries()
        .map_err(|e| ExtractError::ExtractionFailed(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| ExtractError::ExtractionFailed(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| ExtractError::ExtractionFailed(e.to_string()))?;

        // The binary may be at the root or in a subdirectory
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if file_name == exe_name {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| ExtractError::ExtractionFailed(e.to_string()))?;
            return Ok(buf);
        }
    }

    Err(ExtractError::ExtractionFailed(format!(
        "Binary '{}' not found in archive",
        binary_name
    )))
}

/// Extract a binary from a zip archive (Windows).
#[cfg(any(target_os = "windows", test))]
fn extract_from_zip(binary_name: &str, archive_bytes: &[u8]) -> Result<Vec<u8>, ExtractError> {
    use std::io::{Cursor, Read};

    let reader = Cursor::new(archive_bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| ExtractError::ExtractionFailed(e.to_string()))?;
    // Validate every entry before returning any installable binary bytes.
    archive
        .validate_header_consistency()
        .map_err(|e| ExtractError::ExtractionFailed(e.to_string()))?;

    let exe_name = format!("{}.exe", binary_name);

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| ExtractError::ExtractionFailed(e.to_string()))?;

        let name = file
            .name()
            .map_err(|e| ExtractError::ExtractionFailed(e.to_string()))?
            .into_owned();
        let file_name = std::path::Path::new(&name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if file_name == exe_name || file_name == binary_name {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| ExtractError::ExtractionFailed(e.to_string()))?;
            return Ok(buf);
        }
    }

    Err(ExtractError::ExtractionFailed(format!(
        "Binary '{}' not found in archive",
        binary_name
    )))
}

/// Atomically write binary data to the target path.
/// Writes to a temp file in the same directory, then renames.
fn atomic_write_binary(data: &[u8], target_path: &Path) -> Result<(), ExtractError> {
    use std::io::Write;

    // Ensure parent directory exists
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let temp_path = target_path.with_extension("tmp");

    // Write to temp file
    let mut file = std::fs::File::create(&temp_path)?;
    file.write_all(data)?;
    file.flush()?;
    drop(file);

    // Set executable permission on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // Atomic rename
    std::fs::rename(&temp_path, target_path).map_err(|e| {
        let _ = std::fs::remove_file(&temp_path);
        ExtractError::Io(e)
    })?;

    Ok(())
}

/// Error type combining download and extraction errors.
#[derive(Debug, thiserror::Error)]
pub enum DownloadAndInstallError {
    #[error(transparent)]
    Download(#[from] DownloadError),

    #[error(transparent)]
    Extract(#[from] ExtractError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in entries {
            archive
                .start_file(
                    *name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            archive.write_all(body).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn zip_windows_reader_accepts_root_nested_and_extensionless_binaries() {
        for name in ["iii.exe", "package/iii.exe", "iii"] {
            assert_eq!(
                extract_from_zip("iii", &zip_entries(&[(name, b"binary")])).unwrap(),
                b"binary"
            );
        }
    }

    #[test]
    fn zip_windows_reader_rejects_later_header_mismatch_before_returning_binary() {
        let mut bytes = zip_entries(&[("iii.exe", b"binary"), ("safe.txt", b"other")]);
        let local = bytes
            .windows(4)
            .enumerate()
            .filter(|(_, b)| *b == b"PK\x03\x04")
            .nth(1)
            .unwrap()
            .0;
        bytes[local + 30..local + 38].copy_from_slice(b"evil.txt");
        let error = extract_from_zip("iii", &bytes).unwrap_err();
        assert!(
            error.to_string().contains("header inconsistency"),
            "{error}"
        );
    }

    #[test]
    fn zip_windows_reader_rejects_corrupted_payload_crc() {
        let mut bytes = zip_entries(&[("iii.exe", b"binary")]);
        let index = bytes.windows(6).position(|b| b == b"binary").unwrap();
        bytes[index] ^= 1;
        assert!(extract_from_zip("iii", &bytes).is_err());
    }

    #[test]
    fn test_checksum_computation() {
        let data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
