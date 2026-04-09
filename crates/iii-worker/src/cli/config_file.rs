// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Helpers to read/append/remove worker entries from `config.yaml` while
//! preserving existing formatting and comments.

use std::path::Path;

const CONFIG_FILE: &str = "config.yaml";

/// Canonical worker type resolved from config.yaml + filesystem.
#[derive(Debug)]
pub enum ResolvedWorkerType {
    /// OCI worker — has `image:` in config.yaml
    Oci {
        image: String,
        env: std::collections::HashMap<String, String>,
    },
    /// Local-path worker — has `worker_path:` in config.yaml
    Local { worker_path: String },
    /// Binary worker — executable at ~/.iii/workers/{name}
    Binary { binary_path: std::path::PathBuf },
    /// Config-only / builtin worker — no image, path, or binary
    Config,
}

// ──────────────────────────────────────────────────────────────────────────────
// Private helpers (operate on string content, making them easily testable)
// ──────────────────────────────────────────────────────────────────────────────

/// Returns `true` if `- name: {name}` appears anywhere in `content`.
fn worker_exists_in(content: &str, name: &str) -> bool {
    let pattern = format!("- name: {}", name);
    content.lines().any(|line| line.trim() == pattern.trim())
}

/// Indents every line of `yaml` by `spaces` spaces.
/// If `yaml` is empty or contains only whitespace, returns the prefix alone.
fn indent_yaml(yaml: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    let lines: Vec<&str> = yaml.lines().collect();
    if lines.is_empty() {
        return prefix;
    }
    lines
        .iter()
        .map(|line| format!("{}{}", prefix, line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Finds the byte offset just after the last line that belongs to the
/// `workers:` list (i.e., lines that start with whitespace or are list items
/// immediately under `workers:`).
///
/// Returns `content.len()` if the entire file is part of the workers section.
fn find_workers_list_end(content: &str) -> usize {
    let workers_marker = "workers:";
    let mut in_workers = false;
    let mut end_offset = 0;
    let mut current_offset = 0;

    for line in content.lines() {
        let line_len = line.len() + 1; // +1 for '\n'

        if !in_workers {
            if line.trim_start() == workers_marker || line.starts_with(workers_marker) {
                in_workers = true;
                end_offset = current_offset + line_len;
            }
        } else {
            // A non-empty line that starts at column 0 and is NOT the workers:
            // line itself means we've left the workers section.
            if !line.is_empty()
                && !line.starts_with(' ')
                && !line.starts_with('\t')
                && !line.starts_with('-')
            {
                // We've hit a new top-level key; stop here.
                break;
            }
            end_offset = current_offset + line_len;
        }

        current_offset += line_len;
    }

    // If we consumed all lines while in workers section, return full length
    // accounting for whether the file ends with a newline.
    if in_workers && end_offset == 0 {
        content.len()
    } else if in_workers {
        // Clamp to actual content length (handles files without trailing newline)
        end_offset.min(content.len())
    } else {
        content.len()
    }
}

/// Removes the entry `- name: {name}` (and all indented lines that follow)
/// from `content` and returns the resulting string.
fn remove_worker_from(content: &str, name: &str) -> String {
    let target = format!("- name: {}", name);
    let lines: Vec<&str> = content.lines().collect();
    let mut result: Vec<&str> = Vec::with_capacity(lines.len());

    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == target.as_str() {
            // Skip this line and all following indented lines (continuation of
            // the entry) until we hit the next `- name:` or a top-level key.
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                let next_trim = next.trim();
                // Stop skipping when we reach the next list item or a top-level key
                if next_trim.starts_with("- name:")
                    || (!next.starts_with(' ') && !next.starts_with('\t') && !next.is_empty())
                {
                    break;
                }
                i += 1;
            }
        } else {
            result.push(lines[i]);
            i += 1;
        }
    }

    let mut out = result.join("\n");
    // Preserve trailing newline if original had one
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Extract the raw YAML config block for a named worker from file content.
///
/// Returns the config lines (without the `config:` key itself) as a string
/// with leading indentation stripped to the config level.
fn extract_worker_config(content: &str, name: &str) -> Option<String> {
    let target = format!("- name: {}", name);
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    // Find the entry
    while i < lines.len() {
        if lines[i].trim() == target {
            break;
        }
        i += 1;
    }
    if i >= lines.len() {
        return None;
    }
    i += 1; // skip `- name:` line

    // Skip non-config fields (e.g., `image:`) until we find `config:`
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == "config:" || trimmed.starts_with("config:") {
            i += 1; // skip `config:` line
            break;
        }
        if trimmed.starts_with("- name:") || (!lines[i].starts_with(' ') && !lines[i].is_empty()) {
            return None; // hit next entry or top-level key, no config found
        }
        i += 1;
    }

    // Collect indented config lines
    let mut config_lines = Vec::new();
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.starts_with("- name:") || (!line.starts_with(' ') && !line.is_empty()) {
            break;
        }
        if line.is_empty() {
            i += 1;
            continue;
        }
        config_lines.push(line);
        i += 1;
    }

    if config_lines.is_empty() {
        return None;
    }

    // Strip common leading whitespace
    let min_indent = config_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    let stripped: Vec<&str> = config_lines
        .iter()
        .map(|l| {
            if l.len() >= min_indent {
                &l[min_indent..]
            } else {
                l.trim()
            }
        })
        .collect();

    Some(stripped.join("\n"))
}

