use std::collections::HashMap;

use serde::Deserialize;

use crate::cli::error::WorkerError;

const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/iii-hq/workers/main/registry/index.json";

#[derive(Debug, Deserialize)]
pub struct RegistryManifest {
    #[allow(dead_code)]
    pub version: u32,
    pub workers: HashMap<String, WorkerEntry>,
}

#[derive(Debug, Deserialize)]
pub struct WorkerEntry {
    pub description: String,
    pub repo: String,
    pub tag_prefix: Option<String>,
    pub supported_targets: Vec<String>,
    #[serde(default)]
    pub has_checksum: bool,
    #[serde(default)]
    pub default_config: Option<serde_json::Value>,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

/// Validate a worker name against the pattern: lowercase alphanumeric with hyphens,
/// 1-64 characters, must start and end with alphanumeric.
pub fn validate_worker_name(name: &str) -> Result<(), WorkerError> {
    if name.is_empty() || name.len() > 64 {
        return Err(WorkerError::InvalidWorkerName {
            name: name.to_string(),
        });
    }

    let bytes = name.as_bytes();

    // First char must be lowercase alphanumeric
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return Err(WorkerError::InvalidWorkerName {
            name: name.to_string(),
        });
    }

    // Last char must be lowercase alphanumeric
    if !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit() {
        return Err(WorkerError::InvalidWorkerName {
            name: name.to_string(),
        });
    }

    // Middle chars must be lowercase alphanumeric or hyphen
    for &b in bytes.iter().skip(1).take(bytes.len().saturating_sub(2)) {
        if !b.is_ascii_lowercase() && !b.is_ascii_digit() && b != b'-' {
            return Err(WorkerError::InvalidWorkerName {
                name: name.to_string(),
            });
        }
    }

    Ok(())
}

