//! libkrunfw version constants and filename helpers.
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
}
