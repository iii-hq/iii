//! Lazy provisioning of libkrunfw and iii-init binaries.
//!
//! Both follow the same three-stage resolution:
//! 1. Check local filesystem (env var, `~/.iii/lib/`, adjacent to binary)
//! 2. Extract from embedded bytes (compile-time features)
//! 3. Download from GitHub release assets on first use

use std::path::PathBuf;

use super::constants::{III_INIT_FILENAME, iii_init_archive_name};
use super::constants::{
    LIBKRUNFW_VERSION, check_libkrunfw_platform_support, libkrunfw_archive_name,
    libkrunfw_filename, libkrunfw_firmware_filename,
};
use super::resolve::{resolve_init_binary, resolve_libkrunfw_dir};
use super::symlinks;

/// Ensure libkrunfw is available.
///
/// Resolution order:
/// 1. Check local resolution chain (env var, `~/.iii/lib/`, adjacent to binary, system paths)
/// 2. Extract from embedded bytes (compile-time `embed-libkrunfw` feature)
/// 3. Download from GitHub releases on first use
pub async fn ensure_libkrunfw() -> anyhow::Result<PathBuf> {
    if let Some(dir) = resolve_libkrunfw_dir() {
        tracing::debug!(dir = %dir.display(), "libkrunfw found via resolution chain");
        return Ok(dir);
    }

    let bytes = super::libkrunfw_bytes::LIBKRUNFW_BYTES;
    if !bytes.is_empty() {
        let lib_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
            .join(".iii")
            .join("lib");
        tokio::fs::create_dir_all(&lib_dir).await?;

        let filename = libkrunfw_filename();
        let dest = lib_dir.join(&filename);

        tracing::info!("extracting embedded libkrunfw to {}", dest.display());
        tokio::fs::write(&dest, bytes).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
        }

        #[cfg(unix)]
        symlinks::create_libkrunfw_symlinks(&lib_dir);

        eprintln!(
            "  {} libkrunfw v{} extracted to {}",
            "\u{2713}",
            LIBKRUNFW_VERSION,
            lib_dir.display()
        );

        return Ok(lib_dir);
    }

    // Neither found locally nor embedded -- check platform support before downloading
    check_libkrunfw_platform_support().map_err(|msg| anyhow::anyhow!(msg))?;

    eprintln!("  Downloading libkrunfw v{}...", LIBKRUNFW_VERSION);
    download_libkrunfw().await
}

/// Download the libkrunfw firmware from the matching GitHub release.
async fn download_libkrunfw() -> anyhow::Result<PathBuf> {
    use flate2::read::GzDecoder;
    use futures::StreamExt;
    use std::io::Read;
    use tar::Archive;

    let version = env!("CARGO_PKG_VERSION");
    let archive_name = libkrunfw_archive_name();
    let firmware_filename = libkrunfw_firmware_filename();

    let release_tag = format!("iii/v{version}");
    let url =
        format!("https://github.com/iii-hq/iii/releases/download/{release_tag}/{archive_name}");

    tracing::info!(%url, "downloading libkrunfw from GitHub release");

    let client = reqwest::Client::builder()
        .user_agent(format!("iii-worker/{version}"))
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let request = if let Ok(token) =
        std::env::var("III_GITHUB_TOKEN").or_else(|_| std::env::var("GITHUB_TOKEN"))
    {
        client
            .get(&url)
            .header("Authorization", format!("token {token}"))
    } else {
        client.get(&url)
    };

    let response = request.send().await?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to download libkrunfw: HTTP {} from {}\n\
             Hint: Set III_LIBKRUNFW_PATH to a local firmware file, or rebuild with:\n  \
             cargo build -p iii-worker --features embed-libkrunfw --release",
            response.status(),
            url,
        );
    }

    let total_size = response.content_length().unwrap_or(0);
    let pb = indicatif::ProgressBar::new(total_size);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "  [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .unwrap()
        .progress_chars("=> "),
    );

    let mut bytes = Vec::with_capacity(total_size as usize);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        bytes.extend_from_slice(&chunk);
        pb.set_position(bytes.len() as u64);
    }
    pb.finish_and_clear();

    // Extract firmware from the tar.gz archive
    let decoder = GzDecoder::new(bytes.as_slice());
    let mut archive = Archive::new(decoder);
    let mut fw_bytes = None;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name == firmware_filename {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            fw_bytes = Some(buf);
            break;
        }
    }

    let fw_bytes = fw_bytes.ok_or_else(|| {
        anyhow::anyhow!(
            "'{}' not found in downloaded archive '{}'",
            firmware_filename,
            archive_name,
        )
    })?;

    // Write to ~/.iii/lib/{soname} with atomic rename
    let lib_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".iii")
        .join("lib");
    tokio::fs::create_dir_all(&lib_dir).await?;

    let soname = libkrunfw_filename();
    let dest = lib_dir.join(&soname);
    let temp = dest.with_extension("tmp");

    tokio::fs::write(&temp, &fw_bytes).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
    }

    tokio::fs::rename(&temp, &dest).await?;

    #[cfg(unix)]
    symlinks::create_libkrunfw_symlinks(&lib_dir);

    eprintln!(
        "  {} libkrunfw v{} downloaded to {}",
        "\u{2713}",
        LIBKRUNFW_VERSION,
        lib_dir.display()
    );

    Ok(lib_dir)
}

