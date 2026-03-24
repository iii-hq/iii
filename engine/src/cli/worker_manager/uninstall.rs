use std::path::Path;

use crate::cli::error::WorkerError;
use super::{config, manifest, registry, storage};

#[derive(Debug)]
pub struct UninstallOutcome {
    pub name: String,
    pub binary_removed: bool,
    pub config_removed: bool,
    pub warnings: Vec<String>,
}

pub fn uninstall_worker(
    worker_name: &str,
    project_dir: &Path,
) -> Result<UninstallOutcome, WorkerError> {
    // 1. Validate worker name
    registry::validate_worker_name(worker_name)?;

    // 2. Check worker is installed (must be in iii.toml)
    let existing = manifest::read_manifest(project_dir)?;
    if !existing.contains_key(worker_name) {
        return Err(WorkerError::WorkerNotInstalled {
            name: worker_name.to_string(),
        });
    }

    // 3. Remove binary and version marker (skip if missing)
    let binary_removed = storage::remove_worker_binary(project_dir, worker_name)?;
    let _ = storage::remove_version_marker(project_dir, worker_name);

    // 4. Remove manifest entry
    manifest::remove(project_dir, worker_name)?;

    // 5. Remove config block (collect warning on failure instead of hard error)
    let (config_removed, config_warning) =
        match config::remove_worker_config(project_dir, worker_name) {
            Ok(removed) => (removed, None),
            Err(e) => (
                false,
                Some(format!(
                    "Config removal failed: {}. Binary and manifest were already removed.",
                    e
                )),
            ),
        };

    let mut warnings = Vec::new();
    if let Some(w) = config_warning {
        warnings.push(w);
    }

    Ok(UninstallOutcome {
        name: worker_name.to_string(),
        binary_removed,
        config_removed,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_installed_worker(dir: &Path) {
        // Create iii.toml with pdfkit entry
        fs::write(dir.join("iii.toml"), "[workers]\npdfkit = \"1.0.0\"\n").unwrap();

        // Create binary
        let workers_dir = dir.join("iii_workers");
        fs::create_dir_all(&workers_dir).unwrap();
        fs::write(workers_dir.join("pdfkit"), b"fake binary").unwrap();

        // Create config.yaml with marker block
        let config_content = "workers:\n  # === iii:pdfkit BEGIN ===\n  - class: workers::pdfkit::PdfKitWorker\n    config:\n      output_dir: ./output\n  # === iii:pdfkit END ===\n";
        fs::write(dir.join("config.yaml"), config_content).unwrap();
    }

    #[test]
    fn test_uninstall_full_removes_all_three() {
        let dir = TempDir::new().unwrap();
        setup_installed_worker(dir.path());

        let outcome = uninstall_worker("pdfkit", dir.path()).unwrap();

        assert!(outcome.binary_removed);

        assert!(outcome.config_removed);
        assert!(outcome.warnings.is_empty());
        assert_eq!(outcome.name, "pdfkit");

        // Verify binary is gone
        assert!(!dir.path().join("iii_workers/pdfkit").exists());

        // Verify manifest entry is gone
        let manifest = manifest::read_manifest(dir.path()).unwrap();
        assert!(!manifest.contains_key("pdfkit"));

        // Verify config block is gone
        let config_content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(!config_content.contains("pdfkit"));
    }

    #[test]
    fn test_uninstall_not_installed_returns_error() {
        let dir = TempDir::new().unwrap();
        // Empty manifest
        fs::write(dir.path().join("iii.toml"), "[workers]\n").unwrap();

        let result = uninstall_worker("pdfkit", dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            WorkerError::WorkerNotInstalled { name } => assert_eq!(name, "pdfkit"),
            other => panic!("Expected WorkerNotInstalled, got {:?}", other),
        }
    }

    #[test]
    fn test_uninstall_missing_binary_skips_without_error() {
        let dir = TempDir::new().unwrap();
        // Manifest has entry but no binary
        fs::write(
            dir.path().join("iii.toml"),
            "[workers]\npdfkit = \"1.0.0\"\n",
        )
        .unwrap();
        // No config.yaml either
        let outcome = uninstall_worker("pdfkit", dir.path()).unwrap();

        assert!(!outcome.binary_removed);

        assert!(!outcome.config_removed);
    }

    #[test]
    fn test_uninstall_missing_config_markers_skips_without_error() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("iii.toml"),
            "[workers]\npdfkit = \"1.0.0\"\n",
        )
        .unwrap();
        let workers_dir = dir.path().join("iii_workers");
        fs::create_dir_all(&workers_dir).unwrap();
        fs::write(workers_dir.join("pdfkit"), b"fake binary").unwrap();
        // config.yaml exists but no markers for pdfkit
        fs::write(dir.path().join("config.yaml"), "workers:\n").unwrap();

        let outcome = uninstall_worker("pdfkit", dir.path()).unwrap();

        assert!(outcome.binary_removed);

        assert!(!outcome.config_removed);
    }

    #[test]
    fn test_uninstall_invalid_worker_name_rejected() {
        let dir = TempDir::new().unwrap();
        let result = uninstall_worker("../evil", dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            WorkerError::InvalidWorkerName { .. } => {}
            other => panic!("Expected InvalidWorkerName, got {:?}", other),
        }
    }

    #[test]
    fn test_uninstall_config_failure_returns_warning_not_error() {
        let dir = TempDir::new().unwrap();
        // Set up installed worker with manifest entry but no config
        fs::write(
            dir.path().join("iii.toml"),
            "[workers]\npdfkit = \"1.0.0\"\n",
        )
        .unwrap();
        let workers_dir = dir.path().join("iii_workers");
        fs::create_dir_all(&workers_dir).unwrap();
        fs::write(workers_dir.join("pdfkit"), b"fake binary").unwrap();

        let outcome = uninstall_worker("pdfkit", dir.path()).unwrap();
        // Config not found is not an error, just config_removed = false
        assert!(outcome.binary_removed);

        assert!(!outcome.config_removed);
        // No warnings expected in this case (config simply not found)
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn test_uninstall_with_multiple_workers_leaves_others_intact() {
        let dir = TempDir::new().unwrap();

        // Manifest with two workers
        fs::write(
            dir.path().join("iii.toml"),
            "[workers]\npdfkit = \"1.0.0\"\nimgconv = \"2.3.0\"\n",
        )
        .unwrap();

        // Binaries for both workers
        let workers_dir = dir.path().join("iii_workers");
        fs::create_dir_all(&workers_dir).unwrap();
        fs::write(workers_dir.join("pdfkit"), b"fake pdfkit binary").unwrap();
        fs::write(workers_dir.join("imgconv"), b"fake imgconv binary").unwrap();

        // Config with blocks for both workers
        let config_content = "\
workers:\n  \
# === iii:pdfkit BEGIN ===\n  \
- class: workers::pdfkit::PdfKitWorker\n    \
config:\n      \
output_dir: ./output\n  \
# === iii:pdfkit END ===\n  \
# === iii:imgconv BEGIN ===\n  \
- class: workers::imgconv::ImgConvWorker\n    \
config:\n      \
format: png\n  \
# === iii:imgconv END ===\n";
        fs::write(dir.path().join("config.yaml"), config_content).unwrap();

        // Uninstall only pdfkit
        let outcome = uninstall_worker("pdfkit", dir.path()).unwrap();
        assert!(outcome.binary_removed);
        assert!(outcome.config_removed);

        // imgconv must still be in the manifest
        let manifest = manifest::read_manifest(dir.path()).unwrap();
        assert!(!manifest.contains_key("pdfkit"));
        assert!(manifest.contains_key("imgconv"));
        assert_eq!(manifest["imgconv"], "2.3.0");

        // imgconv binary must still exist
        assert!(workers_dir.join("imgconv").exists());
        assert!(!workers_dir.join("pdfkit").exists());

        // imgconv config block must still exist
        let remaining_config = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(!remaining_config.contains("pdfkit"));
        assert!(remaining_config.contains("# === iii:imgconv BEGIN ==="));
        assert!(remaining_config.contains("# === iii:imgconv END ==="));
        assert!(remaining_config.contains("imgconv::ImgConvWorker"));
    }

    #[test]
    fn test_uninstall_removes_binary_but_preserves_other_binaries() {
        let dir = TempDir::new().unwrap();

        // Manifest with two workers
        fs::write(
            dir.path().join("iii.toml"),
            "[workers]\npdfkit = \"1.0.0\"\nocr = \"0.5.0\"\n",
        )
        .unwrap();

        // Binaries for both workers
        let workers_dir = dir.path().join("iii_workers");
        fs::create_dir_all(&workers_dir).unwrap();
        fs::write(workers_dir.join("pdfkit"), b"pdfkit bytes").unwrap();
        fs::write(workers_dir.join("ocr"), b"ocr bytes").unwrap();

        let outcome = uninstall_worker("pdfkit", dir.path()).unwrap();
        assert!(outcome.binary_removed);

        // Target binary is gone
        assert!(!workers_dir.join("pdfkit").exists());

        // Other binary is untouched and has original contents
        assert!(workers_dir.join("ocr").exists());
        let ocr_content = fs::read(workers_dir.join("ocr")).unwrap();
        assert_eq!(ocr_content, b"ocr bytes");
    }

    #[test]
    fn test_uninstall_config_with_multiple_blocks_removes_only_target() {
        let dir = TempDir::new().unwrap();

        fs::write(
            dir.path().join("iii.toml"),
            "[workers]\nalpha = \"1.0.0\"\nbeta = \"2.0.0\"\n",
        )
        .unwrap();

        let workers_dir = dir.path().join("iii_workers");
        fs::create_dir_all(&workers_dir).unwrap();
        fs::write(workers_dir.join("alpha"), b"alpha bin").unwrap();

        let config_content = "\
workers:\n  \
# === iii:alpha BEGIN ===\n  \
- class: workers::alpha::AlphaWorker\n    \
config:\n      \
key: alpha_val\n  \
# === iii:alpha END ===\n  \
# === iii:beta BEGIN ===\n  \
- class: workers::beta::BetaWorker\n    \
config:\n      \
key: beta_val\n  \
# === iii:beta END ===\n";
        fs::write(dir.path().join("config.yaml"), config_content).unwrap();

        let outcome = uninstall_worker("alpha", dir.path()).unwrap();
        assert!(outcome.config_removed);

        let remaining = fs::read_to_string(dir.path().join("config.yaml")).unwrap();

        // Alpha block fully removed
        assert!(!remaining.contains("# === iii:alpha BEGIN ==="));
        assert!(!remaining.contains("# === iii:alpha END ==="));
        assert!(!remaining.contains("alpha_val"));

        // Beta block fully preserved
        assert!(remaining.contains("# === iii:beta BEGIN ==="));
        assert!(remaining.contains("# === iii:beta END ==="));
        assert!(remaining.contains("beta_val"));
        assert!(remaining.contains("workers::beta::BetaWorker"));
    }

    #[test]
    fn test_uninstall_no_manifest_file_returns_worker_not_installed() {
        let dir = TempDir::new().unwrap();
        // No iii.toml created at all

        let result = uninstall_worker("pdfkit", dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            WorkerError::WorkerNotInstalled { name } => assert_eq!(name, "pdfkit"),
            other => panic!("Expected WorkerNotInstalled, got {:?}", other),
        }
    }

    #[test]
    fn test_uninstall_outcome_name_matches_input() {
        let dir = TempDir::new().unwrap();
        setup_installed_worker(dir.path());

        let outcome = uninstall_worker("pdfkit", dir.path()).unwrap();
        assert_eq!(outcome.name, "pdfkit");

        // Also verify with a different worker name
        let dir2 = TempDir::new().unwrap();
        fs::write(
            dir2.path().join("iii.toml"),
            "[workers]\nmyworker = \"3.1.0\"\n",
        )
        .unwrap();
        let workers_dir = dir2.path().join("iii_workers");
        fs::create_dir_all(&workers_dir).unwrap();
        fs::write(workers_dir.join("myworker"), b"bin").unwrap();

        let config_content = "\
workers:\n  \
# === iii:myworker BEGIN ===\n  \
- class: workers::myworker::MyWorker\n    \
config:\n      \
enabled: true\n  \
# === iii:myworker END ===\n";
        fs::write(dir2.path().join("config.yaml"), config_content).unwrap();

        let outcome2 = uninstall_worker("myworker", dir2.path()).unwrap();
        assert_eq!(outcome2.name, "myworker");
    }
}
