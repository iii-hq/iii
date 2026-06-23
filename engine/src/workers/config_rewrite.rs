// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Rewrites `config.yaml` after a worker seeds its `config:` block into the
//! configuration store for the first time. The block is removed (the
//! `- name:` entry is kept) and replaced with a comment pointing at the
//! configuration worker, which is now the runtime source of truth.
//!
//! The edit is line-based on purpose: a `serde_yaml` round-trip would drop
//! comments, reorder keys, and expand the `${VAR:default}` placeholders that
//! other worker blocks rely on. We only ever delete a contiguous run of lines
//! and insert one comment, so everything else in the file is byte-preserved.

/// Comment left in place of a stripped `config:` block. `location` names where
/// the value now lives (e.g. `./data/configuration/<id>.yaml` for the fs
/// adapter).
fn seed_comment(config_id: &str, location: &str) -> String {
    format!(
        "# '{config_id}': value now lives in the configuration worker at {location}. \
         Edit at runtime via 'iii config set {config_id}' (or 'configuration::set'); \
         this block is no longer read."
    )
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// If `line` is a `- name: <worker_name>` list item, return the indent (number
/// of leading spaces before the dash). Tolerates quotes and trailing comments.
fn entry_indent(line: &str, worker_name: &str) -> Option<usize> {
    let indent = leading_spaces(line);
    let rest = line[indent..].strip_prefix('-')?;
    let value = rest.trim_start().strip_prefix("name:")?.trim();
    // Strip quotes, then take the first whitespace-delimited token so a trailing
    // `# comment` or alignment spaces don't defeat the match. Worker names are
    // `[a-z0-9_-]`, so this is safe.
    let value = value.trim_matches('"').trim_matches('\'');
    let name = value.split_whitespace().next().unwrap_or("");
    (name == worker_name).then_some(indent)
}

/// Remove the `config:` block of the worker whose `- name:` equals
/// `worker_name`, replacing it with `comment`. Returns `None` (a no-op) when
/// the entry is absent or already has no `config:` block — which makes a second
/// call idempotent.
///
/// Only the **first** matching `- name:` is touched (duplicate instances across
/// `workers:`/`modules:` are renamed by `assign_instance_ids`, and only the
/// first occurrence seeds).
pub(crate) fn strip_worker_config_block(
    content: &str,
    worker_name: &str,
    comment: &str,
) -> Option<String> {
    let lines: Vec<&str> = content.split('\n').collect();

    // 1. Locate the worker entry.
    let (entry_idx, ent_indent) = lines
        .iter()
        .enumerate()
        .find_map(|(i, l)| entry_indent(l, worker_name).map(|ind| (i, ind)))?;

    // 2. Find this entry's `config:` key — scan its child lines (indent >
    //    ent_indent) until the entry ends (a non-blank line at indent <=
    //    ent_indent, i.e. the next list item or a dedent).
    let mut config_idx = None;
    let mut config_indent = 0;
    for (i, line) in lines.iter().enumerate().skip(entry_idx + 1) {
        if line.trim().is_empty() {
            continue;
        }
        let indent = leading_spaces(line);
        if indent <= ent_indent {
            break; // end of this entry
        }
        let trimmed = line.trim_start();
        if trimmed == "config:" || trimmed.starts_with("config:") {
            config_idx = Some(i);
            config_indent = indent;
            break;
        }
    }
    let config_idx = config_idx?;

    // 3. Extend the block: the `config:` line plus every following nested line
    //    (indent > config_indent). Blank lines are tentative — included only if
    //    a deeper line follows, so blank separators before the next entry stay.
    let mut last = config_idx;
    let mut i = config_idx + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        if leading_spaces(line) > config_indent {
            last = i;
            i += 1;
        } else {
            break;
        }
    }

    // 4. Rebuild: lines before the block, the comment at the block's indent,
    //    then everything after the block (trailing blanks preserved).
    let comment_line = format!("{}{}", " ".repeat(config_indent), comment);
    let mut out: Vec<&str> = lines[..config_idx].to_vec();
    out.push(&comment_line);
    out.extend_from_slice(&lines[last + 1..]);
    Some(out.join("\n"))
}