/// Ensure the iii-init binary is available.
///
/// Resolution order:
/// 1. Check if init is embedded in iii-filesystem (compile-time `embed-init` feature)
/// 2. Check local resolution chain (env var, `~/.iii/lib/iii-init`, adjacent to binary)
/// 3. Download from GitHub releases on first use
///
/// Returns the path to the iii-init binary.
pub async fn ensure_init_binary() -> anyhow::Result<PathBuf> {
    // 1. Check if embedded init is available (non-empty INIT_BYTES)
    if iii_filesystem::init::has_init() {
        tracing::debug!("iii-init is embedded in this build");
        // When embedded, we don't need a file path — the filesystem serves it from memory.
        // Return a sentinel path to indicate "embedded" mode.
        return Ok(PathBuf::from("__embedded__"));
    }

    // 2. Check local resolution chain
    if let Some(path) = resolve_init_binary() {
        tracing::debug!(path = %path.display(), "iii-init found via resolution chain");
        eprintln!("  {} Found iii-init at {}", "\u{2713}", path.display());
        return Ok(path);
    }

    // 3. Download from GitHub releases
    eprintln!("  Downloading iii-init...");
    download_init_binary().await
}

/// Download the iii-init binary from the matching GitHub release.
async fn download_init_binary() -> anyhow::Result<PathBuf> {
    use flate2::read::GzDecoder;
    use futures::StreamExt;
    use std::io::Read;
    use tar::Archive;

    let version = env!("CARGO_PKG_VERSION");
    let archive_name = iii_init_archive_name();

    // The iii-init release is published under the iii release tag
    let release_tag = format!("iii/v{version}");
    let url =
        format!("https://github.com/iii-hq/iii/releases/download/{release_tag}/{archive_name}");

    tracing::info!(%url, "downloading iii-init from GitHub release");

    let client = reqwest::Client::builder()
        .user_agent(format!("iii-worker/{version}"))
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    // Support GitHub token for rate limiting
    let request = if let Ok(token) =
        std::env::var("III_GITHUB_TOKEN").or_else(|_| std::env::var("GITHUB_TOKEN"))
    {
        client
            .get(&url)
            .header("Authorization", format!("token {token}"))
    } else {
        client.get(&url)
    };

    let response = request.send().await?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to download iii-init: HTTP {} from {}\n\
             Hint: Build iii-init manually with:\n  \
             rustup target add {}\n  \
             cargo build -p iii-init --target {} --release",
            response.status(),
            url,
            super::constants::iii_init_musl_target(),
            super::constants::iii_init_musl_target(),
        );
    }

    let total_size = response.content_length().unwrap_or(0);
    let pb = indicatif::ProgressBar::new(total_size);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "  [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .unwrap()
        .progress_chars("=> "),
    );

    let mut bytes = Vec::with_capacity(total_size as usize);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        bytes.extend_from_slice(&chunk);
        pb.set_position(bytes.len() as u64);
    }
    pb.finish_and_clear();

    // Extract iii-init from the tar.gz archive
    let decoder = GzDecoder::new(bytes.as_slice());
    let mut archive = Archive::new(decoder);
    let mut init_bytes = None;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name == III_INIT_FILENAME {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            init_bytes = Some(buf);
            break;
        }
    }

    let init_bytes = init_bytes.ok_or_else(|| {
        anyhow::anyhow!("'{}' not found in downloaded archive", III_INIT_FILENAME)
    })?;

    // Write to ~/.iii/lib/iii-init with atomic rename
    let lib_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".iii")
        .join("lib");
    tokio::fs::create_dir_all(&lib_dir).await?;

    let dest = lib_dir.join(III_INIT_FILENAME);
    let temp = dest.with_extension("tmp");

    tokio::fs::write(&temp, &init_bytes).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
    }

    tokio::fs::rename(&temp, &dest).await?;

    eprintln!(
        "  {} iii-init v{} downloaded to {}",
        "\u{2713}",
        version,
        dest.display()
    );

    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_libkrunfw_bytes_without_embed_feature() {
        let bytes = super::super::libkrunfw_bytes::LIBKRUNFW_BYTES;
        assert!(
            bytes.is_empty(),
            "LIBKRUNFW_BYTES should be empty without embed-libkrunfw feature"
        );
    }

    #[tokio::test]
    async fn test_ensure_libkrunfw_errors_when_not_embedded_and_not_installed() {
        let result = ensure_libkrunfw().await;
        if super::super::libkrunfw_bytes::LIBKRUNFW_BYTES.is_empty() {
            if resolve_libkrunfw_dir().is_none() {
                // Without embedded bytes and no local install, the function attempts
                // a download which will fail in test environments (no matching release).
                // It should error with either a download failure or a connection error.
                assert!(
                    result.is_err(),
                    "should error when not embedded and not installed"
                );
            }
        }
    }
}
