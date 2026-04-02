//! Lazy provisioning of libkrunfw from embedded bytes.
//!
//! The firmware library is embedded at compile time (via the `embed-libkrunfw`
//! feature) and extracted to `~/.iii/lib/` on first use.

use std::path::PathBuf;

use super::constants::{LIBKRUNFW_VERSION, libkrunfw_filename};
use super::resolve::resolve_libkrunfw_dir;
use super::symlinks;

/// Ensure libkrunfw is available, extracting from embedded bytes if necessary.
///
/// Resolution order:
/// 1. Already on disk (system install, Homebrew, previous extraction)
/// 2. Extract from embedded bytes to `~/.iii/lib/`
/// 3. Error if not found and not embedded
pub async fn ensure_libkrunfw() -> anyhow::Result<PathBuf> {
    if let Some(dir) = resolve_libkrunfw_dir() {
        tracing::debug!(dir = %dir.display(), "libkrunfw found via resolution chain");
        return Ok(dir);
    }

    let bytes = super::libkrunfw_bytes::LIBKRUNFW_BYTES;
    if bytes.is_empty() {
        anyhow::bail!(
            "libkrunfw not found and not embedded in this build. \
             Install libkrunfw manually or rebuild with --features embed-libkrunfw"
        );
    }

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

    Ok(lib_dir)
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
                assert!(result.is_err());
                let err_msg = result.unwrap_err().to_string();
                assert!(
                    err_msg.contains("not found and not embedded"),
                    "Error message should mention not found: {err_msg}"
                );
            }
        }
    }
}
