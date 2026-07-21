// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Single shared resolver for locating an `iii-*` helper binary to EXECUTE.
//!
//! Every place that spawns a sibling binary (`iii-worker`, `iii-console`, ...)
//! routes through [`find_existing_binary`] so they can never disagree on which
//! copy runs:
//!
//! - the CLI dispatcher (`cli::platform::find_existing_binary`, bin crate),
//! - the engine's registry-worker spawner (`workers::registry_worker`),
//! - the engine's known-external spawner (`workers::external`).
//!
//! Resolution is **PATH-first**: a dev / override build earlier on `$PATH`
//! wins over the managed install. The managed `~/.local/bin` copy is only the
//! fallback. This lives in the lib (not the bin-only `cli` module) so both the
//! bin and the lib workers can share it.
//!
//! Note: this is the *execution* resolver. The install/update lifecycle
//! (`cli::update`) deliberately probes the managed `~/.local/bin` copy first,
//! because it manages that specific file regardless of what's on `$PATH`.

use std::path::PathBuf;

/// Managed binary install directory (fallback lookup location).
///
/// - macOS/Linux: `~/.local/bin/` (matches `install.sh` and
///   `cli::platform::bin_dir`).
/// - Windows: `%LOCALAPPDATA%\iii\bin\`.
pub fn managed_bin_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("iii")
            .join("bin")
    }
    #[cfg(not(target_os = "windows"))]
    {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local")
            .join("bin")
    }
}

/// Resolve `binary_name` to an executable path, PATH-first then managed dir.
///
/// Returns `None` if the binary is not found in either location.
pub fn find_existing_binary(binary_name: &str) -> Option<PathBuf> {
    // 1. System PATH. `which` handles Windows `.exe`/PATHEXT resolution and
    //    the executable-bit check.
    if let Ok(p) = which::which(binary_name) {
        return Some(p);
    }

    // 2. Managed bin dir fallback.
    let exe_name = if cfg!(target_os = "windows") {
        format!("{}.exe", binary_name)
    } else {
        binary_name.to_string()
    };
    let managed = managed_bin_dir().join(exe_name);
    managed.exists().then_some(managed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    fn write_exe(dir: &std::path::Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// PATH wins even when a managed copy also exists — the whole point of the
    /// change. Uses HOME to relocate the managed dir so the test is hermetic.
    #[test]
    #[serial]
    #[cfg(unix)]
    fn path_takes_precedence_over_managed() {
        let path_dir = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();
        let managed = home_dir.path().join(".local").join("bin");
        fs::create_dir_all(&managed).unwrap();

        let on_path = write_exe(path_dir.path(), "iii-fake");
        write_exe(&managed, "iii-fake"); // also present in managed dir

        let orig_path = std::env::var_os("PATH");
        let orig_home = std::env::var_os("HOME");
        // SAFETY: #[serial] guarantees no parallel env mutation.
        unsafe {
            std::env::set_var("PATH", path_dir.path());
            std::env::set_var("HOME", home_dir.path());
        }

        let resolved = find_existing_binary("iii-fake");

        unsafe {
            restore("PATH", orig_path);
            restore("HOME", orig_home);
        }

        assert_eq!(resolved.as_deref(), Some(on_path.as_path()));
    }

    /// PATH miss falls back to the managed dir.
    #[test]
    #[serial]
    #[cfg(unix)]
    fn falls_back_to_managed_when_not_on_path() {
        let empty_path = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();
        let managed = home_dir.path().join(".local").join("bin");
        fs::create_dir_all(&managed).unwrap();
        let in_managed = write_exe(&managed, "iii-fake");

        let orig_path = std::env::var_os("PATH");
        let orig_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("PATH", empty_path.path());
            std::env::set_var("HOME", home_dir.path());
        }

        let resolved = find_existing_binary("iii-fake");

        unsafe {
            restore("PATH", orig_path);
            restore("HOME", orig_home);
        }

        assert_eq!(resolved.as_deref(), Some(in_managed.as_path()));
    }

    /// Neither location has it → None.
    #[test]
    #[serial]
    #[cfg(unix)]
    fn none_when_absent_everywhere() {
        let empty_path = tempfile::tempdir().unwrap();
        let home_dir = tempfile::tempdir().unwrap();

        let orig_path = std::env::var_os("PATH");
        let orig_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("PATH", empty_path.path());
            std::env::set_var("HOME", home_dir.path());
        }

        let resolved = find_existing_binary("iii-fake");

        unsafe {
            restore("PATH", orig_path);
            restore("HOME", orig_home);
        }

        assert!(resolved.is_none());
    }

    #[cfg(unix)]
    unsafe fn restore(key: &str, orig: Option<std::ffi::OsString>) {
        match orig {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
}
