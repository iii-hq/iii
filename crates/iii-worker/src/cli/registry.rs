// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! OCI registry resolution for worker images.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

pub const MANIFEST_PATH: &str = "/iii/worker.yaml";

const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/iii-hq/workers/main/registry/index.json";

/// Shared HTTP client for registry and download operations.
/// Reuses connections and TLS sessions across requests.
pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerType {
    Binary,
    Managed,
}

#[derive(Debug, Deserialize)]
pub struct RegistryV2Entry {
    #[allow(dead_code)]
    pub description: String,
    #[serde(rename = "type")]
    pub worker_type: Option<WorkerType>,

    // OCI/managed fields (backward compat)
    pub image: Option<String>,
    pub latest: Option<String>,

    // Binary worker fields
    pub repo: Option<String>,
    pub tag_prefix: Option<String>,
    pub supported_targets: Option<Vec<String>>,
    pub has_checksum: Option<bool>,
    pub default_config: Option<serde_json::Value>,
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegistryV2 {
    #[allow(dead_code)]
    pub version: u32,
    pub workers: HashMap<String, RegistryV2Entry>,
}

/// Validates that a worker name is safe for use in filesystem paths and YAML content.
/// Allowed characters: alphanumeric, dash, underscore, dot. Must not be empty or contain `..`.
pub fn validate_worker_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Worker name cannot be empty".into());
    }
    if name.contains("..") {
        return Err(format!("Worker name '{}' contains '..' sequence", name));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!(
            "Worker name '{}' contains invalid characters. Only alphanumeric, dash, underscore, and dot are allowed.",
            name
        ));
    }
    Ok(())
}

/// Validates that a repo string matches the `owner/repo` format with no traversal.
pub fn validate_repo(repo: &str) -> Result<(), String> {
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid repo format '{}': expected 'owner/repo'",
            repo
        ));
    }
    for part in &parts {
        if part.is_empty() || part.contains("..") {
            return Err(format!(
                "Invalid repo format '{}': segments must be non-empty and cannot contain '..'",
                repo
            ));
        }
    }
    Ok(())
}

/// Parse "name@version" into (name, Some(version)) or just (name, None).
pub fn parse_worker_input(input: &str) -> (String, Option<String>) {
    if let Some((name, version)) = input.split_once('@') {
        (name.to_string(), Some(version.to_string()))
    } else {
        (input.to_string(), None)
    }
}

pub async fn fetch_registry() -> Result<RegistryV2, String> {
    let url =
        std::env::var("III_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());

    let body = if url.starts_with("file://") {
        #[cfg(not(debug_assertions))]
        {
            return Err(
                "file:// registry URLs are only supported in debug/test builds. \
                 Set III_REGISTRY_URL to an HTTPS URL."
                    .to_string(),
            );
        }
        #[cfg(debug_assertions)]
        {
            let path = url.strip_prefix("file://").unwrap();
            std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read local registry at {}: {}", path, e))?
        }
    } else {
        let resp = HTTP_CLIENT
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch registry: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("Registry returned HTTP {}", resp.status()));
        }
        resp.text()
            .await
            .map_err(|e| format!("Failed to read registry body: {}", e))?
    };

    serde_json::from_str(&body).map_err(|e| format!("Failed to parse registry: {}", e))
}

