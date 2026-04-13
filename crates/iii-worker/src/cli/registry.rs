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

const DEFAULT_API_URL: &str = "https://api.workers.iii.dev";

/// Shared HTTP client for registry and download operations.
/// Reuses connections and TLS sessions across requests.
pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client")
});

#[derive(Debug, Clone, Deserialize)]
pub struct BinaryInfo {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkerConfig {
    pub name: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BinaryWorkerResponse {
    pub name: String,
    pub version: String,
    pub binaries: HashMap<String, BinaryInfo>,
    pub config: WorkerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OciWorkerResponse {
    pub name: String,
    pub version: String,
    pub image_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerInfoResponse {
    #[serde(rename = "binary")]
    Binary(BinaryWorkerResponse),
    #[serde(rename = "image")]
    Oci(OciWorkerResponse),
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

/// Parse "name@version" into (name, Some(version)) or just (name, None).
pub fn parse_worker_input(input: &str) -> (String, Option<String>) {
    if let Some((name, version)) = input.split_once('@') {
        (name.to_string(), Some(version.to_string()))
    } else {
        (input.to_string(), None)
    }
}

pub async fn fetch_worker_info(
    name: &str,
    version: Option<&str>,
) -> Result<WorkerInfoResponse, String> {
    validate_worker_name(name)?;

    let base_or_file = std::env::var("III_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string());

    let body = if base_or_file.starts_with("file://") {
        #[cfg(not(debug_assertions))]
        {
            return Err("file:// API URLs are only supported in debug/test builds. \
                 Set III_API_URL to an HTTPS URL."
                .to_string());
        }
        #[cfg(debug_assertions)]
        {
            let path = base_or_file.strip_prefix("file://").unwrap();
            std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read local API fixture at {}: {}", path, e))?
        }
    } else {
        let url = format!("{}/download/{}", base_or_file, name);

        let mut request = HTTP_CLIENT.get(&url);
        if let Some(v) = version {
            request = request.query(&[("version", v)]);
        }

        let resp = request
            .send()
            .await
            .map_err(|e| format!("Failed to resolve worker: {}", e))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(format!("Worker '{}' not found", name));
        }
        if !resp.status().is_success() {
            return Err(format!("Failed to resolve worker: HTTP {}", resp.status()));
        }

        resp.text()
            .await
            .map_err(|e| format!("Failed to read API response: {}", e))?
    };

    serde_json::from_str(&body).map_err(|e| format!("Failed to parse worker info: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize tests that mutate env vars to prevent races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[tokio::test]
    async fn fetch_worker_info_binary_via_file() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "name": "image-resize",
            "type": "binary",
            "version": "0.1.2",
            "binaries": {
                "aarch64-apple-darwin": {
                    "sha256": "abc123",
                    "url": "https://example.com/image-resize-aarch64-apple-darwin.tar.gz"
                }
            },
            "config": {
                "name": "image-resize",
                "config": { "width": 200 }
            }
        }"#;
        let response_path = dir.path().join("response.json");
        std::fs::write(&response_path, json).unwrap();

        let url = format!("file://{}", response_path.display());
        let result = {
            let _guard = ENV_LOCK.lock().unwrap();
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("image-resize", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };

        let info = result.unwrap();
        match info {
            WorkerInfoResponse::Binary(b) => {
                assert_eq!(b.name, "image-resize");
                assert_eq!(b.version, "0.1.2");
            }
            _ => panic!("expected Binary variant"),
        }
    }

    #[tokio::test]
    async fn fetch_worker_info_oci_via_file() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "name": "todo-worker",
            "type": "image",
            "version": "0.1.0",
            "image_url": "docker.io/andersonofl/todo-worker:0.1.0"
        }"#;
        let response_path = dir.path().join("response.json");
        std::fs::write(&response_path, json).unwrap();

        let url = format!("file://{}", response_path.display());
        let result = {
            let _guard = ENV_LOCK.lock().unwrap();
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("todo-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };

        let info = result.unwrap();
        match info {
            WorkerInfoResponse::Oci(o) => {
                assert_eq!(o.name, "todo-worker");
                assert_eq!(o.image_url, "docker.io/andersonofl/todo-worker:0.1.0");
            }
            _ => panic!("expected Oci variant"),
        }
    }

    #[tokio::test]
    async fn fetch_worker_info_rejects_invalid_name() {
        let result = fetch_worker_info("../evil", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("invalid characters") || err.contains("'..'"));
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
    fn deserialize_binary_worker_response() {
        let json = r#"{
            "name": "image-resize",
            "type": "binary",
            "version": "0.1.2",
            "binaries": {
                "aarch64-apple-darwin": {
                    "sha256": "5fdbce8e5db431ea6dddb527d3be0adf5bfac92fafac4a0c78d21e438d583f17",
                    "url": "https://github.com/iii-hq/workers/releases/download/image-resize/v0.1.2/image-resize-aarch64-apple-darwin.tar.gz"
                },
                "x86_64-unknown-linux-gnu": {
                    "sha256": "37c9b004c61cc76d8041cd3645ac7e7004cacd9eccbdd6bda1d847922fa98eb4",
                    "url": "https://github.com/iii-hq/workers/releases/download/image-resize/v0.1.2/image-resize-x86_64-unknown-linux-gnu.tar.gz"
                }
            },
            "config": {
                "name": "image-resize",
                "config": {
                    "width": 200,
                    "height": 200,
                    "quality": { "jpeg": 85, "webp": 80 },
                    "strategy": "scale-to-fit"
                }
            }
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Binary(b) => {
                assert_eq!(b.name, "image-resize");
                assert_eq!(b.version, "0.1.2");
                assert_eq!(b.binaries.len(), 2);
                let darwin = b.binaries.get("aarch64-apple-darwin").unwrap();
                assert_eq!(
                    darwin.sha256,
                    "5fdbce8e5db431ea6dddb527d3be0adf5bfac92fafac4a0c78d21e438d583f17"
                );
                assert!(darwin.url.ends_with("aarch64-apple-darwin.tar.gz"));
                assert_eq!(b.config.name, "image-resize");
            }
            _ => panic!("expected Binary variant"),
        }
    }

    #[test]
    fn deserialize_oci_worker_response() {
        let json = r#"{
            "name": "todo-worker",
            "type": "image",
            "version": "0.1.0",
            "image_url": "docker.io/andersonofl/todo-worker:0.1.0"
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Oci(o) => {
                assert_eq!(o.name, "todo-worker");
                assert_eq!(o.version, "0.1.0");
                assert_eq!(o.image_url, "docker.io/andersonofl/todo-worker:0.1.0");
            }
            _ => panic!("expected Oci variant"),
        }
    }

    #[test]
    fn deserialize_unknown_type_fails() {
        let json = r#"{"name": "x", "type": "wasm", "version": "1.0"}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // -- Deserialization edge cases --

    #[test]
    fn deserialize_binary_missing_type_fails() {
        let json = r#"{"name": "x", "version": "1.0", "binaries": {}, "config": {"name": "x", "config": {}}}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'type' field should fail");
    }

    #[test]
    fn deserialize_binary_missing_name_fails() {
        let json = r#"{"type": "binary", "version": "1.0", "binaries": {}, "config": {"name": "x", "config": {}}}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'name' field should fail");
    }

    #[test]
    fn deserialize_binary_missing_binaries_fails() {
        let json = r#"{"name": "x", "type": "binary", "version": "1.0", "config": {"name": "x", "config": {}}}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'binaries' field should fail");
    }

    #[test]
    fn deserialize_binary_missing_config_fails() {
        let json = r#"{"name": "x", "type": "binary", "version": "1.0", "binaries": {}}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'config' field should fail");
    }

    #[test]
    fn deserialize_oci_missing_image_url_fails() {
        let json = r#"{"name": "x", "type": "image", "version": "1.0"}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'image_url' field should fail");
    }

    #[test]
    fn deserialize_binary_empty_binaries_map_ok() {
        let json = r#"{
            "name": "empty-worker",
            "type": "binary",
            "version": "0.1.0",
            "binaries": {},
            "config": {"name": "empty-worker", "config": {}}
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Binary(b) => {
                assert_eq!(b.name, "empty-worker");
                assert!(b.binaries.is_empty());
            }
            _ => panic!("expected Binary variant"),
        }
    }

    #[test]
    fn deserialize_binary_info_missing_sha256_fails() {
        let json = r#"{
            "name": "x",
            "type": "binary",
            "version": "1.0",
            "binaries": {
                "aarch64-apple-darwin": {"url": "https://example.com/file.tar.gz"}
            },
            "config": {"name": "x", "config": {}}
        }"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "BinaryInfo missing sha256 should fail");
    }

    #[test]
    fn deserialize_binary_info_missing_url_fails() {
        let json = r#"{
            "name": "x",
            "type": "binary",
            "version": "1.0",
            "binaries": {
                "aarch64-apple-darwin": {"sha256": "abc123"}
            },
            "config": {"name": "x", "config": {}}
        }"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "BinaryInfo missing url should fail");
    }

    #[test]
    fn deserialize_extra_fields_tolerated() {
        let json = r#"{
            "name": "x",
            "type": "image",
            "version": "1.0",
            "image_url": "docker.io/x:1.0",
            "description": "this field is not in the struct",
            "author": "someone"
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Oci(o) => assert_eq!(o.name, "x"),
            _ => panic!("expected Oci variant"),
        }
    }

    #[test]
    fn deserialize_completely_invalid_json_fails() {
        let json = "not json at all";
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_empty_json_object_fails() {
        let json = "{}";
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "empty object should fail (no type tag)");
    }

    #[test]
    fn deserialize_config_with_null_value_ok() {
        let json = r#"{
            "name": "x",
            "type": "binary",
            "version": "1.0",
            "binaries": {},
            "config": {"name": "x", "config": null}
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Binary(b) => {
                assert!(b.config.config.is_null());
            }
            _ => panic!("expected Binary variant"),
        }
    }