/// Extract the `worker_path:` value for a named worker from file content.
fn extract_worker_path(content: &str, name: &str) -> Option<String> {
    let target = format!("- name: {}", name);
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    // Find the entry
    while i < lines.len() {
        if lines[i].trim() == target {
            i += 1;
            break;
        }
        i += 1;
    }

    // Look for `worker_path:` in the entry's indented lines
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("- name:") || (!lines[i].starts_with(' ') && !lines[i].is_empty()) {
            break; // hit next entry or top-level key
        }
        if let Some(rest) = trimmed.strip_prefix("worker_path:") {
            return Some(rest.trim().to_string());
        }
        i += 1;
    }

    None
}

/// Extract the `image:` value for a named worker from file content.
fn extract_image(content: &str, name: &str) -> Option<String> {
    let target = format!("- name: {}", name);
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    // Find the entry
    while i < lines.len() {
        if lines[i].trim() == target {
            i += 1;
            break;
        }
        i += 1;
    }

    // Look for `image:` in the entry's indented lines
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("- name:") || (!lines[i].starts_with(' ') && !lines[i].is_empty()) {
            break; // hit next entry or top-level key
        }
        if let Some(rest) = trimmed.strip_prefix("image:") {
            return Some(rest.trim().to_string());
        }
        i += 1;
    }

    None
}

/// Deep-merge two YAML config strings. `base` provides defaults, `overrides`
/// takes precedence. Both are parsed as serde_json::Value and merged.
fn merge_yaml_configs(base: &str, overrides: &str) -> String {
    let base_val: serde_json::Value = serde_yaml::from_str(base).unwrap_or(serde_json::Value::Null);
    let override_val: serde_json::Value =
        serde_yaml::from_str(overrides).unwrap_or(serde_json::Value::Null);

    let merged = deep_merge(base_val, override_val);
    serde_yaml::to_string(&merged).unwrap_or_else(|_| base.to_string())
}

/// Recursively merge two JSON values. `b` overrides `a` for scalar values.
/// For objects, keys are merged recursively.
fn deep_merge(a: serde_json::Value, b: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(mut a_map), Value::Object(b_map)) => {
            for (key, b_val) in b_map {
                let merged = if let Some(a_val) = a_map.remove(&key) {
                    deep_merge(a_val, b_val)
                } else {
                    b_val
                };
                a_map.insert(key, merged);
            }
            Value::Object(a_map)
        }
        (_, b) => b, // override takes precedence for non-objects
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────────────────

