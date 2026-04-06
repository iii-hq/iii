//! Runtime resolution of libkrunfw shared library and iii-init binary locations.
//!
//! Resolution chain (checked in order):
//! 1. Environment variable override (file path -> parent dir, or directory)
//! 2. `~/.iii/lib/` (managed download / extraction location)
//! 3. Adjacent to the current binary (skipped in Cargo `target/` directories
//!    to prevent picking up glibc-linked builds)
//! 4. System paths (`/usr/lib`, `/usr/local/lib`, Homebrew on macOS)

use std::path::{Path, PathBuf};

use super::constants::{III_INIT_FILENAME, libkrunfw_filename};

/// Returns the platform-correct environment variable name for the shared library search path.
///
/// - macOS: `DYLD_LIBRARY_PATH`
/// - Linux: `LD_LIBRARY_PATH`
pub fn lib_path_env_var() -> &'static str {
    if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    }
}

/// Resolve the directory containing libkrunfw.
///
/// Checks (in order): env var override, `~/.iii/lib/`, adjacent to binary, system paths.
/// Returns `None` if the library is not found in any location.
pub fn resolve_libkrunfw_dir() -> Option<PathBuf> {
    let filename = libkrunfw_filename();

    // Build list of paths to check
    let env_path = std::env::var("III_LIBKRUNFW_PATH").ok();
    let lib_dir = dirs::home_dir().map(|h| h.join(".iii").join("lib"));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let system_paths: Vec<&Path> = if cfg!(target_os = "macos") {
        vec![
            Path::new("/usr/lib"),
            Path::new("/usr/local/lib"),
            Path::new("/opt/homebrew/opt/libkrun/lib"),
        ]
    } else {
        vec![Path::new("/usr/lib"), Path::new("/usr/local/lib")]
    };

    resolve_dir_with_paths(
        env_path.as_deref(),
        lib_dir.as_deref(),
        exe_dir.as_deref(),
        &system_paths,
        &filename,
    )
}

/// Resolve the path to the iii-init binary.
///
/// Checks (in order):
/// 1. `III_INIT_PATH` environment variable override
/// 2. `~/.iii/lib/iii-init` (managed download location)
/// 3. Adjacent to the current binary (skipped in Cargo `target/` directories
///    to prevent picking up glibc-linked builds -- the VM guest requires a
///    statically linked musl binary)
///
/// Returns `None` if the init binary is not found in any location.
pub fn resolve_init_binary() -> Option<PathBuf> {
    // 1. III_INIT_PATH env var
    if let Some(env_val) = std::env::var("III_INIT_PATH").ok() {
        let p = PathBuf::from(env_val);
        if p.is_file() {
            return Some(p);
        }
    }

    // 2. ~/.iii/lib/iii-init
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".iii").join("lib").join(III_INIT_FILENAME);
        if path.is_file() {
            return Some(path);
        }
    }

    // 3. Adjacent to current binary (skip in Cargo target directories to avoid
    //    picking up the default-target glibc-linked build instead of the required
    //    musl-linked static binary)
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        if !is_cargo_target_dir(&exe_dir) {
            let path = exe_dir.join(III_INIT_FILENAME);
            if path.is_file() {
                return Some(path);
            }
        } else {
            tracing::debug!(
                dir = %exe_dir.display(),
                "skipping adjacent iii-init in Cargo target directory"
            );
        }
    }

    None
}

/// Returns `true` if the given directory looks like a Cargo `target/` output directory.
///
/// Matches `target/release`, `target/debug`, but NOT `target/{triple}/release` --
/// the latter is where cross-compiled musl builds live and should not be skipped.
fn is_cargo_target_dir(dir: &Path) -> bool {
    let dir_name = match dir.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };

    if dir_name != "release" && dir_name != "debug" {
        return false;
    }

    match dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
    {
        // Direct child of `target/` → this is `target/release` or `target/debug`
        Some("target") => true,
        // Otherwise it's `target/{triple}/release` or some other path → not matched
        _ => false,
    }
}

/// Internal testable resolution function.
fn resolve_dir_with_paths(
    env_path: Option<&str>,
    lib_dir: Option<&Path>,
    exe_dir: Option<&Path>,
    system_paths: &[&Path],
    filename: &str,
) -> Option<PathBuf> {
    // 1. III_LIBKRUNFW_PATH env var
    if let Some(env_val) = env_path {
        let p = PathBuf::from(env_val);
        if p.is_file() {
            return p.parent().map(|d| d.to_path_buf());
        }
        if p.is_dir() && p.join(filename).exists() {
            return Some(p);
        }
    }

    // 2. ~/.iii/lib/
    if let Some(dir) = lib_dir {
        if dir.join(filename).exists() {
            return Some(dir.to_path_buf());
        }
    }

    // 3. Adjacent to current binary
    if let Some(dir) = exe_dir {
        if dir.join(filename).exists() {
            return Some(dir.to_path_buf());
        }
    }

    // 4. System paths
    for path in system_paths {
        if path.join(filename).exists() {
            return Some(path.to_path_buf());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_lib_path_env_var() {
        let var = lib_path_env_var();
        if cfg!(target_os = "macos") {
            assert_eq!(var, "DYLD_LIBRARY_PATH");
        } else {
            assert_eq!(var, "LD_LIBRARY_PATH");
        }
    }

    #[test]
    fn test_resolve_env_var_file_path() {
        let tmp = TempDir::new().unwrap();
        let filename = libkrunfw_filename();
        let file_path = tmp.path().join(&filename);
        std::fs::write(&file_path, b"fake").unwrap();

        let result = resolve_dir_with_paths(
            Some(file_path.to_str().unwrap()),
            None,
            None,
            &[],
            &filename,
        );
        assert_eq!(result, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn test_resolve_not_found() {
        let result = resolve_dir_with_paths(None, None, None, &[], &libkrunfw_filename());
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_init_binary_env_var() {
        let tmp = TempDir::new().unwrap();
        let init_path = tmp.path().join(III_INIT_FILENAME);
        std::fs::write(&init_path, b"fake-init").unwrap();

        // SAFETY: test-only, single-threaded access to env var
        unsafe {
            std::env::set_var("III_INIT_PATH", init_path.to_str().unwrap());
        }
        let result = resolve_init_binary();
        unsafe {
            std::env::remove_var("III_INIT_PATH");
        }

        assert_eq!(result, Some(init_path));
    }

    #[test]
    fn test_is_cargo_target_dir_release() {
        assert!(is_cargo_target_dir(Path::new(
            "/some/project/target/release"
        )));
    }

    #[test]
    fn test_is_cargo_target_dir_debug() {
        assert!(is_cargo_target_dir(Path::new("/some/project/target/debug")));
    }

    #[test]
    fn test_is_cargo_target_dir_cross_compile_not_matched() {
        // target/{triple}/release should NOT be matched -- musl builds live here
        assert!(!is_cargo_target_dir(Path::new(
            "/some/project/target/x86_64-unknown-linux-musl/release"
        )));
    }

    #[test]
    fn test_is_cargo_target_dir_system_path_not_matched() {
        assert!(!is_cargo_target_dir(Path::new("/usr/local/bin")));
    }

    #[test]
    fn test_is_cargo_target_dir_home_iii_lib_not_matched() {
        assert!(!is_cargo_target_dir(Path::new("/home/user/.iii/lib")));
    }
}