pub async fn resolve_image(input: &str) -> Result<(String, String), String> {
    if input.contains('/') || input.contains(':') {
        let name = input
            .rsplit('/')
            .next()
            .unwrap_or(input)
            .split(':')
            .next()
            .unwrap_or(input);
        return Ok((input.to_string(), name.to_string()));
    }

    let registry = fetch_registry().await?;
    let entry = registry
        .workers
        .get(input)
        .ok_or_else(|| format!("Worker '{}' not found in registry", input))?;

    let image = entry.image.as_ref().ok_or_else(|| {
        format!(
            "Worker '{}' has no image field (may be a binary worker)",
            input
        )
    })?;
    let latest = entry
        .latest
        .as_ref()
        .ok_or_else(|| format!("Worker '{}' has no latest version", input))?;
    let image_ref = format!("{}:{}", image, latest);
    Ok((image_ref, input.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize tests that mutate the III_REGISTRY_URL env var to prevent races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[tokio::test]
    async fn resolve_image_full_ref_passthrough() {
        let (image, name) = resolve_image("ghcr.io/iii-hq/image-resize:0.1.2")
            .await
            .unwrap();
        assert_eq!(image, "ghcr.io/iii-hq/image-resize:0.1.2");
        assert_eq!(name, "image-resize");
    }

    #[tokio::test]
    async fn resolve_image_shorthand_uses_registry() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("registry.json");
        let registry_json = r#"{"version": 2, "workers": {"image-resize": {"description": "Resize images", "image": "ghcr.io/iii-hq/image-resize", "latest": "0.1.2"}}}"#;
        std::fs::write(&registry_path, registry_json).unwrap();

        let url = format!("file://{}", registry_path.display());
        let result = {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: guarded by ENV_LOCK to prevent races with other env-var tests
            unsafe { std::env::set_var("III_REGISTRY_URL", &url) };
            let r = resolve_image("image-resize").await;
            unsafe { std::env::remove_var("III_REGISTRY_URL") };
            r
        };

        let (image, name) = result.unwrap();
        assert_eq!(image, "ghcr.io/iii-hq/image-resize:0.1.2");
        assert_eq!(name, "image-resize");
    }

    #[tokio::test]
    async fn resolve_image_shorthand_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("registry.json");
        let registry_json = r#"{"version": 2, "workers": {}}"#;
        std::fs::write(&registry_path, registry_json).unwrap();

        let url = format!("file://{}", registry_path.display());
        let result = {
            let _guard = ENV_LOCK.lock().unwrap();
            unsafe { std::env::set_var("III_REGISTRY_URL", &url) };
            let r = resolve_image("nonexistent").await;
            unsafe { std::env::remove_var("III_REGISTRY_URL") };
            r
        };

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found in registry"));
    }

    #[tokio::test]
    async fn resolve_image_with_slash_no_tag() {
        let (image, name) = resolve_image("ghcr.io/iii-hq/image-resize").await.unwrap();
        assert_eq!(image, "ghcr.io/iii-hq/image-resize");
        assert_eq!(name, "image-resize");
    }

    #[test]
    fn parse_registry_v2_with_binary_type() {
        let json = r#"{
            "version": 1,
            "workers": {
                "image-resize": {
                    "type": "binary",
                    "description": "Image resize worker",
                    "repo": "iii-hq/workers",
                    "tag_prefix": "image-resize",
                    "supported_targets": ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"],
                    "has_checksum": true,
                    "default_config": {
                        "name": "image-resize",
                        "config": { "width": 200 }
                    },
                    "version": "0.1.2"
                }
            }
        }"#;
        let registry: RegistryV2 = serde_json::from_str(json).unwrap();
        let entry = registry.workers.get("image-resize").unwrap();
        assert_eq!(entry.worker_type, Some(WorkerType::Binary));
        assert_eq!(entry.repo.as_deref(), Some("iii-hq/workers"));
        assert_eq!(entry.tag_prefix.as_deref(), Some("image-resize"));
        assert_eq!(entry.version.as_deref(), Some("0.1.2"));
        assert!(entry.has_checksum.unwrap_or(false));
        assert_eq!(
            entry.supported_targets.as_ref().unwrap(),
            &vec![
                "aarch64-apple-darwin".to_string(),
                "x86_64-unknown-linux-gnu".to_string()
            ]
        );
    }

    #[test]
    fn parse_registry_v2_managed_type_default() {
        let json = r#"{
            "version": 1,
            "workers": {
                "pdfkit": {
                    "description": "PDF worker",
                    "image": "ghcr.io/iii-hq/pdfkit",
                    "latest": "1.0.0"
                }
            }
        }"#;
        let registry: RegistryV2 = serde_json::from_str(json).unwrap();
        let entry = registry.workers.get("pdfkit").unwrap();
        assert_eq!(entry.worker_type, None);
        assert_eq!(entry.image.as_deref(), Some("ghcr.io/iii-hq/pdfkit"));
        assert_eq!(entry.latest.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn parse_version_override_syntax() {
        let (name, version) = parse_worker_input("image-resize@0.1.2");
        assert_eq!(name, "image-resize");
        assert_eq!(version, Some("0.1.2".to_string()));
    }

    #[test]
    fn parse_name_without_version() {
        let (name, version) = parse_worker_input("image-resize");
        assert_eq!(name, "image-resize");
        assert_eq!(version, None);
    }

    #[test]
    fn parse_worker_input_empty_version() {
        let (name, version) = parse_worker_input("pdfkit@");
        assert_eq!(name, "pdfkit");
        assert_eq!(version, Some("".to_string()));
    }

    #[test]
    fn parse_worker_input_with_multiple_at_signs() {
        let (name, version) = parse_worker_input("scope@org@1.0");
        assert_eq!(name, "scope");
        assert_eq!(version, Some("org@1.0".to_string()));
    }

    #[test]
    fn validate_worker_name_valid() {
        assert!(validate_worker_name("image-resize").is_ok());
        assert!(validate_worker_name("my_worker.v2").is_ok());
        assert!(validate_worker_name("pdfkit").is_ok());
    }

    #[test]
    fn validate_worker_name_rejects_path_traversal() {
        assert!(validate_worker_name("../../../etc/passwd").is_err());
        assert!(validate_worker_name("foo/bar").is_err());
        assert!(validate_worker_name("foo\\bar").is_err());
    }

    #[test]
    fn validate_worker_name_rejects_yaml_injection() {
        assert!(validate_worker_name("evil\n  - name: injected").is_err());
        assert!(validate_worker_name("evil\r\nimage: bad").is_err());
        assert!(validate_worker_name("name: injected").is_err());
    }

    #[test]
    fn validate_worker_name_rejects_empty() {
        assert!(validate_worker_name("").is_err());
    }

    #[test]
    fn validate_worker_name_rejects_dotdot() {
        assert!(validate_worker_name("..").is_err());
        assert!(validate_worker_name("foo..bar").is_err());
    }

    #[test]
    fn validate_repo_valid() {
        assert!(validate_repo("iii-hq/workers").is_ok());
        assert!(validate_repo("my-org/my-repo").is_ok());
    }

    #[test]
    fn validate_repo_rejects_traversal() {
        assert!(validate_repo("../../evil/repo").is_err());
        assert!(validate_repo("owner/../evil").is_err());
    }

    #[test]
    fn validate_repo_rejects_bad_format() {
        assert!(validate_repo("just-a-name").is_err());
        assert!(validate_repo("a/b/c").is_err());
        assert!(validate_repo("/leading-slash").is_err());
        assert!(validate_repo("trailing/").is_err());
    }
}
