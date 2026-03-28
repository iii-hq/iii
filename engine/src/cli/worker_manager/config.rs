// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::fs;
use std::path::Path;

use crate::cli::error::WorkerError;

fn begin_marker(worker_name: &str) -> String {
    format!("  # === iii:{} BEGIN ===", worker_name)
}

fn end_marker(worker_name: &str) -> String {
    format!("  # === iii:{} END ===", worker_name)
}

const CONFIG_HEADER: &str = "workers:\n";

/// Check if a line is a top-level YAML key (not indented, not a comment, not empty).
fn is_top_level_key(line: &str) -> bool {
    !line.is_empty()
        && !line.starts_with(' ')
        && !line.starts_with('\t')
        && !line.starts_with('#')
        && line.contains(':')
}

/// Find the byte offset where the `workers:` section ends
/// (i.e., where the next top-level key starts).
/// Returns None if `workers:` is the last section in the file.
fn find_workers_section_end(content: &str) -> Option<usize> {
    let mut found_workers = false;
    let mut offset = 0;

    for line in content.split('\n') {
        if !found_workers {
            let trimmed = line.trim();
            if trimmed == "workers:" || trimmed.starts_with("workers:") {
                found_workers = true;
            }
        } else if is_top_level_key(line) {
            return Some(offset);
        }

        offset += line.len() + 1; // +1 for the \n delimiter
    }

    None
}

#[derive(Debug, PartialEq)]
pub enum ConfigOutcome {
    Added,
    AlreadyExists,
}

/// Serialize a worker's default_config into a marker-delimited YAML block.
fn serialize_config_block(
    worker_name: &str,
    default_config: &serde_json::Value,
) -> Result<String, WorkerError> {
    let class_value = default_config
        .get("class")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            WorkerError::ConfigError(format!(
                "default_config for '{}' is missing 'class' field",
                worker_name
            ))
        })?;

    let config_value = default_config.get("config");

    let mut lines = Vec::new();
    lines.push(begin_marker(worker_name));
    lines.push(format!("  - class: {}", class_value));

    if let Some(config) = config_value {
        if !config.is_null() {
            lines.push("    config:".to_string());
            let yaml_str = serde_yml::to_string(config).map_err(|e| {
                WorkerError::ConfigError(format!("Failed to serialize config to YAML: {}", e))
            })?;
            for line in yaml_str.lines() {
                if line.is_empty() {
                    continue;
                }
                lines.push(format!("      {}", line));
            }
        }
    }

    lines.push(end_marker(worker_name));

    Ok(lines.join("\n"))
}

/// Add a worker config block to config.yaml with marker delimiters.
/// If the file does not exist, creates it with a `workers:` header.
/// Returns AlreadyExists if markers for this worker already exist.
pub fn add_worker_config(
    project_dir: &Path,
    worker_name: &str,
    default_config: &serde_json::Value,
) -> Result<ConfigOutcome, WorkerError> {
    let config_path = project_dir.join("config.yaml");

    let mut content = if config_path.exists() {
        fs::read_to_string(&config_path).map_err(|e| {
            WorkerError::ConfigError(format!("Failed to read {}: {}", config_path.display(), e))
        })?
    } else {
        CONFIG_HEADER.to_string()
    };

    // Ensure the content has a workers: line
    if !content
        .lines()
        .any(|l| l.trim() == "workers:" || l.starts_with("workers:"))
    {
        content = format!("{}{}", CONFIG_HEADER, content);
    }

    // Check if markers already exist
    if content.contains(&begin_marker(worker_name)) {
        return Ok(ConfigOutcome::AlreadyExists);
    }

    let block = serialize_config_block(worker_name, default_config)?;

    // Insert block within the workers: section (before the next top-level key),
    // or append at end if workers: is the last section.
    match find_workers_section_end(&content) {
        Some(offset) => {
            let mut new_content = String::with_capacity(content.len() + block.len() + 4);
            new_content.push_str(&content[..offset]);
            if !new_content.ends_with('\n') {
                new_content.push('\n');
            }
            new_content.push_str(&block);
            new_content.push('\n');
            new_content.push_str(&content[offset..]);
            content = new_content;
        }
        None => {
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&block);
            content.push('\n');
        }
    }

    // Atomic write
    let tmp_path = project_dir.join("config.yaml.tmp");
    fs::write(&tmp_path, &content).map_err(|e| {
        WorkerError::ConfigError(format!("Failed to write {}: {}", tmp_path.display(), e))
    })?;
    fs::rename(&tmp_path, &config_path)
        .map_err(|e| WorkerError::ConfigError(format!("Failed to rename: {}", e)))?;

    Ok(ConfigOutcome::Added)
}

