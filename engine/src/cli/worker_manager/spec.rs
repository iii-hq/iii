// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use super::registry::WorkerEntry;
use crate::cli::registry::BinarySpec;

/// Build a `BinarySpec` from a dynamic `WorkerEntry`.
///
/// Uses `Box::leak` to satisfy the `'static` lifetime requirement of `BinarySpec`.
/// Acceptable for a CLI that runs once and exits (~100 bytes per call).
pub fn leaked_binary_spec(worker_name: &str, entry: &WorkerEntry) -> BinarySpec {
    let leaked_name: &'static str = Box::leak(worker_name.to_string().into_boxed_str());
    let leaked_repo: &'static str = Box::leak(entry.repo.clone().into_boxed_str());
    let leaked_targets: &'static [&'static str] = Box::leak(
        entry
            .supported_targets
            .iter()
            .map(|s| &*Box::leak(s.clone().into_boxed_str()))
            .collect::<Vec<&'static str>>()
            .into_boxed_slice(),
    );
    let leaked_prefix: Option<&'static str> = entry
        .tag_prefix
        .as_ref()
        .map(|s| &*Box::leak(s.clone().into_boxed_str()));

    BinarySpec {
        name: leaked_name,
        repo: leaked_repo,
        has_checksum: entry.has_checksum,
        supported_targets: leaked_targets,
        commands: &[],
        tag_prefix: leaked_prefix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(
        repo: &str,
        tag_prefix: Option<&str>,
        supported_targets: Vec<&str>,
        has_checksum: bool,
    ) -> WorkerEntry {
        WorkerEntry {
            description: String::new(),
            repo: repo.to_string(),
            tag_prefix: tag_prefix.map(|s| s.to_string()),
            supported_targets: supported_targets
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            has_checksum,
            default_config: None,
            local_path: None,
            version: None,
        }
    }

    #[test]
    fn test_basic_mapping_with_tag_prefix() {
        let entry = make_entry(
            "iii-hq/pdfkit",
            Some("pdfkit"),
            vec!["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"],
            true,
        );

        let spec = leaked_binary_spec("pdfkit", &entry);

        assert_eq!(spec.name, "pdfkit");
        assert_eq!(spec.repo, "iii-hq/pdfkit");
        assert!(spec.has_checksum);
        assert_eq!(spec.supported_targets.len(), 2);
        assert_eq!(spec.supported_targets[0], "aarch64-apple-darwin");
        assert_eq!(spec.supported_targets[1], "x86_64-unknown-linux-gnu");
        assert_eq!(spec.tag_prefix, Some("pdfkit"));
        assert!(spec.commands.is_empty());
    }

    #[test]
    fn test_tag_prefix_none() {
        let entry = make_entry(
            "iii-hq/image-processor",
            None,
            vec!["aarch64-apple-darwin"],
            false,
        );

        let spec = leaked_binary_spec("image-processor", &entry);

        assert!(spec.tag_prefix.is_none());
    }

    #[test]
    fn test_has_checksum_true() {
        let entry = make_entry("iii-hq/checked", None, vec![], true);

        let spec = leaked_binary_spec("checked", &entry);

        assert!(spec.has_checksum);
    }

    #[test]
    fn test_has_checksum_false() {
        let entry = make_entry("iii-hq/unchecked", None, vec![], false);

        let spec = leaked_binary_spec("unchecked", &entry);

        assert!(!spec.has_checksum);
    }

    #[test]
    fn test_empty_supported_targets() {
        let entry = make_entry("iii-hq/empty-targets", None, vec![], false);

        let spec = leaked_binary_spec("empty-targets", &entry);

        assert!(spec.supported_targets.is_empty());
    }

    #[test]
    fn test_multiple_supported_targets() {
        let targets = vec![
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-gnu",
        ];
        let entry = make_entry("iii-hq/multi", None, targets.clone(), false);

        let spec = leaked_binary_spec("multi", &entry);

        assert_eq!(spec.supported_targets.len(), 5);
        for (i, expected) in targets.iter().enumerate() {
            assert_eq!(spec.supported_targets[i], *expected);
        }
    }

    #[test]
    fn test_commands_always_empty() {
        let entry = make_entry(
            "iii-hq/any-worker",
            Some("v"),
            vec!["aarch64-apple-darwin"],
            true,
        );

        let spec = leaked_binary_spec("any-worker", &entry);

        assert!(spec.commands.is_empty());
        assert_eq!(spec.commands.len(), 0);
    }

    #[test]
    fn test_worker_name_correctly_leaked() {
        let name = String::from("my-worker");
        let entry = make_entry("iii-hq/my-worker", None, vec![], false);

        let spec = leaked_binary_spec(&name, &entry);

        assert_eq!(spec.name, "my-worker");
    }

    #[test]
    fn test_repo_string_correctly_leaked() {
        let entry = make_entry("org/repo-name", None, vec![], false);

        let spec = leaked_binary_spec("repo-name", &entry);

        assert_eq!(spec.repo, "org/repo-name");
    }
}