/// Returns `true` if `config.yaml` contains an entry for `name`.
pub fn worker_exists(name: &str) -> bool {
    let path = Path::new(CONFIG_FILE);
    if !path.exists() {
        return false;
    }
    match std::fs::read_to_string(path) {
        Ok(content) => worker_exists_in(&content, name),
        Err(_) => false,
    }
}

/// Appends a `- name: {name}` entry to the `workers:` list in `config.yaml`.
///
/// If `config_yaml` is provided it is indented and written under a `config:`
/// sub-key.  Creates the file (and the `workers:` key) if they do not exist.
pub fn append_worker(name: &str, config_yaml: Option<&str>) -> Result<(), String> {
    append_worker_impl(name, None, config_yaml)
}

/// Same as [`append_worker`] but also writes an `image: {image}` field.
pub fn append_worker_with_image(
    name: &str,
    image: &str,
    config_yaml: Option<&str>,
) -> Result<(), String> {
    append_worker_impl(name, Some(image), config_yaml)
}

/// Same as [`append_worker`] but writes a `worker_path: {worker_path}` field
/// instead of `image:`. Used for local directory-based workers.
pub fn append_worker_with_path(
    name: &str,
    worker_path: &str,
    config_yaml: Option<&str>,
) -> Result<(), String> {
    super::registry::validate_worker_name(name)?;
    let path = Path::new(CONFIG_FILE);

    let mut content = if path.exists() {
        std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {}", CONFIG_FILE, e))?
    } else {
        String::new()
    };

    if worker_exists_in(&content, name) {
        let existing_config = extract_worker_config(&content, name);
        content = remove_worker_from(&content, name);

        if let Some(existing) = existing_config {
            if let Some(incoming) = config_yaml {
                let merged = merge_yaml_configs(incoming, &existing);
                return append_to_content_with_fields(
                    &mut content,
                    path,
                    name,
                    None,
                    Some(worker_path),
                    Some(&merged),
                );
            }
            return append_to_content_with_fields(
                &mut content,
                path,
                name,
                None,
                Some(worker_path),
                Some(&existing),
            );
        }
    }

    append_to_content_with_fields(
        &mut content,
        path,
        name,
        None,
        Some(worker_path),
        config_yaml,
    )
}

/// Returns the `worker_path:` value for a named worker in `config.yaml`, if present.
pub fn get_worker_path(name: &str) -> Option<String> {
    let path = Path::new(CONFIG_FILE);
    let content = std::fs::read_to_string(path).ok()?;
    extract_worker_path(&content, name)
}

fn append_worker_impl(
    name: &str,
    image: Option<&str>,
    config_yaml: Option<&str>,
) -> Result<(), String> {
    super::registry::validate_worker_name(name)?;
    let path = Path::new(CONFIG_FILE);

    // Read existing content or start from scratch.
    let mut content = if path.exists() {
        std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {}", CONFIG_FILE, e))?
    } else {
        String::new()
    };

    // If the worker already exists, merge: extract existing config, remove old
    // entry, then re-append with merged config (existing user values override
    // incoming registry defaults).
    if worker_exists_in(&content, name) {
        let existing_config = extract_worker_config(&content, name);
        content = remove_worker_from(&content, name);

        // Deep-merge: registry defaults first, then user overrides on top.
        if let Some(existing) = existing_config {
            if let Some(incoming) = config_yaml {
                let merged = merge_yaml_configs(incoming, &existing);
                return append_to_content(&mut content, path, name, image, Some(&merged));
            }
            // No new config from registry — keep existing as-is.
            return append_to_content(&mut content, path, name, image, Some(&existing));
        }
        // Worker existed but had no config — use incoming.
    }

    append_to_content(&mut content, path, name, image, config_yaml)
}

/// Low-level: appends a worker entry to `content` and writes to `path`.
fn append_to_content(
    content: &mut String,
    path: &Path,
    name: &str,
    image: Option<&str>,
    config_yaml: Option<&str>,
) -> Result<(), String> {
    append_to_content_with_fields(content, path, name, image, None, config_yaml)
}

