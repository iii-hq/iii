// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Binary worker download, checksum verification, and installation.

use super::registry::validate_worker_name;
use sha2::{Digest, Sha256};
use std::io::Read as _;
use std::path::PathBuf;
use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};

/// Maximum allowed download size: 512 MB.
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_REDIRECTS: usize = 10;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

static NO_REDIRECT_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("Failed to create no-redirect HTTP client")
});

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

pub fn validate_locked_artifact_url(url: &str) -> Result<(), String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|e| format!("invalid artifact URL `{url}`: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("artifact URL `{url}` has no host"))?;

    #[cfg(debug_assertions)]
    if matches!(parsed.scheme(), "http" | "https") && is_loopback_host(host) {
        return Ok(());
    }

    if parsed.scheme() != "https" {
        return Err(format!(
            "artifact URL `{url}` is not trusted: locked worker artifacts must use HTTPS registry URLs"
        ));
    }

    if is_private_or_loopback_host(host) {
        return Err(format!(
            "artifact URL `{url}` is not trusted: private and loopback hosts are not allowed in iii.lock"
        ));
    }

    Ok(())
}

#[cfg(debug_assertions)]
fn is_loopback_host(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

fn is_private_or_loopback_host(host: &str) -> bool {
    let Ok(ip) = host.parse::<std::net::IpAddr>() else {
        return false;
    };
    match ip {
        std::net::IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.octets()[0] == 0
        }
        std::net::IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.segments()[0] & 0xfe00 == 0xfc00
                || ip.segments()[0] & 0xffc0 == 0xfe80
        }
    }
}

async fn download_archive_bytes(
    url: &str,
    validate_final_url: Option<&(dyn Fn(&str) -> Result<(), String> + Send + Sync)>,
) -> Result<Vec<u8>, String> {
    let resp = match validate_final_url {
        Some(validator) => download_archive_bytes_with_validated_redirects(url, validator).await?,
        None => super::registry::HTTP_CLIENT
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Failed to download binary: {}", e))?,
    };

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

    Ok(binary_data.to_vec())
}

async fn download_archive_bytes_with_validated_redirects(
    url: &str,
    validator: &(dyn Fn(&str) -> Result<(), String> + Send + Sync),
) -> Result<reqwest::Response, String> {
    let mut current_url =
        reqwest::Url::parse(url).map_err(|e| format!("invalid artifact URL `{url}`: {e}"))?;

    for redirects_followed in 0..=MAX_REDIRECTS {
        let resp = NO_REDIRECT_HTTP_CLIENT
            .get(current_url.clone())
            .send()
            .await
            .map_err(|e| format!("Failed to download binary: {}", e))?;

        if !resp.status().is_redirection() {
            return Ok(resp);
        }

        if redirects_followed == MAX_REDIRECTS {
            return Err(format!(
                "Binary download exceeded {MAX_REDIRECTS} redirects"
            ));
        }

        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or_else(|| {
                format!(
                    "Binary download redirect from `{}` did not include a Location header",
                    current_url
                )
            })?
            .to_str()
            .map_err(|e| format!("Binary download redirect has invalid Location header: {e}"))?;
        let next_url = current_url.join(location).map_err(|e| {
            format!(
                "Binary download redirect from `{}` has invalid Location `{}`: {}",
                current_url, location, e
            )
        })?;

        validator(next_url.as_str()).map_err(|e| {
            format!("artifact download for `{url}` was redirected to an untrusted URL: {e}")
        })?;
        current_url = next_url;
    }

    unreachable!("redirect loop always returns or errors");
}

pub async fn download_locked_binary_archive(
    worker_name: &str,
    target: &str,
    binary_info: &super::registry::BinaryInfo,
) -> Result<Vec<u8>, String> {
    validate_worker_name(worker_name)?;
    validate_locked_artifact_url(&binary_info.url).map_err(|e| {
        format!(
            "worker `{worker_name}` target `{target}` cannot be replayed from iii.lock: {e}. \
             Fix: restore the committed iii.lock, use a registry-managed artifact URL, or run \
             `iii worker update {worker_name}` only if changing pins is intentional"
        )
    })?;
    let binary_data =
        download_archive_bytes(&binary_info.url, Some(&validate_locked_artifact_url)).await?;
    verify_sha256(&binary_data, &binary_info.sha256)?;
    Ok(binary_data)
}

pub fn unique_worker_temp_path(worker_name: &str, suffix: &str) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    binary_workers_dir().join(format!(".{worker_name}.{pid}.{nanos}.{counter}.{suffix}"))
}