/// Fetch the worker registry from the remote URL.
/// Respects the III_REGISTRY_URL environment variable for overriding the default.
/// Supports file:// URLs for local registries (path resolved as-is from the URL).
pub async fn fetch_registry(client: &reqwest::Client) -> Result<RegistryManifest, WorkerError> {
    let url =
        std::env::var("III_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());

    let content = if url.starts_with("file://") {
        let path = url.strip_prefix("file://").unwrap();
        std::fs::read_to_string(path).map_err(|e| WorkerError::RegistryFetchFailed {
            url: url.clone(),
            reason: format!("Failed to read local registry: {}", e),
        })?
    } else {
        let response =
            client
                .get(&url)
                .send()
                .await
                .map_err(|e| WorkerError::RegistryFetchFailed {
                    url: url.clone(),
                    reason: e.to_string(),
                })?;

        if !response.status().is_success() {
            return Err(WorkerError::RegistryFetchFailed {
                url: url.clone(),
                reason: format!("HTTP {}", response.status()),
            });
        }

        response
            .text()
            .await
            .map_err(|e| WorkerError::RegistryFetchFailed {
                url: url.clone(),
                reason: e.to_string(),
            })?
    };

    let manifest: RegistryManifest =
        serde_json::from_str(&content).map_err(|e| WorkerError::RegistryFetchFailed {
            url,
            reason: e.to_string(),
        })?;

    Ok(manifest)
}

impl RegistryManifest {
    /// Resolve a worker name to its entry in the registry.
    pub fn resolve(&self, name: &str) -> Result<&WorkerEntry, WorkerError> {
        self.workers
            .get(name)
            .ok_or_else(|| WorkerError::WorkerNotFound {
                name: name.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    fn sample_json() -> &'static str {
        r#"{
            "version": 1,
            "workers": {
                "pdfkit": {
                    "description": "PDF generation toolkit",
                    "repo": "iii-hq/pdfkit",
                    "tag_prefix": "pdfkit",
                    "supported_targets": ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"],
                    "has_checksum": true,
                    "default_config": {
                        "class": "workers::pdfkit::PdfKitWorker",
                        "config": { "output_dir": "./output" }
                    }
                },
                "image-processor": {
                    "description": "Image processing worker",
                    "repo": "iii-hq/image-processor",
                    "tag_prefix": null,
                    "supported_targets": ["aarch64-apple-darwin"],
                    "has_checksum": false
                }
            }
        }"#
    }

    #[test]
    fn test_registry_manifest_deserialization() {
        let manifest: RegistryManifest = serde_json::from_str(sample_json()).unwrap();
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.workers.len(), 2);
    }

    #[test]
    fn test_worker_entry_fields() {
        let manifest: RegistryManifest = serde_json::from_str(sample_json()).unwrap();
        let pdfkit = manifest.workers.get("pdfkit").unwrap();
        assert_eq!(pdfkit.description, "PDF generation toolkit");
        assert_eq!(pdfkit.repo, "iii-hq/pdfkit");
        assert_eq!(pdfkit.tag_prefix, Some("pdfkit".to_string()));
        assert_eq!(pdfkit.supported_targets.len(), 2);
        assert!(pdfkit.has_checksum);
    }

    #[test]
    fn test_resolve_known_worker() {
        let manifest: RegistryManifest = serde_json::from_str(sample_json()).unwrap();
        let entry = manifest.resolve("pdfkit").unwrap();
        assert_eq!(entry.repo, "iii-hq/pdfkit");
    }

    #[test]
    fn test_resolve_unknown_worker() {
        let manifest: RegistryManifest = serde_json::from_str(sample_json()).unwrap();
        let result = manifest.resolve("nonexistent");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            WorkerError::WorkerNotFound { name } => assert_eq!(name, "nonexistent"),
            _ => panic!("Expected WorkerNotFound, got {:?}", err),
        }
    }

    #[test]
    fn test_validate_worker_name_valid() {
        assert!(validate_worker_name("pdfkit").is_ok());
        assert!(validate_worker_name("image-processor").is_ok());
        assert!(validate_worker_name("a1").is_ok());
    }

    #[test]
    fn test_validate_worker_name_rejects_path_traversal() {
        assert!(validate_worker_name("../evil").is_err());
    }

    #[test]
    fn test_validate_worker_name_rejects_uppercase() {
        assert!(validate_worker_name("Foo").is_err());
    }

    #[test]
    fn test_validate_worker_name_rejects_underscores() {
        assert!(validate_worker_name("foo_bar").is_err());
    }

    #[test]
    fn test_validate_worker_name_rejects_leading_hyphen() {
        assert!(validate_worker_name("-leading").is_err());
    }

    #[test]
    fn test_validate_worker_name_rejects_trailing_hyphen() {
        assert!(validate_worker_name("trailing-").is_err());
    }

    #[test]
    fn test_validate_worker_name_rejects_empty() {
        assert!(validate_worker_name("").is_err());
    }

    #[test]
    fn test_validate_worker_name_rejects_too_long() {
        let long_name = "a".repeat(65);
        assert!(validate_worker_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_worker_name_accepts_max_length() {
        let max_name = "a".repeat(64);
        assert!(validate_worker_name(&max_name).is_ok());
    }

    #[test]
    #[serial]
    fn test_registry_url_env_override() {
        // Verify that III_REGISTRY_URL env var is read (unit test for URL resolution logic)
        let custom_url = "https://example.com/custom-registry.json";
        unsafe { std::env::set_var("III_REGISTRY_URL", custom_url); }
        let url =
            std::env::var("III_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());
        assert_eq!(url, custom_url);
        unsafe { std::env::remove_var("III_REGISTRY_URL"); }

        // Verify fallback to default
        let url =
            std::env::var("III_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());
        assert_eq!(url, DEFAULT_REGISTRY_URL);
    }

    #[test]
    fn test_default_config_some() {
        let manifest: RegistryManifest = serde_json::from_str(sample_json()).unwrap();
        let pdfkit = manifest.workers.get("pdfkit").unwrap();
        assert!(pdfkit.default_config.is_some());
        let dc = pdfkit.default_config.as_ref().unwrap();
        assert_eq!(dc["class"], "workers::pdfkit::PdfKitWorker");
        assert_eq!(dc["config"]["output_dir"], "./output");
    }

    #[test]
    fn test_default_config_none_when_absent() {
        let manifest: RegistryManifest = serde_json::from_str(sample_json()).unwrap();
        let img = manifest.workers.get("image-processor").unwrap();
        assert!(img.default_config.is_none());
    }

    #[test]
    fn test_default_config_null() {
        let json = r#"{
            "version": 1,
            "workers": {
                "nullmod": {
                    "description": "Worker with null config",
                    "repo": "iii-hq/nullmod",
                    "tag_prefix": null,
                    "supported_targets": [],
                    "default_config": null
                }
            }
        }"#;
        let manifest: RegistryManifest = serde_json::from_str(json).unwrap();
        let entry = manifest.workers.get("nullmod").unwrap();
        assert!(entry.default_config.is_none());
    }

    #[test]
    fn test_worker_entry_local_path_deserialization() {
        let json = r#"{
            "version": 1,
            "workers": {
                "local-mod": {
                    "description": "Local worker",
                    "repo": "iii-hq/local-mod",
                    "tag_prefix": null,
                    "supported_targets": ["aarch64-apple-darwin"],
                    "has_checksum": true,
                    "default_config": null,
                    "local_path": "../path/to/binary",
                    "version": "1.2.3"
                }
            }
        }"#;
        let manifest: RegistryManifest = serde_json::from_str(json).unwrap();
        let entry = manifest.workers.get("local-mod").unwrap();
        assert_eq!(entry.local_path.as_deref(), Some("../path/to/binary"));
        assert_eq!(entry.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn test_worker_entry_local_path_defaults_to_none() {
        let manifest: RegistryManifest = serde_json::from_str(sample_json()).unwrap();
        let pdfkit = manifest.workers.get("pdfkit").unwrap();
        assert!(pdfkit.local_path.is_none());
        assert!(pdfkit.version.is_none());
    }

    #[test]
    #[serial]
    fn test_file_protocol_fetch() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry_path = dir.path().join("index.json");
        std::fs::write(&registry_path, sample_json()).unwrap();

        unsafe { std::env::set_var(
            "III_REGISTRY_URL",
            format!("file://{}", registry_path.display()),
        ); }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let result = rt.block_on(fetch_registry(&client));
        unsafe { std::env::remove_var("III_REGISTRY_URL"); }

        let manifest = result.unwrap();
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.workers.len(), 2);
    }

    #[test]
    fn test_default_registry_url_points_to_workers_repo() {
        assert_eq!(
            DEFAULT_REGISTRY_URL,
            "https://raw.githubusercontent.com/iii-hq/workers/main/registry/index.json"
        );
    }

    #[test]
    fn test_worker_entry_no_checksum_default() {
        let json = r#"{
            "version": 1,
            "workers": {
                "simple": {
                    "description": "Simple worker",
                    "repo": "iii-hq/simple",
                    "tag_prefix": null,
                    "supported_targets": []
                }
            }
        }"#;
        let manifest: RegistryManifest = serde_json::from_str(json).unwrap();
        let entry = manifest.workers.get("simple").unwrap();
        assert!(!entry.has_checksum);
    }

    #[test]
    fn test_validate_worker_name_single_character_accepted() {
        // Single-char names are valid: the middle-char loop yields zero
        // elements via saturating_sub, so "a" passes validation.
        assert!(validate_worker_name("a").is_ok());
    }

    #[test]
    fn test_validate_worker_name_digits_only() {
        assert!(validate_worker_name("123").is_ok());
    }

    #[test]
    fn test_validate_worker_name_consecutive_hyphens() {
        assert!(validate_worker_name("foo--bar").is_ok());
    }

    #[test]
    fn test_validate_worker_name_single_digit_accepted() {
        // Single-digit names are valid: same logic as single-char.
        assert!(validate_worker_name("1").is_ok());
    }

    #[test]
    fn test_validate_worker_name_starting_with_digit() {
        assert!(validate_worker_name("1worker").is_ok());
    }

    #[test]
    fn test_validate_worker_name_rejects_special_chars() {
        let result_dot = validate_worker_name("foo.bar");
        assert!(result_dot.is_err());
        match result_dot.unwrap_err() {
            WorkerError::InvalidWorkerName { name } => assert_eq!(name, "foo.bar"),
            other => panic!("Expected InvalidWorkerName, got {:?}", other),
        }

        let result_at = validate_worker_name("foo@bar");
        assert!(result_at.is_err());
        match result_at.unwrap_err() {
            WorkerError::InvalidWorkerName { name } => assert_eq!(name, "foo@bar"),
            other => panic!("Expected InvalidWorkerName, got {:?}", other),
        }

        let result_slash = validate_worker_name("foo/bar");
        assert!(result_slash.is_err());
        match result_slash.unwrap_err() {
            WorkerError::InvalidWorkerName { name } => assert_eq!(name, "foo/bar"),
            other => panic!("Expected InvalidWorkerName, got {:?}", other),
        }
    }

    #[test]
    #[serial]
    fn test_fetch_registry_file_protocol_nonexistent_path() {
        let nonexistent = "file:///tmp/does_not_exist_registry_motia_test/index.json";
        unsafe { std::env::set_var("III_REGISTRY_URL", nonexistent); }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let result = rt.block_on(fetch_registry(&client));
        unsafe { std::env::remove_var("III_REGISTRY_URL"); }

        assert!(result.is_err());
        match result.unwrap_err() {
            WorkerError::RegistryFetchFailed { url, reason } => {
                assert_eq!(url, nonexistent);
                assert!(reason.contains("Failed to read local registry"));
            }
            other => panic!("Expected RegistryFetchFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_returns_correct_entry_fields() {
        let manifest: RegistryManifest = serde_json::from_str(sample_json()).unwrap();
        let entry = manifest.resolve("image-processor").unwrap();
        assert_eq!(entry.description, "Image processing worker");
        assert_eq!(entry.repo, "iii-hq/image-processor");
        assert!(entry.tag_prefix.is_none());
        assert_eq!(entry.supported_targets, vec!["aarch64-apple-darwin"]);
        assert!(!entry.has_checksum);
        assert!(entry.default_config.is_none());
        assert!(entry.local_path.is_none());
        assert!(entry.version.is_none());
    }

    #[test]
    fn test_registry_manifest_empty_workers_map() {
        let json = r#"{
            "version": 1,
            "workers": {}
        }"#;
        let manifest: RegistryManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.version, 1);
        assert!(manifest.workers.is_empty());

        let result = manifest.resolve("anything");
        assert!(result.is_err());
        match result.unwrap_err() {
            WorkerError::WorkerNotFound { name } => assert_eq!(name, "anything"),
            other => panic!("Expected WorkerNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_worker_entry_all_optional_fields_absent() {
        let json = r#"{
            "version": 1,
            "workers": {
                "minimal": {
                    "description": "Minimal worker",
                    "repo": "iii-hq/minimal",
                    "supported_targets": ["x86_64-unknown-linux-gnu"]
                }
            }
        }"#;
        let manifest: RegistryManifest = serde_json::from_str(json).unwrap();
        let entry = manifest.workers.get("minimal").unwrap();
        assert_eq!(entry.description, "Minimal worker");
        assert_eq!(entry.repo, "iii-hq/minimal");
        assert!(entry.tag_prefix.is_none());
        assert_eq!(entry.supported_targets, vec!["x86_64-unknown-linux-gnu"]);
        assert!(!entry.has_checksum);
        assert!(entry.default_config.is_none());
        assert!(entry.local_path.is_none());
        assert!(entry.version.is_none());
    }
}