/// Read `path`, strip `config_id`'s block, and write it back atomically
/// (temp + rename, preserving the file's permissions). Best-effort: any IO
/// error is logged and swallowed so a failed rewrite never fails boot.
///
/// Returns `true` only when the file was actually rewritten. The caller uses
/// this to keep its in-memory copy of the entry in sync with the file: on a
/// no-op or a failed write the file still carries the block, so the in-memory
/// config must NOT be cleared (else the next reload diff would see a phantom
/// change and needlessly restart the worker).
pub(crate) fn apply_strip(path: &str, config_id: &str, location: &str) -> bool {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!(path, error = %err, "config seed-strip: cannot read config file");
            return false;
        }
    };
    let Some(updated) = strip_worker_config_block(&raw, config_id, &seed_comment(config_id, location))
    else {
        return false; // nothing to strip (entry/block absent) — idempotent no-op
    };
    if let Err(err) = atomic_write(path, &updated) {
        tracing::warn!(path, error = %err, "config seed-strip: failed to write config file");
        return false;
    }
    tracing::info!(
        config_id,
        path,
        "stripped seeded config block from config.yaml; value now lives in the configuration worker"
    );
    true
}

fn atomic_write(path: &str, content: &str) -> std::io::Result<()> {
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, content)?;
    if let Ok(meta) = std::fs::metadata(path) {
        std::fs::set_permissions(&tmp, meta.permissions()).ok();
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMENT: &str = "# moved";

    fn sample() -> String {
        // Two workers + a trailing top-level key. iii-stream carries `${VAR}`
        // placeholders and a nested `config:` (under adapter) to prove neither
        // is disturbed when iii-state is stripped, and that the nested config
        // is removed wholesale when iii-stream is stripped.
        "\
workers:
  - name: iii-stream
    config:
      port: ${STREAM_PORT:3112}
      adapter:
        name: redis
        config:
          redis_url: redis://localhost:6379
  - name: iii-state
    config:
      adapter:
        name: kv
  # a standalone comment
  - name: configuration
    config:
      ttl_seconds: 0
rootfs: overlay
"
        .to_string()
    }

    #[test]
    fn strips_block_and_inserts_comment_keeping_entry() {
        let out = strip_worker_config_block(&sample(), "iii-state", COMMENT).unwrap();
        assert!(out.contains("  - name: iii-state\n    # moved\n"));
        // iii-state's adapter/kv lines are gone.
        assert!(!out.contains("name: kv"));
        // Other entries and the standalone comment survive untouched.
        assert!(out.contains("redis_url: redis://localhost:6379"));
        assert!(out.contains("  # a standalone comment"));
        assert!(out.contains("  - name: configuration"));
        assert!(out.contains("rootfs: overlay"));
    }

    #[test]
    fn preserves_env_placeholders_in_other_blocks() {
        let out = strip_worker_config_block(&sample(), "iii-state", COMMENT).unwrap();
        assert!(out.contains("port: ${STREAM_PORT:3112}"));
    }

    #[test]
    fn strips_nested_config_wholesale() {
        // Stripping iii-stream must remove its block including the nested
        // `config:` under adapter, without touching iii-state.
        let out = strip_worker_config_block(&sample(), "iii-stream", COMMENT).unwrap();
        assert!(out.contains("  - name: iii-stream\n    # moved\n"));
        assert!(!out.contains("redis_url"));
        assert!(out.contains("  - name: iii-state\n    config:"));
    }

    #[test]
    fn idempotent_second_call_is_noop() {
        let once = strip_worker_config_block(&sample(), "iii-state", COMMENT).unwrap();
        assert!(strip_worker_config_block(&once, "iii-state", COMMENT).is_none());
    }

    #[test]
    fn absent_entry_is_noop() {
        assert!(strip_worker_config_block(&sample(), "iii-missing", COMMENT).is_none());
    }

    #[test]
    fn entry_without_config_block_is_noop() {
        let content = "\
workers:
  - name: iii-stream
    image: foo:latest
  - name: iii-state
    config:
      adapter:
        name: kv
";
        assert!(strip_worker_config_block(content, "iii-stream", COMMENT).is_none());
    }

    #[test]
    fn matches_entry_after_image_field() {
        let content = "\
workers:
  - name: iii-stream
    image: foo:latest
    config:
      port: 3112
  - name: iii-state
    config:
      adapter:
        name: kv
";
        let out = strip_worker_config_block(content, "iii-stream", COMMENT).unwrap();
        assert!(out.contains("    image: foo:latest\n    # moved\n"));
        assert!(!out.contains("port: 3112"));
        assert!(out.contains("name: kv"));
    }

    #[test]
    fn only_first_duplicate_entry_is_stripped() {
        let content = "\
workers:
  - name: iii-stream
    config:
      port: 1
modules:
  - name: iii-stream
    config:
      port: 2
";
        let out = strip_worker_config_block(content, "iii-stream", COMMENT).unwrap();
        // First occurrence stripped, second left intact.
        assert!(out.contains("    # moved\n"));
        assert!(!out.contains("port: 1"));
        assert!(out.contains("port: 2"));
    }
}