/// Remove a worker config block from config.yaml (lines between markers, inclusive).
/// Returns Ok(true) if the block was found and removed, Ok(false) if not found.
pub fn remove_worker_config(project_dir: &Path, worker_name: &str) -> Result<bool, WorkerError> {
    let config_path = project_dir.join("config.yaml");

    if !config_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&config_path).map_err(|e| {
        WorkerError::ConfigError(format!("Failed to read {}: {}", config_path.display(), e))
    })?;

    let begin = begin_marker(worker_name);
    let end = end_marker(worker_name);

    let lines: Vec<&str> = content.lines().collect();

    let begin_idx = lines.iter().position(|l| l.trim_end() == begin.trim_end());
    let end_idx = lines.iter().position(|l| l.trim_end() == end.trim_end());

    match (begin_idx, end_idx) {
        (Some(b), Some(e)) if b <= e => {
            let mut result: Vec<&str> = Vec::new();
            result.extend_from_slice(&lines[..b]);
            // Skip trailing blank line after the removed block
            let after = e + 1;
            if after < lines.len() && lines[after].trim().is_empty() {
                result.extend_from_slice(&lines[after + 1..]);
            } else {
                result.extend_from_slice(&lines[after..]);
            }

            let mut output = result.join("\n");
            if !output.ends_with('\n') {
                output.push('\n');
            }

            // Atomic write
            let tmp_path = project_dir.join("config.yaml.tmp");
            fs::write(&tmp_path, &output).map_err(|e| {
                WorkerError::ConfigError(format!("Failed to write {}: {}", tmp_path.display(), e))
            })?;
            fs::rename(&tmp_path, &config_path)
                .map_err(|e| WorkerError::ConfigError(format!("Failed to rename: {}", e)))?;

            Ok(true)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_config() -> serde_json::Value {
        serde_json::json!({
            "class": "workers::pdfkit::PdfKitWorker",
            "config": {
                "output_dir": "./output",
                "format": "pdf"
            }
        })
    }

    #[test]
    fn test_add_creates_file_with_header_and_block() {
        let dir = TempDir::new().unwrap();
        let result = add_worker_config(dir.path(), "pdfkit", &sample_config()).unwrap();
        assert_eq!(result, ConfigOutcome::Added);

        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(content.starts_with("workers:\n"));
        assert!(content.contains("# === iii:pdfkit BEGIN ==="));
        assert!(content.contains("- class: workers::pdfkit::PdfKitWorker"));
        assert!(content.contains("output_dir:"));
        assert!(content.contains("./output"));
        assert!(content.contains("format: pdf"));
        assert!(content.contains("# === iii:pdfkit END ==="));
    }

    #[test]
    fn test_add_appends_to_existing_config() {
        let dir = TempDir::new().unwrap();
        let existing =
            "workers:\n  - class: workers::existing::Mod\n    config:\n      key: value\n";
        fs::write(dir.path().join("config.yaml"), existing).unwrap();

        let result = add_worker_config(dir.path(), "pdfkit", &sample_config()).unwrap();
        assert_eq!(result, ConfigOutcome::Added);

        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(
            content.contains("workers::existing::Mod"),
            "Existing content preserved"
        );
        assert!(
            content.contains("# === iii:pdfkit BEGIN ==="),
            "New block added"
        );
    }

    #[test]
    fn test_add_nested_config_correct_indentation() {
        let dir = TempDir::new().unwrap();
        let config = serde_json::json!({
            "class": "workers::nested::Mod",
            "config": {
                "outer": {
                    "inner": "value"
                }
            }
        });

        add_worker_config(dir.path(), "nested", &config).unwrap();
        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(content.contains("    config:"));
        assert!(content.contains("      outer:"));
    }

    #[test]
    fn test_add_already_exists() {
        let dir = TempDir::new().unwrap();
        add_worker_config(dir.path(), "pdfkit", &sample_config()).unwrap();

        let result = add_worker_config(dir.path(), "pdfkit", &sample_config()).unwrap();
        assert_eq!(result, ConfigOutcome::AlreadyExists);
    }

    #[test]
    fn test_remove_removes_marker_block() {
        let dir = TempDir::new().unwrap();
        add_worker_config(dir.path(), "pdfkit", &sample_config()).unwrap();

        let removed = remove_worker_config(dir.path(), "pdfkit").unwrap();
        assert!(removed);

        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(!content.contains("pdfkit"), "Block should be removed");
        assert!(content.contains("workers:"), "Header should remain");
    }

    #[test]
    fn test_remove_no_markers_returns_false() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("config.yaml"), "workers:\n").unwrap();

        let removed = remove_worker_config(dir.path(), "pdfkit").unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_remove_no_file_returns_false() {
        let dir = TempDir::new().unwrap();
        let removed = remove_worker_config(dir.path(), "pdfkit").unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_user_comments_preserved_after_add_and_remove() {
        let dir = TempDir::new().unwrap();
        let existing = "# User comment at top\nworkers:\n  # Another comment\n  - class: workers::existing::Mod\n";
        fs::write(dir.path().join("config.yaml"), existing).unwrap();

        add_worker_config(dir.path(), "pdfkit", &sample_config()).unwrap();
        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(content.contains("# User comment at top"));
        assert!(content.contains("# Another comment"));

        remove_worker_config(dir.path(), "pdfkit").unwrap();
        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(content.contains("# User comment at top"));
        assert!(content.contains("# Another comment"));
    }

    #[test]
    fn test_add_inserts_in_workers_section_not_modules() {
        let dir = TempDir::new().unwrap();
        let existing = "workers:\n  - class: workers::existing::Worker\n    config:\n      key: value\n\nmodules:\n  - class: modules::api::RestApiModule\n    config:\n      port: 3111\n";
        fs::write(dir.path().join("config.yaml"), existing).unwrap();

        let result = add_worker_config(dir.path(), "pdfkit", &sample_config()).unwrap();
        assert_eq!(result, ConfigOutcome::Added);

        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();

        // The block should be within the workers: section, before modules:
        let workers_pos = content.find("workers:").unwrap();
        let modules_pos = content.find("modules:").unwrap();
        let block_pos = content.find("# === iii:pdfkit BEGIN ===").unwrap();

        assert!(block_pos > workers_pos, "Block should be after workers:");
        assert!(block_pos < modules_pos, "Block should be before modules:");

        // Verify modules section is unchanged
        assert!(
            content.contains("modules::api::RestApiModule"),
            "Modules content preserved"
        );
    }

    #[test]
    fn test_env_var_syntax_preserved() {
        let dir = TempDir::new().unwrap();
        let existing = "workers:\n  - class: workers::stream::StreamWorker\n    config:\n      port: ${STREAM_PORT:3112}\n      host: 127.0.0.1\n";
        fs::write(dir.path().join("config.yaml"), existing).unwrap();

        add_worker_config(dir.path(), "pdfkit", &sample_config()).unwrap();
        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(
            content.contains("${STREAM_PORT:3112}"),
            "Env var syntax preserved after add"
        );

        remove_worker_config(dir.path(), "pdfkit").unwrap();
        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(
            content.contains("${STREAM_PORT:3112}"),
            "Env var syntax preserved after remove"
        );
    }

    #[test]
    fn test_add_missing_class_field_returns_config_error() {
        let dir = TempDir::new().unwrap();
        let config = serde_json::json!({
            "config": {
                "output_dir": "./output"
            }
        });

        let result = add_worker_config(dir.path(), "badworker", &config);
        match result {
            Err(WorkerError::ConfigError(msg)) => {
                assert!(
                    msg.contains("missing 'class' field"),
                    "Error message should mention missing class field, got: {}",
                    msg
                );
                assert!(
                    msg.contains("badworker"),
                    "Error message should mention the worker name, got: {}",
                    msg
                );
            }
            other => panic!("Expected ConfigError, got: {:?}", other),
        }
    }

    #[test]
    fn test_add_with_null_config_omits_config_section() {
        let dir = TempDir::new().unwrap();
        let config = serde_json::json!({
            "class": "workers::nullcfg::NullWorker",
            "config": null
        });

        let result = add_worker_config(dir.path(), "nullcfg", &config).unwrap();
        assert_eq!(result, ConfigOutcome::Added);

        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(content.contains("- class: workers::nullcfg::NullWorker"));
        assert!(content.contains("# === iii:nullcfg BEGIN ==="));
        assert!(content.contains("# === iii:nullcfg END ==="));

        // Extract the block between markers and verify no "config:" line exists
        let begin = content.find("# === iii:nullcfg BEGIN ===").unwrap();
        let end = content.find("# === iii:nullcfg END ===").unwrap();
        let block = &content[begin..end];
        assert!(
            !block.contains("config:"),
            "config: section should be omitted when config is null, got block: {}",
            block
        );
    }

    #[test]
    fn test_add_with_no_config_field_omits_config_section() {
        let dir = TempDir::new().unwrap();
        let config = serde_json::json!({
            "class": "workers::bare::BareWorker"
        });

        let result = add_worker_config(dir.path(), "bare", &config).unwrap();
        assert_eq!(result, ConfigOutcome::Added);

        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(content.contains("- class: workers::bare::BareWorker"));

        let begin = content.find("# === iii:bare BEGIN ===").unwrap();
        let end = content.find("# === iii:bare END ===").unwrap();
        let block = &content[begin..end];
        assert!(
            !block.contains("config:"),
            "config: section should be omitted when config field is absent, got block: {}",
            block
        );
    }

    #[test]
    fn test_add_prepends_workers_header_when_missing() {
        let dir = TempDir::new().unwrap();
        // File exists but has no "workers:" line
        let existing = "modules:\n  - class: modules::api::Mod\n";
        fs::write(dir.path().join("config.yaml"), existing).unwrap();

        let result = add_worker_config(dir.path(), "pdfkit", &sample_config()).unwrap();
        assert_eq!(result, ConfigOutcome::Added);

        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(
            content.contains("workers:\n"),
            "Should have prepended workers: header"
        );
        assert!(
            content.contains("# === iii:pdfkit BEGIN ==="),
            "Worker block should be present"
        );
        // Original content should still be intact
        assert!(content.contains("modules::api::Mod"));
    }

    #[test]
    fn test_remove_begin_without_end_returns_false() {
        let dir = TempDir::new().unwrap();
        // Corrupted file: begin marker present, end marker missing
        let corrupted = "workers:\n  # === iii:pdfkit BEGIN ===\n  - class: workers::pdfkit::PdfKitWorker\n    config:\n      output_dir: ./output\n";
        fs::write(dir.path().join("config.yaml"), corrupted).unwrap();

        let removed = remove_worker_config(dir.path(), "pdfkit").unwrap();
        assert!(!removed, "Should return false when end marker is missing");

        // File should remain untouched
        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert_eq!(content, corrupted);
    }

    #[test]
    fn test_remove_end_before_begin_returns_false() {
        let dir = TempDir::new().unwrap();
        // End marker appears before begin marker
        let reversed = "workers:\n  # === iii:pdfkit END ===\n  - class: workers::pdfkit::PdfKitWorker\n  # === iii:pdfkit BEGIN ===\n";
        fs::write(dir.path().join("config.yaml"), reversed).unwrap();

        let removed = remove_worker_config(dir.path(), "pdfkit").unwrap();
        assert!(
            !removed,
            "Should return false when end marker comes before begin marker"
        );

        // File should remain untouched
        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert_eq!(content, reversed);
    }

    #[test]
    fn test_is_top_level_key_edge_cases_via_find_workers_section_end() {
        // Tab-indented lines should NOT be treated as top-level keys
        let with_tab = "workers:\n\t- class: workers::foo::Foo\n";
        assert!(
            find_workers_section_end(with_tab).is_none(),
            "Tab-indented line should not end the workers section"
        );

        // Comment lines should NOT be treated as top-level keys
        let with_comment =
            "workers:\n  - class: workers::foo::Foo\n# this is a comment with colon:\n";
        assert!(
            find_workers_section_end(with_comment).is_none(),
            "Comment line should not end the workers section"
        );

        // Empty lines should NOT be treated as top-level keys
        let with_empty = "workers:\n  - class: workers::foo::Foo\n\n  - class: workers::bar::Bar\n";
        assert!(
            find_workers_section_end(with_empty).is_none(),
            "Empty line should not end the workers section"
        );

        // A real top-level key SHOULD end the workers section
        let with_next_key =
            "workers:\n  - class: workers::foo::Foo\nmodules:\n  - class: modules::bar::Bar\n";
        let end_offset = find_workers_section_end(with_next_key);
        assert!(
            end_offset.is_some(),
            "Top-level key 'modules:' should end the workers section"
        );
        // The offset should point to the start of "modules:"
        let offset = end_offset.unwrap();
        assert!(
            with_next_key[offset..].starts_with("modules:"),
            "Offset should point to the start of 'modules:', got: '{}'",
            &with_next_key[offset..offset + 10.min(with_next_key.len() - offset)]
        );
    }

    #[test]
    fn test_add_with_empty_config_object() {
        let dir = TempDir::new().unwrap();
        let config = serde_json::json!({
            "class": "workers::minimal::MinWorker",
            "config": {}
        });

        let result = add_worker_config(dir.path(), "minimal", &config).unwrap();
        assert_eq!(result, ConfigOutcome::Added);

        let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(content.contains("- class: workers::minimal::MinWorker"));
        assert!(content.contains("# === iii:minimal BEGIN ==="));
        assert!(content.contains("# === iii:minimal END ==="));
    }
}
