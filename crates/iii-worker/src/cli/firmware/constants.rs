//! libkrunfw and iii-init version constants and filename helpers.
//!
//! The firmware library is embedded in the binary at compile time and extracted
//! to `~/.iii/lib/` on first use. The pre-built libkrunfw binaries are
//! committed under `engine/firmware/` (sourced from microsandbox v0.3.8).
//! The VMM (msb_krun) is compiled directly as a Rust dependency.

/// libkrunfw release version.
pub const LIBKRUNFW_VERSION: &str = "5.2.1";

/// libkrunfw ABI version (soname major).
pub const LIBKRUNFW_ABI: &str = "5";

/// Returns the platform-specific libkrunfw filename (the installed soname).
///
/// - macOS: `libkrunfw.5.dylib`
/// - Linux: `libkrunfw.so.5.2.1`
pub fn libkrunfw_filename() -> String {
    if cfg!(target_os = "macos") {
        format!("libkrunfw.{LIBKRUNFW_ABI}.dylib")
    } else {
        format!("libkrunfw.so.{LIBKRUNFW_VERSION}")
    }
}

/// Returns the raw firmware filename as it appears inside the release archive
/// and in the `engine/firmware/` directory.
///
/// - macOS aarch64: `libkrunfw-darwin-aarch64.dylib`
/// - Linux x86_64: `libkrunfw-linux-x86_64.so`
pub fn libkrunfw_firmware_filename() -> String {
    let os_name = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    format!("libkrunfw-{os_name}-{}.{ext}", std::env::consts::ARCH)
}

/// Returns the release archive name for the libkrunfw firmware on the host platform.
///
/// Examples:
/// - `libkrunfw-linux-x86_64.tar.gz`
/// - `libkrunfw-darwin-aarch64.tar.gz`
pub fn libkrunfw_archive_name() -> String {
    let os_name = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    format!(
        "libkrunfw-{os_name}-{}.tar.gz",
        std::env::consts::ARCH
    )
}

/// Check whether libkrunfw firmware is available for the current platform.
///
/// Returns `Err` with a descriptive message for platforms where no firmware
/// binary exists (e.g., Intel Macs / darwin-x86_64).
pub fn check_libkrunfw_platform_support() -> Result<(), String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    // Available firmware: darwin-aarch64, linux-x86_64, linux-aarch64
    // Missing: darwin-x86_64 (Intel Mac)
    if os == "macos" && arch == "x86_64" {
        return Err(format!(
            "libkrunfw firmware is not available for Intel Macs (darwin-x86_64).\n\
             Managed workers require an Apple Silicon Mac (aarch64) or a Linux host.\n\
             Set III_LIBKRUNFW_PATH to a manually-built firmware file to override."
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// iii-init constants
// ---------------------------------------------------------------------------

/// The binary filename for iii-init as it appears in release archives and on disk.
pub const III_INIT_FILENAME: &str = "iii-init";

/// Returns the musl target triple for the init binary based on host CPU architecture.
///
/// The init binary always runs inside a Linux VM guest regardless of the host OS,
/// so macOS `aarch64` maps to `aarch64-unknown-linux-musl`, not an Apple target.
pub fn iii_init_musl_target() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64-unknown-linux-musl",
        "aarch64" => "aarch64-unknown-linux-musl",
        other => panic!("unsupported architecture for iii-init: {}", other),
    }
}

/// Returns the release archive name for iii-init (e.g., `iii-init-x86_64-unknown-linux-musl.tar.gz`).
pub fn iii_init_archive_name() -> String {
    format!("iii-init-{}.tar.gz", iii_init_musl_target())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_libkrunfw_version_constants() {
        assert_eq!(LIBKRUNFW_VERSION, "5.2.1");
        assert_eq!(LIBKRUNFW_ABI, "5");
    }

    #[test]
    fn test_libkrunfw_filename() {
        let name = libkrunfw_filename();
        if cfg!(target_os = "macos") {
            assert_eq!(name, "libkrunfw.5.dylib");
        } else {
            assert_eq!(name, "libkrunfw.so.5.2.1");
        }
    }

    #[test]
    fn test_libkrunfw_firmware_filename() {
        let name = libkrunfw_firmware_filename();
        assert!(name.starts_with("libkrunfw-"), "should start with 'libkrunfw-': {name}");
        if cfg!(target_os = "macos") {
            assert!(name.contains("darwin"), "macOS should have 'darwin': {name}");
            assert!(name.ends_with(".dylib"), "macOS should end with '.dylib': {name}");
        } else {
            assert!(name.contains("linux"), "Linux should have 'linux': {name}");
            assert!(name.ends_with(".so"), "Linux should end with '.so': {name}");
        }
    }

    #[test]
    fn test_libkrunfw_archive_name() {
        let name = libkrunfw_archive_name();
        assert!(name.starts_with("libkrunfw-"), "should start with 'libkrunfw-': {name}");
        assert!(name.ends_with(".tar.gz"), "should end with '.tar.gz': {name}");
        if cfg!(target_os = "macos") {
            assert!(name.contains("darwin"), "macOS should have 'darwin': {name}");
        } else {
            assert!(name.contains("linux"), "Linux should have 'linux': {name}");
        }
    }

    #[test]
    fn test_iii_init_filename() {
        assert_eq!(III_INIT_FILENAME, "iii-init");
    }

    #[test]
    fn test_iii_init_musl_target() {
        let target = iii_init_musl_target();
        assert!(
            target == "x86_64-unknown-linux-musl" || target == "aarch64-unknown-linux-musl",
            "unexpected target: {target}"
        );
    }

    #[test]
    fn test_iii_init_archive_name() {
        let name = iii_init_archive_name();
        assert!(name.starts_with("iii-init-"));
        assert!(name.ends_with(".tar.gz"));
    }
}
