// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.

//! Small Registry helpers used by the descriptor-native installer.
//!
//! Resolution, dependency locking, descriptor verification, and artifact
//! installation live in `iii_compose::registry`. This module deliberately
//! contains no alternate Registry response model or legacy download endpoint.

use serde::Deserialize;
use std::sync::LazyLock;

/// Shared HTTP client used only by the bounded binary artifact downloader.
pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .expect("failed to create Registry artifact client")
});

#[derive(Debug, Clone, Deserialize)]
pub struct BinaryInfo {
    pub url: String,
    pub sha256: String,
}

pub use crate::core::types::validate_worker_name;

/// Parse `name@version` into an immutable Registry request.
///
/// The caller validates `name`; an empty version is left present so Registry
/// resolution rejects it instead of silently treating it as `latest`.
pub fn parse_worker_input(input: &str) -> (String, Option<String>) {
    if let Some((name, version)) = input.split_once('@') {
        (name.to_string(), Some(version.to_string()))
    } else {
        (input.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_registry_reference_only() {
        assert_eq!(
            parse_worker_input("queue@1.2.3-rc.1"),
            ("queue".into(), Some("1.2.3-rc.1".into()))
        );
        assert_eq!(parse_worker_input("queue"), ("queue".into(), None));
    }
}
