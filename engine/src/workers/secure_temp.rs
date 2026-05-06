// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Shared helpers for serializing worker config blocks to a temp YAML file
//! consumed by spawned workers via `--config <path>`. Used by both the
//! in-process spawn path (`external.rs`) and the via-iii-worker path
//! (`registry_worker.rs`).

use std::path::PathBuf;

use serde_json::Value;

/// Per-engine deterministic path: `iii-{name}-{pid}-config.yaml`.
///
/// The `{pid}` segment scopes the file to a single engine instance so two
/// engines on the same host running a worker with the same name (e.g. parent
/// repo + worktree) don't collide on a shared `/tmp` path.
pub fn temp_config_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("iii-{}-{}-config.yaml", name, std::process::id()))
}

pub fn write_engine_config_temp(name: &str, cfg: &Value) -> Result<PathBuf, String> {
    let yaml = serde_yaml::to_string(cfg)
        .map_err(|e| format!("serialize config for '{}': {}", name, e))?;
    let path = temp_config_path(name);
    write_secure(&path, &yaml)
        .map_err(|e| format!("write config for '{}' to {}: {}", name, path.display(), e))?;
    Ok(path)
}

/// `unlink` + `create_new` + `mode(0o600)` because `OpenOptions::mode` is
/// honored only on file creation: opening a stale `0o644` file with
/// `create + truncate` keeps the loose perms and leaks secrets the worker
/// config can carry. Sticky-bit `/tmp` blocks unlinking foreign-owned files,
/// so an attacker squatting our deterministic path makes `create_new` fail
/// with `EEXIST` instead of silently writing through.
#[cfg(unix)]
pub fn write_secure(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::remove_file(path);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(content.as_bytes())
}

#[cfg(not(unix))]
pub fn write_secure(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_config_path_includes_name_and_pid() {
        let p = temp_config_path("pdfkit");
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("iii-pdfkit-"));
        assert!(name.ends_with("-config.yaml"));
        assert!(name.contains(&std::process::id().to_string()));
    }

    #[test]
    fn write_engine_config_temp_roundtrips_yaml() {
        let cfg = serde_json::json!({"host": "127.0.0.1", "port": 9000, "tls": true});
        let path = write_engine_config_temp("test-roundtrip-shared", &cfg).expect("write");

        let parsed: serde_json::Value =
            serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed, cfg);

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn write_engine_config_temp_sets_owner_only_perms() {
        use std::os::unix::fs::PermissionsExt;
        let path = write_engine_config_temp("test-perms-shared", &serde_json::json!({"x": 1}))
            .expect("write");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "got {:o}", mode);
        let _ = std::fs::remove_file(&path);
    }

    /// Regression: `OpenOptions::mode` is honored only on creation, so a
    /// stale `0o644` file must NOT keep its loose perms across the next
    /// write. `write_secure` enforces this via `unlink` + `create_new`.
    #[cfg(unix)]
    #[test]
    fn write_secure_overwrites_stale_perms() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join("iii-test-stale-secure-config.yaml");
        std::fs::write(&path, "stale").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_secure(&path, "fresh").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "got {:o}", mode);

        let _ = std::fs::remove_file(&path);
    }
}
