// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! OCI registry resolution for worker images.

use serde::Deserialize;
use std::collections::HashMap;

pub const MANIFEST_PATH: &str = "/iii/worker.yaml";

const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/iii-hq/workers/main/registry/index.json";

#[derive(Debug, Deserialize)]
struct RegistryV2Entry {
    #[allow(dead_code)]
    description: String,
    image: String,
    latest: String,
}

#[derive(Debug, Deserialize)]
struct RegistryV2 {
    #[allow(dead_code)]
    version: u32,
    workers: HashMap<String, RegistryV2Entry>,
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

    let url =
        std::env::var("III_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let body = if url.starts_with("file://") {
        let path = url.strip_prefix("file://").unwrap();
        std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read local registry at {}: {}", path, e))?
    } else {
        let resp = client
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

    let registry: RegistryV2 =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse registry: {}", e))?;

    let entry = registry
        .workers
        .get(input)
        .ok_or_else(|| format!("Worker '{}' not found in registry", input))?;

    let image_ref = format!("{}:{}", entry.image, entry.latest);
    Ok((image_ref, input.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // SAFETY: test is single-threaded for env var access
        unsafe { std::env::set_var("III_REGISTRY_URL", &url) };
        let result = resolve_image("image-resize").await;
        unsafe { std::env::remove_var("III_REGISTRY_URL") };

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
        unsafe { std::env::set_var("III_REGISTRY_URL", &url) };
        let result = resolve_image("nonexistent").await;
        unsafe { std::env::remove_var("III_REGISTRY_URL") };

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found in registry"));
    }

    #[tokio::test]
    async fn resolve_image_with_slash_no_tag() {
        let (image, name) = resolve_image("ghcr.io/iii-hq/image-resize").await.unwrap();
        assert_eq!(image, "ghcr.io/iii-hq/image-resize");
        assert_eq!(name, "image-resize");
    }
}
