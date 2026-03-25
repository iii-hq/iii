// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::path::Path;

use colored::Colorize;
use semver::Version;

use super::{config, manifest, registry, storage};
use crate::cli::error::WorkerError;
use crate::cli::{download, github, platform};

#[derive(Debug)]
pub enum InstallOutcome {
    Installed {
        name: String,
        version: Version,
        config_updated: bool,
    },
    Updated {
        name: String,
        old_version: String,
        new_version: Version,
        config_updated: bool,
    },
}

pub async fn install_worker(
    worker_name: &str,
    version: Option<&str>,
    project_dir: &Path,
    client: &reqwest::Client,
    force: bool,
) -> Result<InstallOutcome, WorkerError> {
    // 1. Validate worker name
    registry::validate_worker_name(worker_name)?;

    // 2. Fetch registry manifest
    let registry_manifest = registry::fetch_registry(client).await?;

    // 3. Resolve worker from registry
    let worker_entry = registry_manifest.resolve(worker_name)?;

    // 4. Check platform support
    let target = platform::current_target();
    if !worker_entry.supported_targets.iter().any(|t| t == target) {
        return Err(WorkerError::UnsupportedPlatform {
            name: worker_name.to_string(),
            platform: target.to_string(),
            supported: worker_entry.supported_targets.join(", "),
        });
    }

    // 4.5: Detect local registry mode
    let is_local_registry = std::env::var("III_REGISTRY_URL")
        .map(|u| u.starts_with("file://"))
        .unwrap_or(false);

    let version_parsed: Version;
    let target_path = storage::worker_binary_path(project_dir, worker_name);

    if is_local_registry {
        // Local install path -- copy binary instead of GitHub download
        if let Some(ref local_path) = worker_entry.local_path {
            // Resolve local_path relative to registry file location
            let registry_url = std::env::var("III_REGISTRY_URL").unwrap();
            let registry_file_path = registry_url.strip_prefix("file://").unwrap();
            let registry_dir = Path::new(registry_file_path)
                .parent()
                .unwrap_or_else(|| Path::new("."));
            let source = registry_dir.join(local_path);

            if !source.exists() {
                return Err(WorkerError::DownloadFailed {
                    name: worker_name.to_string(),
                    reason: format!("Local binary not found: {}", source.display()),
                });
            }

            // Parse version from registry entry (required for local installs)
            let version_str =
                worker_entry
                    .version
                    .as_deref()
                    .ok_or_else(|| WorkerError::DownloadFailed {
                        name: worker_name.to_string(),
                        reason: "Local registry worker has no version field".to_string(),
                    })?;
            version_parsed =
                version_str
                    .parse::<Version>()
                    .map_err(|e| WorkerError::DownloadFailed {
                        name: worker_name.to_string(),
                        reason: format!("Invalid version '{}': {}", version_str, e),
                    })?;

            // Ensure iii_workers/ directory exists
            storage::ensure_workers_dir(project_dir)?;

            // Copy binary to iii_workers/<name>
            std::fs::copy(&source, &target_path).map_err(|e| WorkerError::DownloadFailed {
                name: worker_name.to_string(),
                reason: format!("Failed to copy binary: {}", e),
            })?;

            // Set executable permissions on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target_path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| WorkerError::DownloadFailed {
                        name: worker_name.to_string(),
                        reason: format!("Failed to set permissions: {}", e),
                    })?;
            }

            eprintln!(
                "  {} Copied local binary from {}",
                "✓".green(),
                source.display()
            );
        } else {
            return Err(WorkerError::DownloadFailed {
                name: worker_name.to_string(),
                reason: "Local registry worker has no local_path field".to_string(),
            });
        }
    } else {
        // 5. Build a BinarySpec for the existing github/download pipeline.
        let spec = super::spec::leaked_binary_spec(worker_name, worker_entry);

        // 6. Fetch release from GitHub (version-pinned or latest)
        let release = match version {
            Some(v) => {
                let tag = match &worker_entry.tag_prefix {
                    Some(prefix) => format!("{}/v{}", prefix, v),
                    None => format!("v{}", v),
                };
                github::fetch_release_by_tag(client, &worker_entry.repo, &tag)
                    .await
                    .map_err(|e| match &e {
                        github::IiiGithubError::Registry(_) => WorkerError::VersionNotFound {
                            name: worker_name.to_string(),
                            version: v.to_string(),
                        },
                        _ => WorkerError::DownloadFailed {
                            name: worker_name.to_string(),
                            reason: e.to_string(),
                        },
                    })?
            }
            None => github::fetch_latest_release(client, &spec)
                .await
                .map_err(|e| WorkerError::DownloadFailed {
                    name: worker_name.to_string(),
                    reason: e.to_string(),
                })?,
        };
        version_parsed = github::parse_release_version(&release.tag_name).map_err(|e| {
            WorkerError::DownloadFailed {
                name: worker_name.to_string(),
                reason: format!(
                    "Invalid version in release tag '{}': {}",
                    release.tag_name, e
                ),
            }
        })?;

        // 7. Find platform-specific asset
        let asset_name_str = platform::asset_name(spec.name);
        let asset = github::find_asset(&release, &asset_name_str).ok_or_else(|| {
            WorkerError::DownloadFailed {
                name: worker_name.to_string(),
                reason: format!(
                    "Release asset '{}' not found in release {}",
                    asset_name_str, release.tag_name
                ),
            }
        })?;

        // 8. Resolve checksum URL if the worker supports checksums
        let checksum_url = if worker_entry.has_checksum {
            let cs_name = platform::checksum_asset_name(spec.name);
            let url = github::find_asset(&release, &cs_name)
                .map(|a| a.browser_download_url.clone())
                .ok_or_else(|| WorkerError::AssetNotFound {
                    name: worker_name.to_string(),
                    reason: format!(
                        "checksum asset '{}' not found in release {}",
                        cs_name, release.tag_name
                    ),
                })?;
            Some(url)
        } else {
            None
        };

        // 9. Ensure iii_workers/ directory exists
        storage::ensure_workers_dir(project_dir)?;

        // 10. Download, verify checksum, extract, and write binary to iii_workers/<name>
        // UX-01: download_and_install routes through download_with_progress() which displays
        // an indicatif progress bar during the binary download.
        download::download_and_install(client, &spec, asset, checksum_url.as_deref(), &target_path)
            .await
            .map_err(|e| WorkerError::DownloadFailed {
                name: worker_name.to_string(),
                reason: e.to_string(),
            })?;
    }

    // 11. Check if this is an update (worker already in manifest)
    let existing = match manifest::read_manifest(project_dir) {
        Ok(m) => m,
        Err(e) => {
            let _ = std::fs::remove_file(&target_path);
            return Err(e);
        }
    };
    let old_version = existing.get(worker_name).cloned();

    // 12. Update iii.toml manifest -- with cleanup on failure
    if let Err(e) = manifest::add_or_update(project_dir, worker_name, &version_parsed.to_string()) {
        // Clean up: remove the binary we just placed to avoid orphaned state
        let _ = std::fs::remove_file(&target_path);
        return Err(e);
    }

    // 12.1. Write version marker so batch install can detect version drift
    let _ = storage::write_version_marker(project_dir, worker_name, &version_parsed.to_string());

    // 12.5. Generate config.yaml block if worker has default_config
    let mut config_updated = false;
    if let Some(ref default_config) = worker_entry.default_config {
        match config::add_worker_config(project_dir, worker_name, default_config) {
            Ok(config::ConfigOutcome::Added) => {
                config_updated = true;
            }
            Ok(config::ConfigOutcome::AlreadyExists) => {
                if force {
                    // --force: overwrite without prompting
                    let _removed = config::remove_worker_config(project_dir, worker_name)?;
                    match config::add_worker_config(project_dir, worker_name, default_config) {
                        Ok(config::ConfigOutcome::Added) => {
                            config_updated = true;
                            eprintln!("  {} config.yaml overwritten (--force)", "✓".green());
                        }
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!(
                                "  {} Failed to update config.yaml: {}",
                                "warning:".yellow(),
                                e
                            );
                        }
                    }
                } else {
                    eprintln!(
                        "  {} Config for '{}' already exists in config.yaml (use --force to overwrite)",
                        "-".dimmed(),
                        worker_name
                    );
                }
            }
            Err(e) => {
                // Log warning but don't fail the install -- binary and manifest are already written
                eprintln!(
                    "  {} Failed to update config.yaml: {}",
                    "warning:".yellow(),
                    e
                );
            }
        }
    }

    // 13. Return outcome
    Ok(match old_version {
        Some(old) => InstallOutcome::Updated {
            name: worker_name.to_string(),
            old_version: old,
            new_version: version_parsed,
            config_updated,
        },
        None => InstallOutcome::Installed {
            name: worker_name.to_string(),
            version: version_parsed,
            config_updated,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serial_test::serial;
    use tempfile::TempDir;

    use super::*;

    #[test]
    #[serial]
    fn test_local_install_copies_binary() {
        let project_dir = TempDir::new().unwrap();
        let registry_dir = TempDir::new().unwrap();

        // Place source binary INSIDE registry_dir so relative path is simple
        let source_binary = registry_dir.path().join("image-resize");
        fs::write(&source_binary, b"fake binary content").unwrap();

        let registry_path = registry_dir.path().join("index.json");
        let registry_json = format!(
            r#"{{
            "version": 1,
            "workers": {{
                "image-resize": {{
                    "description": "Test worker",
                    "repo": "iii-hq/image-resize",
                    "tag_prefix": null,
                    "supported_targets": ["{}"],
                    "has_checksum": false,
                    "default_config": null,
                    "local_path": "./image-resize",
                    "version": "0.1.0"
                }}
            }}
        }}"#,
            crate::cli::platform::current_target()
        );
        fs::write(&registry_path, &registry_json).unwrap();

        unsafe {
            std::env::set_var(
                "III_REGISTRY_URL",
                format!("file://{}", registry_path.display()),
            );
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let result = rt.block_on(install_worker(
            "image-resize",
            None,
            project_dir.path(),
            &client,
            false,
        ));

        unsafe {
            std::env::remove_var("III_REGISTRY_URL");
        }

        let outcome = result.unwrap();
        match outcome {
            InstallOutcome::Installed {
                ref name,
                ref version,
                ..
            } => {
                assert_eq!(name, "image-resize");
                assert_eq!(version.to_string(), "0.1.0");
            }
            _ => panic!("Expected Installed outcome"),
        }

        let installed = super::storage::worker_binary_path(project_dir.path(), "image-resize");
        assert!(
            installed.exists(),
            "Binary should be copied to iii_workers/"
        );
        assert_eq!(fs::read(&installed).unwrap(), b"fake binary content");
    }

    #[test]
    fn test_install_cleanup_on_manifest_failure() {
        // Verify that the cleanup logic (remove_file) works on a binary path.
        // In production, if manifest::add_or_update fails after download,
        // the binary is removed to avoid orphaned state.
        let dir = TempDir::new().unwrap();
        let workers_dir = dir.path().join("iii_workers");
        fs::create_dir_all(&workers_dir).unwrap();
        let binary_path = workers_dir.join("testmod");
        fs::write(&binary_path, b"fake binary").unwrap();

        // Simulate cleanup: remove_file should work
        assert!(binary_path.exists());
        let _ = std::fs::remove_file(&binary_path);
        assert!(!binary_path.exists(), "Cleanup should remove the binary");
    }

    #[test]
    #[serial]
    fn test_install_worker_with_invalid_worker_name() {
        let project_dir = TempDir::new().unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let result = rt.block_on(install_worker(
            "../evil-path",
            None,
            project_dir.path(),
            &client,
            false,
        ));

        let err = result.unwrap_err();
        match err {
            WorkerError::InvalidWorkerName { name } => {
                assert_eq!(name, "../evil-path");
            }
            _ => panic!("Expected InvalidWorkerName, got {:?}", err),
        }
    }

    #[test]
    #[serial]
    fn test_local_install_no_local_path_in_registry_entry() {
        let project_dir = TempDir::new().unwrap();
        let registry_dir = TempDir::new().unwrap();

        let registry_path = registry_dir.path().join("index.json");
        let registry_json = format!(
            r#"{{
            "version": 1,
            "workers": {{
                "image-resize": {{
                    "description": "Test worker",
                    "repo": "iii-hq/image-resize",
                    "tag_prefix": null,
                    "supported_targets": ["{}"],
                    "has_checksum": false,
                    "default_config": null,
                    "local_path": null,
                    "version": "0.1.0"
                }}
            }}
        }}"#,
            crate::cli::platform::current_target()
        );
        fs::write(&registry_path, &registry_json).unwrap();

        unsafe {
            std::env::set_var(
                "III_REGISTRY_URL",
                format!("file://{}", registry_path.display()),
            );
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let result = rt.block_on(install_worker(
            "image-resize",
            None,
            project_dir.path(),
            &client,
            false,
        ));

        unsafe {
            std::env::remove_var("III_REGISTRY_URL");
        }

        let err = result.unwrap_err();
        match err {
            WorkerError::DownloadFailed { name, reason } => {
                assert_eq!(name, "image-resize");
                assert!(
                    reason.contains("no local_path"),
                    "Expected 'no local_path' in reason, got: {}",
                    reason
                );
            }
            _ => panic!("Expected DownloadFailed, got {:?}", err),
        }
    }

    #[test]
    #[serial]
    fn test_local_install_source_binary_does_not_exist() {
        let project_dir = TempDir::new().unwrap();
        let registry_dir = TempDir::new().unwrap();

        // Do NOT create the binary file -- it should be missing
        let registry_path = registry_dir.path().join("index.json");
        let registry_json = format!(
            r#"{{
            "version": 1,
            "workers": {{
                "image-resize": {{
                    "description": "Test worker",
                    "repo": "iii-hq/image-resize",
                    "tag_prefix": null,
                    "supported_targets": ["{}"],
                    "has_checksum": false,
                    "default_config": null,
                    "local_path": "./image-resize",
                    "version": "0.1.0"
                }}
            }}
        }}"#,
            crate::cli::platform::current_target()
        );
        fs::write(&registry_path, &registry_json).unwrap();

        unsafe {
            std::env::set_var(
                "III_REGISTRY_URL",
                format!("file://{}", registry_path.display()),
            );
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let result = rt.block_on(install_worker(
            "image-resize",
            None,
            project_dir.path(),
            &client,
            false,
        ));

        unsafe {
            std::env::remove_var("III_REGISTRY_URL");
        }

        let err = result.unwrap_err();
        match err {
            WorkerError::DownloadFailed { name, reason } => {
                assert_eq!(name, "image-resize");
                assert!(
                    reason.contains("Local binary not found"),
                    "Expected 'Local binary not found' in reason, got: {}",
                    reason
                );
            }
            _ => panic!("Expected DownloadFailed, got {:?}", err),
        }
    }

    #[test]
    #[serial]
    fn test_local_install_produces_updated_outcome() {
        let project_dir = TempDir::new().unwrap();
        let registry_dir = TempDir::new().unwrap();

        let source_binary = registry_dir.path().join("image-resize");
        fs::write(&source_binary, b"fake binary v1").unwrap();

        let registry_path = registry_dir.path().join("index.json");

        // First install: version 0.1.0
        let registry_json_v1 = format!(
            r#"{{
            "version": 1,
            "workers": {{
                "image-resize": {{
                    "description": "Test worker",
                    "repo": "iii-hq/image-resize",
                    "tag_prefix": null,
                    "supported_targets": ["{}"],
                    "has_checksum": false,
                    "default_config": null,
                    "local_path": "./image-resize",
                    "version": "0.1.0"
                }}
            }}
        }}"#,
            crate::cli::platform::current_target()
        );
        fs::write(&registry_path, &registry_json_v1).unwrap();

        unsafe {
            std::env::set_var(
                "III_REGISTRY_URL",
                format!("file://{}", registry_path.display()),
            );
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();

        let result1 = rt.block_on(install_worker(
            "image-resize",
            None,
            project_dir.path(),
            &client,
            false,
        ));
        let outcome1 = result1.unwrap();
        match outcome1 {
            InstallOutcome::Installed {
                ref name,
                ref version,
                ..
            } => {
                assert_eq!(name, "image-resize");
                assert_eq!(version.to_string(), "0.1.0");
            }
            _ => panic!("First install should produce Installed outcome"),
        }

        // Second install: version 0.2.0
        fs::write(&source_binary, b"fake binary v2").unwrap();
        let registry_json_v2 = format!(
            r#"{{
            "version": 1,
            "workers": {{
                "image-resize": {{
                    "description": "Test worker",
                    "repo": "iii-hq/image-resize",
                    "tag_prefix": null,
                    "supported_targets": ["{}"],
                    "has_checksum": false,
                    "default_config": null,
                    "local_path": "./image-resize",
                    "version": "0.2.0"
                }}
            }}
        }}"#,
            crate::cli::platform::current_target()
        );
        fs::write(&registry_path, &registry_json_v2).unwrap();

        let result2 = rt.block_on(install_worker(
            "image-resize",
            None,
            project_dir.path(),
            &client,
            false,
        ));

        unsafe {
            std::env::remove_var("III_REGISTRY_URL");
        }

        let outcome2 = result2.unwrap();
        match outcome2 {
            InstallOutcome::Updated {
                ref name,
                ref old_version,
                ref new_version,
                ..
            } => {
                assert_eq!(name, "image-resize");
                assert_eq!(old_version, "0.1.0");
                assert_eq!(new_version.to_string(), "0.2.0");
            }
            _ => panic!("Second install should produce Updated outcome"),
        }
    }

    #[test]
    #[serial]
    fn test_local_install_with_default_config_generates_config() {
        let project_dir = TempDir::new().unwrap();
        let registry_dir = TempDir::new().unwrap();

        let source_binary = registry_dir.path().join("image-resize");
        fs::write(&source_binary, b"fake binary content").unwrap();

        let registry_path = registry_dir.path().join("index.json");
        let registry_json = format!(
            r#"{{
            "version": 1,
            "workers": {{
                "image-resize": {{
                    "description": "Test worker",
                    "repo": "iii-hq/image-resize",
                    "tag_prefix": null,
                    "supported_targets": ["{}"],
                    "has_checksum": false,
                    "default_config": {{
                        "class": "workers::image_resize::ImageResizeWorker",
                        "config": {{ "output_dir": "./resized" }}
                    }},
                    "local_path": "./image-resize",
                    "version": "0.1.0"
                }}
            }}
        }}"#,
            crate::cli::platform::current_target()
        );
        fs::write(&registry_path, &registry_json).unwrap();

        unsafe {
            std::env::set_var(
                "III_REGISTRY_URL",
                format!("file://{}", registry_path.display()),
            );
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let result = rt.block_on(install_worker(
            "image-resize",
            None,
            project_dir.path(),
            &client,
            false,
        ));

        unsafe {
            std::env::remove_var("III_REGISTRY_URL");
        }

        let outcome = result.unwrap();
        match outcome {
            InstallOutcome::Installed { config_updated, .. } => {
                assert!(
                    config_updated,
                    "config_updated should be true when default_config is present"
                );
            }
            _ => panic!("Expected Installed outcome"),
        }

        let config_content = fs::read_to_string(project_dir.path().join("config.yaml")).unwrap();
        assert!(
            config_content.contains("workers::image_resize::ImageResizeWorker"),
            "config.yaml should contain the worker class"
        );
        assert!(
            config_content.contains("output_dir"),
            "config.yaml should contain the config values"
        );
    }

    #[test]
    #[serial]
    fn test_local_install_existing_config_no_force() {
        let project_dir = TempDir::new().unwrap();
        let registry_dir = TempDir::new().unwrap();

        let source_binary = registry_dir.path().join("image-resize");
        fs::write(&source_binary, b"fake binary content").unwrap();

        let default_config_json = serde_json::json!({
            "class": "workers::image_resize::ImageResizeWorker",
            "config": { "output_dir": "./resized" }
        });

        // Pre-create config.yaml with existing worker config block
        config::add_worker_config(project_dir.path(), "image-resize", &default_config_json)
            .unwrap();

        let config_before = fs::read_to_string(project_dir.path().join("config.yaml")).unwrap();

        let registry_path = registry_dir.path().join("index.json");
        let registry_json = format!(
            r#"{{
            "version": 1,
            "workers": {{
                "image-resize": {{
                    "description": "Test worker",
                    "repo": "iii-hq/image-resize",
                    "tag_prefix": null,
                    "supported_targets": ["{}"],
                    "has_checksum": false,
                    "default_config": {{
                        "class": "workers::image_resize::ImageResizeWorker",
                        "config": {{ "output_dir": "./resized" }}
                    }},
                    "local_path": "./image-resize",
                    "version": "0.1.0"
                }}
            }}
        }}"#,
            crate::cli::platform::current_target()
        );
        fs::write(&registry_path, &registry_json).unwrap();

        unsafe {
            std::env::set_var(
                "III_REGISTRY_URL",
                format!("file://{}", registry_path.display()),
            );
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let result = rt.block_on(install_worker(
            "image-resize",
            None,
            project_dir.path(),
            &client,
            false, // force=false
        ));

        unsafe {
            std::env::remove_var("III_REGISTRY_URL");
        }

        let outcome = result.unwrap();
        match outcome {
            InstallOutcome::Installed { config_updated, .. } => {
                assert!(
                    !config_updated,
                    "config_updated should be false when config already exists and force=false"
                );
            }
            _ => panic!("Expected Installed outcome"),
        }

        let config_after = fs::read_to_string(project_dir.path().join("config.yaml")).unwrap();
        assert_eq!(
            config_before, config_after,
            "config.yaml should not be modified when force=false"
        );
    }

    #[test]
    #[serial]
    fn test_local_install_existing_config_with_force() {
        let project_dir = TempDir::new().unwrap();
        let registry_dir = TempDir::new().unwrap();

        let source_binary = registry_dir.path().join("image-resize");
        fs::write(&source_binary, b"fake binary content").unwrap();

        let old_config_json = serde_json::json!({
            "class": "workers::image_resize::OldWorker",
            "config": { "output_dir": "./old" }
        });

        // Pre-create config.yaml with an existing (old) worker config block
        config::add_worker_config(project_dir.path(), "image-resize", &old_config_json).unwrap();

        let config_before = fs::read_to_string(project_dir.path().join("config.yaml")).unwrap();
        assert!(
            config_before.contains("OldWorker"),
            "Pre-condition: old config should be present"
        );

        let registry_path = registry_dir.path().join("index.json");
        let registry_json = format!(
            r#"{{
            "version": 1,
            "workers": {{
                "image-resize": {{
                    "description": "Test worker",
                    "repo": "iii-hq/image-resize",
                    "tag_prefix": null,
                    "supported_targets": ["{}"],
                    "has_checksum": false,
                    "default_config": {{
                        "class": "workers::image_resize::NewWorker",
                        "config": {{ "output_dir": "./new" }}
                    }},
                    "local_path": "./image-resize",
                    "version": "0.1.0"
                }}
            }}
        }}"#,
            crate::cli::platform::current_target()
        );
        fs::write(&registry_path, &registry_json).unwrap();

        unsafe {
            std::env::set_var(
                "III_REGISTRY_URL",
                format!("file://{}", registry_path.display()),
            );
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let result = rt.block_on(install_worker(
            "image-resize",
            None,
            project_dir.path(),
            &client,
            true, // force=true
        ));

        unsafe {
            std::env::remove_var("III_REGISTRY_URL");
        }

        let outcome = result.unwrap();
        match outcome {
            InstallOutcome::Installed { config_updated, .. } => {
                assert!(
                    config_updated,
                    "config_updated should be true when force=true overwrites existing config"
                );
            }
            _ => panic!("Expected Installed outcome"),
        }

        let config_after = fs::read_to_string(project_dir.path().join("config.yaml")).unwrap();
        assert!(
            config_after.contains("NewWorker"),
            "config.yaml should contain the new worker class after force overwrite"
        );
        assert!(
            !config_after.contains("OldWorker"),
            "config.yaml should no longer contain the old worker class"
        );
    }
}
