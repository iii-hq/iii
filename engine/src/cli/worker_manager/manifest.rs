// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use toml_edit::{DocumentMut, value};

use crate::cli::error::WorkerError;

#[derive(Deserialize)]
struct ManifestFile {
    workers: Option<BTreeMap<String, String>>,
}

/// Read the iii.toml manifest from the project directory.
/// Returns a BTreeMap of worker_name -> version.
/// If the file does not exist, returns an empty BTreeMap.
pub fn read_manifest(project_dir: &Path) -> Result<BTreeMap<String, String>, WorkerError> {
    let manifest_path = project_dir.join("iii.toml");

    if !manifest_path.exists() {
        return Ok(BTreeMap::new());
    }

    let content = fs::read_to_string(&manifest_path).map_err(|e| {
        WorkerError::ManifestError(format!("Failed to read {}: {}", manifest_path.display(), e))
    })?;

    let parsed: ManifestFile = toml::from_str(&content).map_err(|e| {
        WorkerError::ManifestError(format!(
            "Failed to parse {}: {}",
            manifest_path.display(),
            e
        ))
    })?;

    Ok(parsed.workers.unwrap_or_default())
}

/// Add or update a worker entry in iii.toml.
/// Creates the file with [workers] header if it does not exist.
/// Entries are kept sorted alphabetically.
/// Uses atomic write (write to tmp, rename) to prevent corruption.
pub fn add_or_update(
    project_dir: &Path,
    worker_name: &str,
    version: &str,
) -> Result<(), WorkerError> {
    let manifest_path = project_dir.join("iii.toml");
    let tmp_path = project_dir.join("iii.toml.tmp");

    let content = if manifest_path.exists() {
        fs::read_to_string(&manifest_path).map_err(|e| {
            WorkerError::ManifestError(format!("Failed to read {}: {}", manifest_path.display(), e))
        })?
    } else {
        "[workers]\n".to_string()
    };

    let mut doc: DocumentMut = content.parse().map_err(|e| {
        WorkerError::ManifestError(format!(
            "Failed to parse {}: {}",
            manifest_path.display(),
            e
        ))
    })?;

    // Ensure [workers] table exists
    if !doc.contains_table("workers") {
        doc["workers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    // Set the worker entry
    doc["workers"][worker_name] = value(version);

    // Sort entries in the workers table
    if let Some(table) = doc["workers"].as_table_mut() {
        table.sort_values();
    }

    // Atomic write: write to tmp file, then rename
    fs::write(&tmp_path, doc.to_string()).map_err(|e| {
        WorkerError::ManifestError(format!("Failed to write {}: {}", tmp_path.display(), e))
    })?;

    fs::rename(&tmp_path, &manifest_path).map_err(|e| {
        WorkerError::ManifestError(format!(
            "Failed to rename {} to {}: {}",
            tmp_path.display(),
            manifest_path.display(),
            e
        ))
    })?;

    Ok(())
}

/// Remove a worker entry from iii.toml.
/// If the worker is not present, this is a no-op (idempotent).
/// Uses atomic write (write to tmp, rename) to prevent corruption.
pub fn remove(project_dir: &Path, worker_name: &str) -> Result<(), WorkerError> {
    let manifest_path = project_dir.join("iii.toml");

    if !manifest_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&manifest_path).map_err(|e| {
        WorkerError::ManifestError(format!("Failed to read {}: {}", manifest_path.display(), e))
    })?;

    let mut doc: DocumentMut = content.parse().map_err(|e| {
        WorkerError::ManifestError(format!(
            "Failed to parse {}: {}",
            manifest_path.display(),
            e
        ))
    })?;

    if let Some(table) = doc["workers"].as_table_mut() {
        table.remove(worker_name);
    }

    let tmp_path = project_dir.join("iii.toml.tmp");
    fs::write(&tmp_path, doc.to_string()).map_err(|e| {
        WorkerError::ManifestError(format!("Failed to write {}: {}", tmp_path.display(), e))
    })?;

    fs::rename(&tmp_path, &manifest_path)
        .map_err(|e| WorkerError::ManifestError(format!("Failed to rename: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_add_or_update_creates_file_with_workers_header() {
        let dir = TempDir::new().unwrap();
        add_or_update(dir.path(), "pdfkit", "1.0.0").unwrap();

        let content = fs::read_to_string(dir.path().join("iii.toml")).unwrap();
        assert!(content.contains("[workers]"));
        assert!(content.contains("pdfkit = \"1.0.0\""));
    }

    #[test]
    fn test_add_or_update_adds_worker_entry() {
        let dir = TempDir::new().unwrap();
        add_or_update(dir.path(), "image-processor", "2.3.1").unwrap();

        let content = fs::read_to_string(dir.path().join("iii.toml")).unwrap();
        assert!(content.contains("image-processor = \"2.3.1\""));
    }

    #[test]
    fn test_add_or_update_updates_existing_version() {
        let dir = TempDir::new().unwrap();
        add_or_update(dir.path(), "pdfkit", "1.0.0").unwrap();
        add_or_update(dir.path(), "pdfkit", "2.0.0").unwrap();

        let content = fs::read_to_string(dir.path().join("iii.toml")).unwrap();
        assert!(content.contains("pdfkit = \"2.0.0\""));
        assert!(!content.contains("pdfkit = \"1.0.0\""));
    }

    #[test]
    fn test_add_or_update_keeps_entries_sorted() {
        let dir = TempDir::new().unwrap();
        add_or_update(dir.path(), "zebra", "1.0.0").unwrap();
        add_or_update(dir.path(), "alpha", "1.0.0").unwrap();

        let content = fs::read_to_string(dir.path().join("iii.toml")).unwrap();
        let alpha_pos = content.find("alpha").unwrap();
        let zebra_pos = content.find("zebra").unwrap();
        assert!(
            alpha_pos < zebra_pos,
            "alpha should appear before zebra in sorted output"
        );
    }

    #[test]
    fn test_add_or_update_preserves_comments() {
        let dir = TempDir::new().unwrap();
        let initial = "# Project manifest\n[workers]\n# My worker\npdfkit = \"1.0.0\"\n";
        fs::write(dir.path().join("iii.toml"), initial).unwrap();

        add_or_update(dir.path(), "image-processor", "2.0.0").unwrap();

        let content = fs::read_to_string(dir.path().join("iii.toml")).unwrap();
        assert!(
            content.contains("# Project manifest"),
            "Top comment should be preserved"
        );
    }

    #[test]
    fn test_read_manifest_returns_empty_when_no_file() {
        let dir = TempDir::new().unwrap();
        let result = read_manifest(dir.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_remove_existing_worker() {
        let dir = TempDir::new().unwrap();
        let content = "[workers]\nalpha = \"1.0.0\"\nbeta = \"2.0.0\"\n";
        fs::write(dir.path().join("iii.toml"), content).unwrap();

        remove(dir.path(), "alpha").unwrap();

        let result = read_manifest(dir.path()).unwrap();
        assert!(!result.contains_key("alpha"));
        assert_eq!(result.get("beta").unwrap(), "2.0.0");
    }

    #[test]
    fn test_remove_preserves_comments() {
        let dir = TempDir::new().unwrap();
        let content =
            "# Project manifest\n[workers]\n# My workers\nalpha = \"1.0.0\"\nbeta = \"2.0.0\"\n";
        fs::write(dir.path().join("iii.toml"), content).unwrap();

        remove(dir.path(), "alpha").unwrap();

        let file_content = fs::read_to_string(dir.path().join("iii.toml")).unwrap();
        assert!(file_content.contains("# Project manifest"));
        assert!(file_content.contains("beta = \"2.0.0\""));
    }

    #[test]
    fn test_remove_nonexistent_worker_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let content = "[workers]\nalpha = \"1.0.0\"\n";
        fs::write(dir.path().join("iii.toml"), content).unwrap();

        // Should not error when removing a worker that doesn't exist
        remove(dir.path(), "nonexistent").unwrap();

        let result = read_manifest(dir.path()).unwrap();
        assert_eq!(result.get("alpha").unwrap(), "1.0.0");
    }

    #[test]
    fn test_remove_no_manifest_file_is_ok() {
        let dir = TempDir::new().unwrap();
        // No iii.toml file exists - should be a no-op
        remove(dir.path(), "anything").unwrap();
    }

    #[test]
    fn test_read_manifest_returns_workers() {
        let dir = TempDir::new().unwrap();
        let content = "[workers]\nalpha = \"1.0.0\"\nbeta = \"2.0.0\"\n";
        fs::write(dir.path().join("iii.toml"), content).unwrap();

        let result = read_manifest(dir.path()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("alpha").unwrap(), "1.0.0");
        assert_eq!(result.get("beta").unwrap(), "2.0.0");
    }

    #[test]
    fn test_read_manifest_sorted_alphabetically() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("iii.toml"),
            "[workers]\nzulu = \"1.0.0\"\nalpha = \"2.0.0\"\nmike = \"3.0.0\"\n",
        )
        .unwrap();
        let workers = read_manifest(dir.path()).unwrap();
        let keys: Vec<&String> = workers.keys().collect();
        assert_eq!(
            keys,
            vec!["alpha", "mike", "zulu"],
            "BTreeMap should sort keys alphabetically"
        );
    }

    #[test]
    fn test_read_manifest_with_invalid_toml() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("iii.toml"), "this is not { valid toml !!!").unwrap();

        let result = read_manifest(dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            WorkerError::ManifestError(_) => {}
            other => panic!("Expected ManifestError, got: {:?}", other),
        }
    }

    #[test]
    fn test_read_manifest_with_no_workers_section() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("iii.toml"),
            "[metadata]\nname = \"my-project\"\n",
        )
        .unwrap();

        let result = read_manifest(dir.path()).unwrap();
        assert!(
            result.is_empty(),
            "Missing [workers] section should yield empty map"
        );
    }

    #[test]
    fn test_add_or_update_with_invalid_existing_toml() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("iii.toml"), "corrupt { data = ???").unwrap();

        let result = add_or_update(dir.path(), "pdfkit", "1.0.0");
        assert!(result.is_err());
        match result.unwrap_err() {
            WorkerError::ManifestError(_) => {}
            other => panic!("Expected ManifestError, got: {:?}", other),
        }
    }

    #[test]
    fn test_read_manifest_with_empty_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("iii.toml"), "").unwrap();

        // An empty string is valid TOML (empty document), but has no [workers] section,
        // so deserialization succeeds and unwrap_or_default returns an empty map.
        let result = read_manifest(dir.path()).unwrap();
        assert!(result.is_empty(), "Empty file should yield empty map");
    }

    #[test]
    fn test_add_or_update_two_entries_preserves_both() {
        let dir = TempDir::new().unwrap();
        add_or_update(dir.path(), "pdfkit", "1.0.0").unwrap();
        add_or_update(dir.path(), "image-processor", "2.3.1").unwrap();

        let result = read_manifest(dir.path()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("pdfkit").unwrap(), "1.0.0");
        assert_eq!(result.get("image-processor").unwrap(), "2.3.1");
    }

    #[test]
    fn test_remove_last_worker_leaves_empty_table() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("iii.toml"),
            "[workers]\nonly-one = \"1.0.0\"\n",
        )
        .unwrap();

        remove(dir.path(), "only-one").unwrap();

        let content = fs::read_to_string(dir.path().join("iii.toml")).unwrap();
        assert!(
            content.contains("[workers]"),
            "[workers] section should still exist"
        );

        let result = read_manifest(dir.path()).unwrap();
        assert!(
            result.is_empty(),
            "Workers map should be empty after removing the last entry"
        );
    }
}