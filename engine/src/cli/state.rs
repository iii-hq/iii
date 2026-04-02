// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};

use super::error::StateError;

/// Persistent state tracking installed binaries and update checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    /// Installed binary metadata keyed by binary name
    #[serde(default)]
    pub binaries: HashMap<String, BinaryState>,

    /// Timestamp of last update check
    #[serde(default)]
    pub last_update_check: Option<DateTime<Utc>>,

    /// Hours between update checks (default: 24)
    #[serde(default = "default_interval")]
    pub update_check_interval_hours: u64,
}

/// State for a single installed binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryState {
    /// Installed version
    pub version: Version,

    /// When this version was installed
    pub installed_at: DateTime<Utc>,

    /// The asset name that was downloaded
    pub asset_name: String,
}

fn default_interval() -> u64 {
    24
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            binaries: HashMap::new(),
            last_update_check: None,
            update_check_interval_hours: default_interval(),
        }
    }
}

impl AppState {
    /// Load state from the state file. Returns default state if file doesn't exist.
    ///
    /// Automatically migrates legacy `iii-cli` state entries to `iii` so that
    /// users upgrading from the old separate CLI binary retain their version
    /// tracking without re-downloading.
    pub fn load(path: &Path) -> Result<Self, StateError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| StateError::ReadFailed(format!("{}: {}", path.display(), e)))?;
        let mut state: Self = serde_json::from_str(&content)?;

        // Migrate legacy iii-cli state entry to iii
        if state.binaries.contains_key("iii-cli") && !state.binaries.contains_key("iii") {
            if let Some(old_entry) = state.binaries.remove("iii-cli") {
                state.binaries.insert("iii".to_string(), old_entry);
            }
        } else if state.binaries.contains_key("iii-cli") {
            // Both exist — remove the legacy entry, keep the newer iii entry
            state.binaries.remove("iii-cli");
        }

        Ok(state)
    }

    /// Save state to the state file using atomic write-to-temp-then-rename.
    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let content = serde_json::to_string_pretty(self)?;
        let temp_path = path.with_extension("json.tmp");

        // Write to temp file first
        std::fs::write(&temp_path, &content)?;

        // Atomic rename
        std::fs::rename(&temp_path, path).inspect_err(|_e| {
            // Clean up temp file on failure
            let _ = std::fs::remove_file(&temp_path);
        })?;

        Ok(())
    }

    /// Check if an update check is due based on the configured interval.
    pub fn is_update_check_due(&self) -> bool {
        match self.last_update_check {
            None => true,
            Some(last) => {
                let elapsed = Utc::now() - last;
                elapsed.num_hours() >= self.update_check_interval_hours as i64
            }
        }
    }

    /// Record a binary installation.
    pub fn record_install(&mut self, binary_name: &str, version: Version, asset_name: String) {
        self.binaries.insert(
            binary_name.to_string(),
            BinaryState {
                version,
                installed_at: Utc::now(),
                asset_name,
            },
        );
    }

    /// Get the installed version of a binary, if any.
    pub fn installed_version(&self, binary_name: &str) -> Option<&Version> {
        self.binaries.get(binary_name).map(|b| &b.version)
    }

    /// Mark the update check as completed.
    pub fn mark_update_checked(&mut self) {
        self.last_update_check = Some(Utc::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_state() {
        let state = AppState::default();
        assert!(state.binaries.is_empty());
        assert!(state.last_update_check.is_none());
        assert_eq!(state.update_check_interval_hours, 24);
    }

    #[test]
    fn test_load_nonexistent_returns_default() {
        let path = Path::new("/tmp/nonexistent-iii-state.json");
        let state = AppState::load(path).unwrap();
        assert!(state.binaries.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let mut state = AppState::default();
        state.record_install(
            "iii-console",
            Version::new(0, 2, 4),
            "iii-console-aarch64-apple-darwin.tar.gz".to_string(),
        );
        state.mark_update_checked();

        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        // Drop the temp file so we can write to the path
        drop(temp);

        state.save(&path).unwrap();
        let loaded = AppState::load(&path).unwrap();

        assert_eq!(loaded.binaries.len(), 1);
        assert_eq!(
            loaded.installed_version("iii-console"),
            Some(&Version::new(0, 2, 4))
        );
        assert!(loaded.last_update_check.is_some());

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_is_update_check_due() {
        let mut state = AppState::default();
        assert!(state.is_update_check_due());

        state.mark_update_checked();
        assert!(!state.is_update_check_due());
    }

    #[test]
    fn test_atomic_write_no_partial() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");

        let state = AppState::default();
        state.save(&path).unwrap();

        // Temp file should not exist after successful save
        let temp_path = path.with_extension("json.tmp");
        assert!(!temp_path.exists());
    }

    #[test]
    fn test_load_migrates_iii_cli_to_iii() {
        // Simulate old state file with "iii-cli" entry
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");

        let mut old_state = AppState::default();
        old_state.record_install(
            "iii-cli",
            Version::new(0, 2, 5),
            "iii-cli-aarch64-apple-darwin.tar.gz".to_string(),
        );
        old_state.save(&path).unwrap();

        // Load should migrate "iii-cli" to "iii"
        let loaded = AppState::load(&path).unwrap();
        assert!(
            loaded.binaries.contains_key("iii"),
            "should have migrated iii-cli to iii"
        );
        assert!(
            !loaded.binaries.contains_key("iii-cli"),
            "should have removed iii-cli entry"
        );
        assert_eq!(
            loaded.installed_version("iii"),
            Some(&Version::new(0, 2, 5))
        );
    }

    #[test]
    fn test_load_removes_iii_cli_when_both_exist() {
        // Simulate state with both "iii-cli" and "iii" entries
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");

        let mut state = AppState::default();
        state.record_install(
            "iii-cli",
            Version::new(0, 2, 5),
            "iii-cli-aarch64-apple-darwin.tar.gz".to_string(),
        );
        state.record_install(
            "iii",
            Version::new(0, 9, 0),
            "iii-aarch64-apple-darwin.tar.gz".to_string(),
        );
        state.save(&path).unwrap();

        // Load should keep "iii" and remove "iii-cli"
        let loaded = AppState::load(&path).unwrap();
        assert_eq!(
            loaded.installed_version("iii"),
            Some(&Version::new(0, 9, 0)),
            "should keep the newer iii entry"
        );
        assert!(
            !loaded.binaries.contains_key("iii-cli"),
            "should remove legacy iii-cli entry"
        );
    }
}