/// Low-level: appends a worker entry with optional `image` and `worker_path`
/// fields to `content` and writes to `path`.
fn append_to_content_with_fields(
    content: &mut String,
    path: &Path,
    name: &str,
    image: Option<&str>,
    worker_path: Option<&str>,
    config_yaml: Option<&str>,
) -> Result<(), String> {
    // Ensure there is a `workers:` key.
    if !content.contains("workers:") {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("workers:\n");
    }

    // Build the new entry block.
    let mut entry = format!("  - name: {}\n", name);
    if let Some(img) = image {
        entry.push_str(&format!("    image: {}\n", img));
    }
    if let Some(wp) = worker_path {
        entry.push_str(&format!("    worker_path: {}\n", wp));
    }
    if let Some(cfg) = config_yaml {
        let cfg = cfg.trim_end_matches('\n');
        if !cfg.is_empty() {
            entry.push_str("    config:\n");
            entry.push_str(&indent_yaml(cfg, 6));
            entry.push('\n');
        }
    }

    // Insert the entry at the end of the workers section.
    let insert_pos = find_workers_list_end(content);

    let prefix = &content[..insert_pos];
    let suffix = &content[insert_pos..];

    let mut new_content = String::with_capacity(content.len() + entry.len() + 1);
    new_content.push_str(prefix);
    if !prefix.is_empty() && !prefix.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&entry);
    new_content.push_str(suffix);

    std::fs::write(path, &new_content)
        .map_err(|e| format!("failed to write {}: {}", CONFIG_FILE, e))?;

    *content = new_content;

    Ok(())
}

/// Returns the `image:` value for a named worker in `config.yaml`, if present.
pub fn get_worker_image(name: &str) -> Option<String> {
    let path = Path::new(CONFIG_FILE);
    let content = std::fs::read_to_string(path).ok()?;
    extract_image(&content, name)
}

/// Resolve the worker type from config.yaml content (no filesystem access for binary check).
/// Used by tests and by `resolve_worker_type`.
fn resolve_worker_type_from_content(content: &str, name: &str) -> ResolvedWorkerType {
    // Check worker_path first (local), then image (OCI).
    // Consistent ordering: local > OCI > binary > config.
    if let Some(worker_path) = extract_worker_path(content, name) {
        return ResolvedWorkerType::Local { worker_path };
    }

    if let Some(image) = extract_image(content, name) {
        let config_str = extract_worker_config(content, name);
        let mut env = std::collections::HashMap::new();
        if let Some(cfg) = config_str {
            if let Ok(val) = serde_yaml::from_str::<serde_json::Value>(&cfg) {
                flatten_value_to_env(&val, "", &mut env);
            }
        }
        return ResolvedWorkerType::Oci { image, env };
    }

    ResolvedWorkerType::Config
}

/// Resolve the canonical worker type for a named worker.
/// Reads config.yaml once and checks the filesystem for binary workers.
/// Priority: local (worker_path) > OCI (image) > binary (~/.iii/workers/{name}) > config.
pub fn resolve_worker_type(name: &str) -> ResolvedWorkerType {
    let path = std::path::Path::new(CONFIG_FILE);
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return check_binary_fallback(name),
    };

    match resolve_worker_type_from_content(&content, name) {
        ResolvedWorkerType::Config => check_binary_fallback(name),
        other => other,
    }
}

fn check_binary_fallback(name: &str) -> ResolvedWorkerType {
    let binary_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".iii/workers")
        .join(name);
    if binary_path.exists() {
        ResolvedWorkerType::Binary { binary_path }
    } else {
        ResolvedWorkerType::Config
    }
}

/// Returns the `config:` block for a named worker as a flat `HashMap<String, String>`.
///
/// Suitable for injecting as environment variables into OCI workers.
/// Nested keys are flattened with `_` separator and uppercased.
pub fn get_worker_config_as_env(name: &str) -> std::collections::HashMap<String, String> {
    let path = Path::new(CONFIG_FILE);
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return std::collections::HashMap::new(),
    };

    let config_str = match extract_worker_config(&content, name) {
        Some(c) => c,
        None => return std::collections::HashMap::new(),
    };

    let value: serde_json::Value = match serde_yaml::from_str(&config_str) {
        Ok(v) => v,
        Err(_) => return std::collections::HashMap::new(),
    };

    let mut env = std::collections::HashMap::new();
    flatten_value_to_env(&value, "", &mut env);
    env
}

