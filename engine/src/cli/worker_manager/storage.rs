// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::error::WorkerError;

/// Returns the path where a worker binary should be stored within the project.
/// On Unix: project_dir/iii_workers/worker_name
/// On Windows: project_dir/iii_workers/worker_name.exe
pub fn worker_binary_path(project_dir: &Path, worker_name: &str) -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        format!("{}.exe", worker_name)
    } else {
        worker_name.to_string()
    };
    project_dir.join("iii_workers").join(name)
}

/// Ensure the iii_workers/ directory exists within the project directory.
/// Creates it if it does not exist.
pub fn ensure_workers_dir(project_dir: &Path) -> Result<(), WorkerError> {
    let workers_dir = project_dir.join("iii_workers");
    if !workers_dir.exists() {
        fs::create_dir_all(&workers_dir)?;
    }
    Ok(())
}

/// Returns the path to a worker's version marker file (iii_workers/.name.version).
fn version_marker_path(project_dir: &Path, worker_name: &str) -> PathBuf {
    project_dir
        .join("iii_workers")
        .join(format!(".{}.version", worker_name))
}

/// Read the installed version for a worker from its version marker file.
/// Returns None if the marker does not exist or cannot be read.
pub fn read_installed_version(project_dir: &Path, worker_name: &str) -> Option<String> {
    fs::read_to_string(version_marker_path(project_dir, worker_name))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Write a version marker file for a successfully installed worker.
pub fn write_version_marker(
    project_dir: &Path,
    worker_name: &str,
    version: &str,
) -> Result<(), WorkerError> {
    fs::write(version_marker_path(project_dir, worker_name), version)?;
    Ok(())
}

/// Remove the version marker file for a worker.
pub fn remove_version_marker(project_dir: &Path, worker_name: &str) -> Result<(), WorkerError> {
    let path = version_marker_path(project_dir, worker_name);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Remove a worker binary from iii_workers/.
/// Returns Ok(true) if the file existed and was deleted, Ok(false) if it was already absent.
pub fn remove_worker_binary(project_dir: &Path, worker_name: &str) -> Result<bool, WorkerError> {
    let path = worker_binary_path(project_dir, worker_name);
    if path.exists() {
        fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_worker_binary_path_unix() {
        let dir = Path::new("/tmp/myproject");
        let path = worker_binary_path(dir, "pdfkit");

        if cfg!(target_os = "windows") {
            assert_eq!(path, dir.join("iii_workers").join("pdfkit.exe"));
        } else {
            assert_eq!(path, dir.join("iii_workers").join("pdfkit"));
        }
    }

    #[test]
    fn test_worker_binary_path_contains_iii_workers() {
        let dir = Path::new("/some/project");
        let path = worker_binary_path(dir, "image-processor");
        assert!(path.to_str().unwrap().contains("iii_workers"));
    }

    #[test]
    fn test_ensure_workers_dir_creates_directory() {
        let dir = TempDir::new().unwrap();
        let workers_dir = dir.path().join("iii_workers");
        assert!(!workers_dir.exists());

        ensure_workers_dir(dir.path()).unwrap();
        assert!(workers_dir.exists());
        assert!(workers_dir.is_dir());
    }

    #[test]
    fn test_remove_worker_binary_existing() {
        let dir = TempDir::new().unwrap();
        ensure_workers_dir(dir.path()).unwrap();
        let binary_path = worker_binary_path(dir.path(), "pdfkit");
        fs::write(&binary_path, b"fake binary").unwrap();

        let result = remove_worker_binary(dir.path(), "pdfkit").unwrap();
        assert!(result, "Should return true when file existed");
        assert!(!binary_path.exists(), "File should be deleted");
    }

    #[test]
    fn test_remove_worker_binary_absent() {
        let dir = TempDir::new().unwrap();
        ensure_workers_dir(dir.path()).unwrap();

        let result = remove_worker_binary(dir.path(), "nonexistent").unwrap();
        assert!(!result, "Should return false when file was already absent");
    }

    #[test]
    fn test_ensure_workers_dir_idempotent() {
        let dir = TempDir::new().unwrap();
        ensure_workers_dir(dir.path()).unwrap();
        // Should not error on second call
        ensure_workers_dir(dir.path()).unwrap();
        assert!(dir.path().join("iii_workers").exists());
    }

    #[test]
    fn test_worker_binary_path_hyphenated_name() {
        let dir = Path::new("/tmp/myproject");
        let path = worker_binary_path(dir, "image-processor");

        if cfg!(target_os = "windows") {
            assert_eq!(path, dir.join("iii_workers").join("image-processor.exe"));
        } else {
            assert_eq!(path, dir.join("iii_workers").join("image-processor"));
        }
    }

    #[test]
    fn test_worker_binary_path_single_char_name() {
        let dir = Path::new("/tmp/myproject");
        let path = worker_binary_path(dir, "a");

        if cfg!(target_os = "windows") {
            assert_eq!(path, dir.join("iii_workers").join("a.exe"));
        } else {
            assert_eq!(path, dir.join("iii_workers").join("a"));
        }
    }

    #[test]
    fn test_ensure_workers_dir_preserves_existing_files() {
        let dir = TempDir::new().unwrap();
        ensure_workers_dir(dir.path()).unwrap();

        let existing_file = dir.path().join("iii_workers").join("existing_binary");
        fs::write(&existing_file, b"should survive").unwrap();

        ensure_workers_dir(dir.path()).unwrap();

        assert!(
            existing_file.exists(),
            "Pre-existing file should be preserved"
        );
        assert_eq!(fs::read(&existing_file).unwrap(), b"should survive");
    }

    #[test]
    fn test_remove_worker_binary_no_iii_workers_dir() {
        let dir = TempDir::new().unwrap();
        // Do NOT call ensure_workers_dir -- iii_workers does not exist
        assert!(!dir.path().join("iii_workers").exists());

        let result = remove_worker_binary(dir.path(), "ghost").unwrap();
        assert!(
            !result,
            "Should return false when iii_workers dir does not exist"
        );
    }

    #[test]
    fn test_multiple_binaries_coexist() {
        let dir = TempDir::new().unwrap();
        ensure_workers_dir(dir.path()).unwrap();

        let path_a = worker_binary_path(dir.path(), "alpha");
        let path_b = worker_binary_path(dir.path(), "beta");
        fs::write(&path_a, b"binary-a").unwrap();
        fs::write(&path_b, b"binary-b").unwrap();

        let removed = remove_worker_binary(dir.path(), "alpha").unwrap();
        assert!(removed, "alpha should have been removed");
        assert!(!path_a.exists(), "alpha binary should be gone");
        assert!(path_b.exists(), "beta binary should still exist");
        assert_eq!(fs::read(&path_b).unwrap(), b"binary-b");
    }
}
