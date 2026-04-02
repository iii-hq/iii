//! Runtime resolution of libkrunfw and libkrun shared library locations.
//!
//! Resolution chain (checked in order for both libraries):
//! 1. Environment variable override (file path -> parent dir, or directory)
//! 2. `~/.iii/lib/` (managed download / extraction location)
//! 3. Adjacent to the current binary
//! 4. System paths (`/usr/lib`, `/usr/local/lib`, Homebrew on macOS)

use std::path::{Path, PathBuf};

use super::constants::libkrunfw_filename;

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

/// Internal testable resolution function.
///
/// Takes all path sources as arguments so the resolution chain can be tested
/// without depending on `dirs::home_dir()` or `current_exe()`.
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
            // Pointing at the file itself -- return its parent directory
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

    // 4. System paths (backward compatibility)
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
    fn test_resolve_env_var_dir_path() {
        let tmp = TempDir::new().unwrap();
        let filename = libkrunfw_filename();
        std::fs::write(tmp.path().join(&filename), b"fake").unwrap();

        let result = resolve_dir_with_paths(
            Some(tmp.path().to_str().unwrap()),
            None,
            None,
            &[],
            &filename,
        );
        assert_eq!(result, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn test_resolve_iii_lib() {
        let tmp = TempDir::new().unwrap();
        let lib_dir = tmp.path().join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        let filename = libkrunfw_filename();
        std::fs::write(lib_dir.join(&filename), b"fake").unwrap();

        let result = resolve_dir_with_paths(None, Some(&lib_dir), None, &[], &filename);
        assert_eq!(result, Some(lib_dir));
    }

    #[test]
    fn test_resolve_adjacent_to_binary() {
        let tmp = TempDir::new().unwrap();
        let filename = libkrunfw_filename();
        std::fs::write(tmp.path().join(&filename), b"fake").unwrap();

        let result = resolve_dir_with_paths(None, None, Some(tmp.path()), &[], &filename);
        assert_eq!(result, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn test_resolve_system_path() {
        let tmp = TempDir::new().unwrap();
        let sys_dir = tmp.path().join("sys");
        std::fs::create_dir_all(&sys_dir).unwrap();
        let filename = libkrunfw_filename();
        std::fs::write(sys_dir.join(&filename), b"fake").unwrap();

        let result =
            resolve_dir_with_paths(None, None, None, &[sys_dir.as_path()], &filename);
        assert_eq!(result, Some(sys_dir));
    }

    #[test]
    fn test_resolve_not_found() {
        let result = resolve_dir_with_paths(None, None, None, &[], &libkrunfw_filename());
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_priority_env_over_lib() {
        let env_tmp = TempDir::new().unwrap();
        let lib_tmp = TempDir::new().unwrap();
        let filename = libkrunfw_filename();

        // Both locations have the file
        std::fs::write(env_tmp.path().join(&filename), b"env").unwrap();
        std::fs::write(lib_tmp.path().join(&filename), b"lib").unwrap();

        let result = resolve_dir_with_paths(
            Some(env_tmp.path().to_str().unwrap()),
            Some(lib_tmp.path()),
            None,
            &[],
            &filename,
        );
        // Env var should win
        assert_eq!(result, Some(env_tmp.path().to_path_buf()));
    }

}