/// Recursively flatten a JSON value into key=value pairs.
/// Keys are uppercased, nested objects joined with `_`.
fn flatten_value_to_env(
    value: &serde_json::Value,
    prefix: &str,
    out: &mut std::collections::HashMap<String, String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                let full_key = if prefix.is_empty() {
                    key.to_uppercase()
                } else {
                    format!("{}_{}", prefix, key.to_uppercase())
                };
                flatten_value_to_env(val, &full_key, out);
            }
        }
        serde_json::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

/// Returns the image and config env for a named worker in a single file read.
/// Returns `None` if the worker has no `image:` field (binary workers).
pub fn get_worker_start_info(
    name: &str,
) -> Option<(String, std::collections::HashMap<String, String>)> {
    let path = Path::new(CONFIG_FILE);
    let content = std::fs::read_to_string(path).ok()?;

    // Extract image
    let target = format!("- name: {}", name);
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut image: Option<String> = None;

    // Find the entry
    while i < lines.len() {
        if lines[i].trim() == target {
            i += 1;
            break;
        }
        i += 1;
    }

    // Look for image in entry's indented lines
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("- name:") || (!lines[i].starts_with(' ') && !lines[i].is_empty()) {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("image:") {
            image = Some(rest.trim().to_string());
        }
        i += 1;
    }

    let image = image?; // Return None if no image (binary worker)

    // Extract config env using existing helper
    let env = match extract_worker_config(&content, name) {
        Some(cfg) => match serde_yaml::from_str::<serde_json::Value>(&cfg) {
            Ok(value) => {
                let mut env = std::collections::HashMap::new();
                flatten_value_to_env(&value, "", &mut env);
                env
            }
            Err(_) => std::collections::HashMap::new(),
        },
        None => std::collections::HashMap::new(),
    };

    Some((image, env))
}

