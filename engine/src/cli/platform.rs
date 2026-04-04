// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::path::PathBuf;

use super::error::RegistryError;
use super::registry::BinarySpec;

/// Returns the current platform's target triple for asset lookup.
///
/// Linux x86_64 prefers musl for maximum portability.
/// Linux aarch64 uses gnu (no musl builds available).
pub fn current_target() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "aarch64-apple-darwin";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-darwin";

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "x86_64-unknown-linux-musl";

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "aarch64-unknown-linux-gnu";

    #[cfg(all(target_os = "linux", target_arch = "arm"))]
    return "armv7-unknown-linux-gnueabihf";

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "x86_64-pc-windows-msvc";

    #[cfg(all(target_os = "windows", target_arch = "x86"))]
    return "i686-pc-windows-msvc";

    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return "aarch64-pc-windows-msvc";

    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "arm"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86"),
        all(target_os = "windows", target_arch = "aarch64"),
    )))]
    compile_error!("unsupported target platform for iii")
}

/// Returns the archive extension for the current platform.
pub fn archive_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    }
}

/// Constructs the expected asset filename for a binary on the current platform.
/// e.g., "iii-console-aarch64-apple-darwin.tar.gz"
pub fn asset_name(binary_name: &str) -> String {
    format!(
        "{}-{}.{}",
        binary_name,
        current_target(),
        archive_extension()
    )
}

/// Returns the platform-appropriate data directory for iii.
///
/// - Linux: $XDG_DATA_HOME/iii/ (fallback ~/.local/share/iii/)
/// - macOS: ~/Library/Application Support/iii/
/// - Windows: %LOCALAPPDATA%\iii\
///
/// For backward compatibility, if the old `iii-cli` directory exists and the
/// new `iii` directory does not, the old path is returned so existing state
/// is preserved until the user migrates.
pub fn data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local")
            .join("share")
    });

    let new_dir = base.join("iii");
    let old_dir = base.join("iii-cli");

    // Prefer the new directory; fall back to the old one for migration compat.
    if !new_dir.exists() && old_dir.exists() {
        old_dir
    } else {
        new_dir
    }
}

/// Returns the directory where binaries are stored.
///
/// On macOS/Linux: ~/.local/bin/ (matches install.sh convention).
/// On Windows: %LOCALAPPDATA%\iii\bin\ (unchanged).
///
/// Deliberately decoupled from data_dir() so that binaries live in a
/// PATH-visible location shared with shell-script installers.
pub fn bin_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        data_dir().join("bin")
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local")
            .join("bin")
    }
}

/// Returns the path where a specific binary should be stored.
pub fn binary_path(binary_name: &str) -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        format!("{}.exe", binary_name)
    } else {
        binary_name.to_string()
    };
    bin_dir().join(name)
}

/// Returns the path to the state.json file.
pub fn state_file_path() -> PathBuf {
    data_dir().join("state.json")
}

/// Checks whether the current platform is supported by the given binary.
/// Returns Ok(()) if supported, or an error with a helpful message if not.
pub fn check_platform_support(spec: &BinarySpec) -> Result<(), RegistryError> {
    let target = current_target();
    if spec.supported_targets.contains(&target) {
        Ok(())
    } else {
        let supported = spec
            .supported_targets
            .iter()
            .map(|t| format_target_human(t))
            .collect::<Vec<_>>()
            .join(", ");
        Err(RegistryError::UnsupportedPlatform {
            binary: spec.name.to_string(),
            platform: format_target_human(target),
            supported,
        })
    }
}

/// Formats a target triple into a human-readable string.
fn format_target_human(target: &str) -> String {
    match target {
        "aarch64-apple-darwin" => "macOS (Apple Silicon)".to_string(),
        "x86_64-apple-darwin" => "macOS (Intel)".to_string(),
        "x86_64-unknown-linux-gnu" => "Linux x86_64 (glibc)".to_string(),
        "x86_64-unknown-linux-musl" => "Linux x86_64 (musl)".to_string(),
        "aarch64-unknown-linux-gnu" => "Linux ARM64".to_string(),
        "armv7-unknown-linux-gnueabihf" => "Linux ARMv7".to_string(),
        "x86_64-pc-windows-msvc" => "Windows x86_64".to_string(),
        "i686-pc-windows-msvc" => "Windows x86".to_string(),
        "aarch64-pc-windows-msvc" => "Windows ARM64".to_string(),
        other => other.to_string(),
    }
}

/// Find an existing installation of a binary.
///
/// Checks in order:
/// 1. Our managed bin dir (~/.local/bin/ on macOS/Linux, data_dir/bin on Windows)
/// 2. System PATH
///
/// Returns the path to the binary if found, or None.
pub fn find_existing_binary(binary_name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(target_os = "windows") {
        format!("{}.exe", binary_name)
    } else {
        binary_name.to_string()
    };

    // 1. Check our managed bin dir (~/.local/bin/ on macOS/Linux)
    let managed = bin_dir().join(&exe_name);
    if managed.exists() {
        return Some(managed);
    }

    // 2. Check system PATH
    which_binary(&exe_name)
}

/// Look up a binary on the system PATH.
fn which_binary(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.exists())
    })
}

/// Constructs the expected checksum asset filename for a binary.
/// e.g., "iii-console-aarch64-apple-darwin.sha256"
/// Note: taiki-e produces checksums as separate assets WITHOUT the archive extension.
pub fn checksum_asset_name(binary_name: &str) -> String {
    format!("{}-{}.sha256", binary_name, current_target())
}

/// Ensures the storage directories exist.
///
/// Creates both bin_dir() (~/.local/bin/) and data_dir() (for state.json).
pub fn ensure_dirs() -> Result<(), super::error::StorageError> {
    let bin = bin_dir();
    if !bin.exists() {
        std::fs::create_dir_all(&bin).map_err(|e| super::error::StorageError::CreateDir {
            path: bin.display().to_string(),
            source: e,
        })?;
    }
    let data = data_dir();
    if !data.exists() {
        std::fs::create_dir_all(&data).map_err(|e| super::error::StorageError::CreateDir {
            path: data.display().to_string(),
            source: e,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_target_not_empty() {
        assert!(!current_target().is_empty());
    }

    #[test]
    fn test_archive_extension() {
        let ext = archive_extension();
        assert!(ext == "tar.gz" || ext == "zip");
    }

    #[test]
    fn test_asset_name_format() {
        let name = asset_name("iii-console");
        assert!(name.starts_with("iii-console-"));
        assert!(name.ends_with(archive_extension()));
    }

    #[test]
    fn test_data_dir_not_empty() {
        assert!(!data_dir().as_os_str().is_empty());
    }

    #[test]
    fn test_binary_path_format() {
        let path = binary_path("iii-console");
        assert!(path.to_str().unwrap().contains("iii-console"));
    }

    #[test]
    fn test_platform_support_check() {
        use crate::cli::registry::REGISTRY;
        // iii-console supports all major platforms
        let console = &REGISTRY[0];
        let result = check_platform_support(console);
        assert!(result.is_ok());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_bin_dir_is_local_bin() {
        let bd = bin_dir();
        let bd_str = bd.to_str().unwrap();
        assert!(
            bd_str.ends_with(".local/bin"),
            "bin_dir() should end with .local/bin on Unix, got: {}",
            bd_str
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_bin_dir_separate_from_data_dir() {
        let bd = bin_dir();
        let dd = data_dir();
        assert!(
            !bd.starts_with(&dd),
            "bin_dir() should NOT be a subdirectory of data_dir() on Unix"
        );
    }
}
