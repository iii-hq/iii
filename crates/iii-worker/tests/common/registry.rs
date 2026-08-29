// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Descriptor-native Registry fixture for managed worker integration tests.

use iii_compose::descriptor::PackageDescriptor;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

pub struct DescriptorRegistry {
    server: MockServer,
}

impl DescriptorRegistry {
    /// Serve an immutable OCI package using the current Registry resolve
    /// contract. No legacy package metadata or embedded manifest is present.
    pub async fn oci(worker: &str) -> Self {
        let server = MockServer::start().await;
        let descriptor: PackageDescriptor = serde_json::from_value(serde_json::json!({
            "name": worker,
            "version": "1.0.0",
            "source": {
                "path": format!("workers/{worker}"),
                "package_manifest": "Cargo.toml"
            },
            "artifact": {
                "kind": "oci-image",
                "context": ".",
                "dockerfile": "Dockerfile",
                "platforms": ["linux/amd64", "linux/arm64"]
            },
            "runtime": {
                "exec": ["./worker"]
            },
            "registry": {
                "description": "Integration-test worker",
                "license": "Elastic-2.0",
                "tags": ["test"],
                "dependencies": {},
                "publish": true
            },
            "validation": {
                "interface": "skipped"
            }
        }))
        .expect("valid package descriptor fixture");
        let descriptor_sha256 = descriptor.sha256();
        let image_tag = format!("ghcr.io/iii-hq/{worker}@sha256:{}", "0".repeat(64));

        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "graph": [{
                    "package_descriptor": descriptor,
                    "descriptor_sha256": descriptor_sha256,
                    "artifacts": {
                        "kind": "oci-image",
                        "image_tag": image_tag
                    }
                }],
                "edges": []
            })))
            .mount(&server)
            .await;

        Self { server }
    }

    pub fn uri(&self) -> String {
        self.server.uri()
    }
}