    // -- fetch_worker_info error paths --

    #[tokio::test]
    async fn fetch_worker_info_empty_name_rejected() {
        let result = fetch_worker_info("", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn fetch_worker_info_dotdot_name_rejected() {
        let result = fetch_worker_info("..", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("'..'"));
    }

    #[tokio::test]
    async fn fetch_worker_info_file_not_found() {
        let url = "file:///tmp/nonexistent-iii-test-fixture-12345.json";
        let result = {
            let _guard = ENV_LOCK.lock().unwrap();
            unsafe { std::env::set_var("III_API_URL", url) };
            let r = fetch_worker_info("some-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("Failed to read local API fixture")
        );
    }

    #[tokio::test]
    async fn fetch_worker_info_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "this is not json").unwrap();

        let url = format!("file://{}", path.display());
        let result = {
            let _guard = ENV_LOCK.lock().unwrap();
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("some-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse worker info"));
    }

    #[tokio::test]
    async fn fetch_worker_info_empty_json_object() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.json");
        std::fs::write(&path, "{}").unwrap();

        let url = format!("file://{}", path.display());
        let result = {
            let _guard = ENV_LOCK.lock().unwrap();
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("some-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse worker info"));
    }

    #[tokio::test]
    async fn fetch_worker_info_wrong_type_in_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wasm.json");
        std::fs::write(&path, r#"{"name": "x", "type": "wasm", "version": "1.0"}"#).unwrap();

        let url = format!("file://{}", path.display());
        let result = {
            let _guard = ENV_LOCK.lock().unwrap();
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("some-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse worker info"));
    }
}