/// Removes the named worker entry from `config.yaml`.
pub fn remove_worker(name: &str) -> Result<(), String> {
    let path = Path::new(CONFIG_FILE);
    if !path.exists() {
        return Err(format!("{} not found", CONFIG_FILE));
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", CONFIG_FILE, e))?;

    if !worker_exists_in(&content, name) {
        return Err(format!("Worker '{}' not found in {}", name, CONFIG_FILE));
    }

    let new_content = remove_worker_from(&content, name);

    std::fs::write(path, &new_content)
        .map_err(|e| format!("failed to write {}: {}", CONFIG_FILE, e))?;

    Ok(())
}

/// Returns all worker names listed under `workers:` in `config.yaml`.
pub fn list_worker_names() -> Vec<String> {
    let path = Path::new(CONFIG_FILE);
    if !path.exists() {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut names = Vec::new();
    let mut in_workers = false;

    for line in content.lines() {
        if line.trim_start() == "workers:" || line.starts_with("workers:") {
            in_workers = true;
            continue;
        }
        if in_workers {
            // Left the workers section if we hit a non-indented, non-empty line
            if !line.is_empty()
                && !line.starts_with(' ')
                && !line.starts_with('\t')
                && !line.starts_with('-')
            {
                break;
            }
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("- name:") {
                let name = rest.trim().to_string();
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
    }

    names
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_exists_in_found() {
        let content = "workers:\n  - name: iii-stream\n    config:\n      port: 3112\n";
        assert!(worker_exists_in(content, "iii-stream"));
    }

    #[test]
    fn test_worker_exists_in_not_found() {
        let content = "workers:\n  - name: iii-stream\n";
        assert!(!worker_exists_in(content, "image-resize"));
    }

    #[test]
    fn test_remove_worker_from() {
        let content = "workers:\n  - name: iii-stream\n    config:\n      port: 3112\n  - name: image-resize\n    config:\n      width: 200\n";
        let result = remove_worker_from(content, "image-resize");
        assert!(result.contains("iii-stream"));
        assert!(!result.contains("image-resize"));
    }

    #[test]
    fn test_indent_yaml() {
        let yaml = "width: 200\nheight: 100";
        let indented = indent_yaml(yaml, 4);
        assert_eq!(indented, "    width: 200\n    height: 100");
    }

    #[test]
    fn test_worker_exists_in_comment_line_not_matched() {
        let content = "workers:\n  # - name: ghost\n  - name: real\n";
        assert!(!worker_exists_in(content, "ghost"));
        assert!(worker_exists_in(content, "real"));
    }

    #[test]
    fn test_remove_worker_from_preserves_others() {
        let content = "workers:\n  - name: a\n    config:\n      x: 1\n  - name: b\n  - name: c\n";
        let result = remove_worker_from(content, "a");
        assert!(!result.contains("- name: a"));
        assert!(result.contains("- name: b"));
        assert!(result.contains("- name: c"));
    }

    #[test]
    fn test_list_worker_names_basic() {
        // We can't call list_worker_names() without a real file, but we can
        // test the logic via the helpers used in it.
        let content = "workers:\n  - name: foo\n  - name: bar\n";
        let mut names = Vec::new();
        let mut in_workers = false;
        for line in content.lines() {
            if line.trim_start() == "workers:" || line.starts_with("workers:") {
                in_workers = true;
                continue;
            }
            if in_workers {
                if !line.is_empty()
                    && !line.starts_with(' ')
                    && !line.starts_with('\t')
                    && !line.starts_with('-')
                {
                    break;
                }
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("- name:") {
                    let name = rest.trim().to_string();
                    if !name.is_empty() {
                        names.push(name);
                    }
                }
            }
        }
        assert_eq!(names, vec!["foo", "bar"]);
    }

    #[test]
    fn test_indent_yaml_empty() {
        assert_eq!(indent_yaml("", 4), "    ");
    }

    #[test]
    fn test_extract_worker_config_basic() {
        let content = "workers:\n  - name: iii-stream\n    config:\n      port: 3112\n      host: localhost\n";
        let config = extract_worker_config(content, "iii-stream").unwrap();
        assert!(config.contains("port: 3112"));
        assert!(config.contains("host: localhost"));
    }

    #[test]
    fn test_extract_worker_config_missing_config() {
        let content = "workers:\n  - name: iii-stream\n";
        let config = extract_worker_config(content, "iii-stream");
        assert!(config.is_none());
    }

    #[test]
    fn test_extract_worker_config_not_found() {
        let content = "workers:\n  - name: iii-stream\n    config:\n      port: 3112\n";
        let config = extract_worker_config(content, "nonexistent");
        assert!(config.is_none());
    }

    #[test]
    fn test_extract_worker_config_with_image_field() {
        let content = "workers:\n  - name: pdfkit\n    image: ghcr.io/iii-hq/pdfkit:1.0\n    config:\n      timeout: 30\n";
        let config = extract_worker_config(content, "pdfkit").unwrap();
        assert!(config.contains("timeout: 30"));
        assert!(!config.contains("image:"));
    }

    #[test]
    fn test_extract_worker_config_multiple_workers() {
        let content = "workers:\n  - name: a\n    config:\n      x: 1\n  - name: b\n    config:\n      y: 2\n";
        let config_a = extract_worker_config(content, "a").unwrap();
        assert!(config_a.contains("x: 1"));
        assert!(!config_a.contains("y: 2"));

        let config_b = extract_worker_config(content, "b").unwrap();
        assert!(config_b.contains("y: 2"));
        assert!(!config_b.contains("x: 1"));
    }

    #[test]
    fn test_merge_yaml_configs_override() {
        let base = "width: 200\nheight: 100";
        let overrides = "width: 300";
        let merged = merge_yaml_configs(base, overrides);
        let val: serde_json::Value = serde_yaml::from_str(&merged).unwrap();
        assert_eq!(val["width"], 300);
        assert_eq!(val["height"], 100);
    }

    #[test]
    fn test_merge_yaml_configs_disjoint_keys() {
        let base = "a: 1";
        let overrides = "b: 2";
        let merged = merge_yaml_configs(base, overrides);
        let val: serde_json::Value = serde_yaml::from_str(&merged).unwrap();
        assert_eq!(val["a"], 1);
        assert_eq!(val["b"], 2);
    }

    #[test]
    fn test_deep_merge_nested_objects() {
        use serde_json::json;
        let a = json!({"server": {"host": "localhost", "port": 8080}});
        let b = json!({"server": {"port": 9090, "tls": true}});
        let merged = deep_merge(a, b);
        assert_eq!(merged["server"]["host"], "localhost");
        assert_eq!(merged["server"]["port"], 9090);
        assert_eq!(merged["server"]["tls"], true);
    }

    #[test]
    fn test_deep_merge_scalar_override() {
        use serde_json::json;
        let a = json!("old");
        let b = json!("new");
        let merged = deep_merge(a, b);
        assert_eq!(merged, json!("new"));
    }

    #[test]
    fn test_flatten_value_to_env_flat() {
        use serde_json::json;
        let val = json!({"host": "localhost", "port": "8080"});
        let mut env = std::collections::HashMap::new();
        flatten_value_to_env(&val, "", &mut env);
        assert_eq!(env.get("HOST").unwrap(), "localhost");
        assert_eq!(env.get("PORT").unwrap(), "8080");
    }

    #[test]
    fn test_flatten_value_to_env_nested() {
        use serde_json::json;
        let val = json!({"database": {"host": "db.local", "port": 5432}});
        let mut env = std::collections::HashMap::new();
        flatten_value_to_env(&val, "", &mut env);
        assert_eq!(env.get("DATABASE_HOST").unwrap(), "db.local");
        assert_eq!(env.get("DATABASE_PORT").unwrap(), "5432");
    }

    #[test]
    fn test_flatten_value_to_env_bool_and_number() {
        use serde_json::json;
        let val = json!({"debug": true, "retries": 3});
        let mut env = std::collections::HashMap::new();
        flatten_value_to_env(&val, "", &mut env);
        assert_eq!(env.get("DEBUG").unwrap(), "true");
        assert_eq!(env.get("RETRIES").unwrap(), "3");
    }

    #[test]
    fn test_find_workers_list_end_with_trailing_content() {
        let content = "workers:\n  - name: a\n  - name: b\nother_key: value\n";
        let end = find_workers_list_end(content);
        let workers_section = &content[..end];
        assert!(workers_section.contains("- name: b"));
        assert!(!workers_section.contains("other_key"));
    }

    #[test]
    fn test_find_workers_list_end_entire_file() {
        let content = "workers:\n  - name: a\n  - name: b\n";
        let end = find_workers_list_end(content);
        assert_eq!(end, content.len());
    }

    #[test]
    fn test_find_workers_list_end_no_workers_key() {
        let content = "other: stuff\n";
        let end = find_workers_list_end(content);
        assert_eq!(end, content.len());
    }

    #[test]
    fn test_extract_image_found() {
        let content = "workers:\n  - name: pdfkit\n    image: ghcr.io/iii-hq/pdfkit:1.0\n";
        let image = extract_image(content, "pdfkit");
        assert_eq!(image, Some("ghcr.io/iii-hq/pdfkit:1.0".to_string()));
    }

    #[test]
    fn test_extract_image_not_found() {
        let content = "workers:\n  - name: local-w\n    worker_path: /tmp/w\n";
        let image = extract_image(content, "local-w");
        assert!(image.is_none());
    }

    #[test]
    fn test_resolve_worker_type_local() {
        let content = "workers:\n  - name: my-local\n    worker_path: /home/user/proj\n";
        let resolved = resolve_worker_type_from_content(content, "my-local");
        assert!(
            matches!(resolved, ResolvedWorkerType::Local { worker_path } if worker_path == "/home/user/proj")
        );
    }

    #[test]
    fn test_resolve_worker_type_oci() {
        let content = "workers:\n  - name: pdfkit\n    image: ghcr.io/iii-hq/pdfkit:1.0\n    config:\n      timeout: 30\n";
        let resolved = resolve_worker_type_from_content(content, "pdfkit");
        match resolved {
            ResolvedWorkerType::Oci { image, env } => {
                assert_eq!(image, "ghcr.io/iii-hq/pdfkit:1.0");
                assert_eq!(env.get("TIMEOUT"), Some(&"30".to_string()));
            }
            other => panic!("expected Oci, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_worker_type_config_fallback() {
        let content = "workers:\n  - name: builtin\n";
        let resolved = resolve_worker_type_from_content(content, "builtin");
        assert!(matches!(resolved, ResolvedWorkerType::Config));
    }

    #[test]
    fn test_resolve_worker_type_local_takes_precedence_over_image() {
        let content =
            "workers:\n  - name: weird\n    worker_path: /tmp/proj\n    image: ghcr.io/org/w:1\n";
        let resolved = resolve_worker_type_from_content(content, "weird");
        assert!(matches!(resolved, ResolvedWorkerType::Local { .. }));
    }

    #[test]
    fn test_remove_worker_from_first_entry() {
        let content = "workers:\n  - name: first\n    config:\n      x: 1\n  - name: second\n";
        let result = remove_worker_from(content, "first");
        assert!(!result.contains("- name: first"));
        assert!(result.contains("- name: second"));
    }

    #[test]
    fn test_remove_worker_from_only_entry() {
        let content = "workers:\n  - name: solo\n    config:\n      x: 1\n";
        let result = remove_worker_from(content, "solo");
        assert!(!result.contains("- name: solo"));
        assert!(result.contains("workers:"));
    }

    #[test]
    fn test_append_worker_with_path_field() {
        let mut content = "workers:\n".to_string();
        let path = std::path::Path::new("/tmp/test-config.yaml");
        let _ = std::fs::write(path, &content);
        append_to_content_with_fields(
            &mut content,
            path,
            "local-worker",
            None,
            Some("/absolute/path/to/worker"),
            Some("timeout: 30"),
        )
        .unwrap();
        assert!(content.contains("- name: local-worker"));
        assert!(content.contains("worker_path: /absolute/path/to/worker"));
        assert!(content.contains("timeout: 30"));
    }

    #[test]
    fn test_get_worker_path_found() {
        let content = "workers:\n  - name: my-worker\n    worker_path: /home/user/my-worker\n    config:\n      timeout: 30\n";
        let path = extract_worker_path(content, "my-worker");
        assert_eq!(path, Some("/home/user/my-worker".to_string()));
    }

    #[test]
    fn test_get_worker_path_not_found() {
        let content = "workers:\n  - name: oci-worker\n    image: ghcr.io/org/worker:tag\n";
        let path = extract_worker_path(content, "oci-worker");
        assert!(path.is_none());
    }

    #[test]
    fn test_get_worker_start_info_with_image_and_config() {
        let content = "workers:\n  - name: pdfkit\n    image: ghcr.io/iii-hq/pdfkit:1.0\n    config:\n      timeout: 30\n";
        // We can't call the public fn without a real file, but we can verify the internal logic
        // by testing the components. The public fn is tested in integration tests.
        let image = {
            let target = "- name: pdfkit";
            let lines: Vec<&str> = content.lines().collect();
            let mut i = 0;
            while i < lines.len() {
                if lines[i].trim() == target {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let mut img = None;
            while i < lines.len() {
                let trimmed = lines[i].trim();
                if trimmed.starts_with("- name:")
                    || (!lines[i].starts_with(' ') && !lines[i].is_empty())
                {
                    break;
                }
                if let Some(rest) = trimmed.strip_prefix("image:") {
                    img = Some(rest.trim().to_string());
                }
                i += 1;
            }
            img
        };
        assert_eq!(image, Some("ghcr.io/iii-hq/pdfkit:1.0".to_string()));

        let config = extract_worker_config(content, "pdfkit");
        assert!(config.is_some());
        assert!(config.unwrap().contains("timeout: 30"));
    }
}