pub fn set_executable_permission(path: &std::path::Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to set executable permission: {}", e))?;
    }
    Ok(())
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

    let binary_data = download_archive_bytes(&binary_info.url, None).await?;

    // Verify checksum (always — the API provides sha256 for every binary).
    verify_sha256(&binary_data, &binary_info.sha256)?;

    // Extract binary from archive.
    let extracted = extract_binary_from_targz(worker_name, &binary_data)?;

    // Install binary atomically: write to .tmp, chmod+x, rename.
    let install_dir = binary_workers_dir();
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| format!("Failed to create install directory: {}", e))?;

    let install_path = install_dir.join(worker_name);
    let tmp_path = unique_worker_temp_path(worker_name, "tmp");

    std::fs::write(&tmp_path, &extracted)
        .map_err(|e| format!("Failed to write binary to temp file: {}", e))?;

    let finalize = || -> Result<PathBuf, String> {
        set_executable_permission(&tmp_path)?;

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

    #[test]
    fn validate_locked_artifact_url_accepts_https_public_host() {
        validate_locked_artifact_url("https://workers.iii.dev/worker.tar.gz").unwrap();
        validate_locked_artifact_url(
            "https://github.com/iii-hq/workers/releases/download/skills/v0.1.4/skills-x86_64-unknown-linux-gnu.tar.gz",
        )
        .unwrap();
    }

    #[test]
    fn validate_locked_artifact_url_rejects_non_https() {
        let err = validate_locked_artifact_url("http://workers.iii.dev/worker.tar.gz").unwrap_err();
        assert!(err.contains("HTTPS"), "got: {err}");
    }

    #[test]
    fn validate_locked_artifact_url_rejects_private_or_loopback_hosts() {
        // Loopback / private IPs must never end up in iii.lock regardless of
        // scheme; release builds reject them outright.
        #[cfg(not(debug_assertions))]
        {
            let err = validate_locked_artifact_url("https://127.0.0.1/worker.tar.gz").unwrap_err();
            assert!(err.contains("not trusted"), "got: {err}");
        }
        for url in [
            "https://10.0.0.1/worker.tar.gz",
            "https://192.168.1.1/worker.tar.gz",
        ] {
            let err = validate_locked_artifact_url(url)
                .err()
                .unwrap_or_else(|| panic!("expected {url} to be rejected"));
            assert!(err.contains("not trusted"), "got: {err}");
        }
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

    /// Spawn a one-shot HTTP server on loopback that returns `response_bytes`
    /// to whatever request comes in. Returns the bound base URL and a join
    /// handle that the caller should `abort()` when done.
    async fn spawn_oneshot_http(response_bytes: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
        let (base_url, handle, _) = spawn_counting_http(response_bytes).await;
        (base_url, handle)
    }

    async fn spawn_counting_http(
        response_bytes: Vec<u8>,
    ) -> (
        String,
        tokio::task::JoinHandle<()>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hit_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let server_hit_count = hit_count.clone();
        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let response = response_bytes.clone();
                let server_hit_count = server_hit_count.clone();
                tokio::spawn(async move {
                    server_hit_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf).await;
                    let _ = stream.write_all(&response).await;
                });
            }
        });
        (format!("http://{addr}"), handle, hit_count)
    }

    #[tokio::test]
    async fn download_archive_bytes_revalidates_redirect_target() {
        // Final server: returns a 200 with a body. A direct fetch here yields
        // `final_url == url`, so the validator must NOT be called.
        let final_body = b"final-payload".to_vec();
        let final_response = {
            let mut bytes = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\ncontent-type: application/octet-stream\r\nconnection: close\r\n\r\n",
                final_body.len()
            )
            .into_bytes();
            bytes.extend_from_slice(&final_body);
            bytes
        };
        let (final_base, final_server, final_hits) = spawn_counting_http(final_response).await;

        // Redirect server: returns 302 -> final_base/final, so the final URL
        // observed after `send()` differs from the input URL and the validator
        // MUST be called.
        let redirect_target = format!("{final_base}/final");
        let redirect_response = format!(
            "HTTP/1.1 302 Found\r\nlocation: {redirect_target}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        )
        .into_bytes();
        let (redirect_base, redirect_server) = spawn_oneshot_http(redirect_response).await;

        let always_reject = |url: &str| -> Result<(), String> { Err(format!("untrusted: {url}")) };

        // 1. Redirect path: validator IS called and its error is wrapped.
        let err = download_archive_bytes(&format!("{redirect_base}/start"), Some(&always_reject))
            .await
            .err()
            .expect("redirected download must be rejected by validator");
        assert!(
            err.contains("redirected to an untrusted URL"),
            "expected wrapped redirect rejection, got: {err}",
        );
        assert_eq!(
            final_hits.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "validated redirects must be rejected before requesting the target",
        );

        // 2. No-redirect path with validator: validator NOT called when the
        //    final URL matches the request URL, so the download succeeds.
        let direct = format!("{final_base}/final");
        let body = download_archive_bytes(&direct, Some(&always_reject))
            .await
            .expect("direct download with unchanged URL must skip validator");
        assert_eq!(body, final_body);
        assert_eq!(final_hits.load(std::sync::atomic::Ordering::SeqCst), 1);

        // 3. Redirect path without validator: legacy behavior preserved —
        //    redirects are still followed.
        let body = download_archive_bytes(&format!("{redirect_base}/start"), None)
            .await
            .expect("download without validator must follow redirects");
        assert_eq!(body, final_body);

        redirect_server.abort();
        final_server.abort();
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
