// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Private, persistent workspaces for registry bundles that run on the host.
//!
//! The registry cache is shared and integrity-checked, so publisher commands
//! must never run inside it. A host bundle gets a project/container-owned copy;
//! its source fingerprint decides when that copy is replaced and its prepared
//! marker decides when the publisher's install command runs again.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::{ComposeError, Result},
    manifest::HostBundleSpec,
};

const INTEGRITY_FILE: &str = ".iii-compose-integrity.json";
const WORKSPACE_META: &str = ".iii-compose-host-workspace.json";
const PREPARED_FILE: &str = ".iii-compose-host-prepared.json";
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct WorkspaceMeta {
    format_version: u32,
    source_fingerprint: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PreparedMeta {
    format_version: u32,
    source_fingerprint: String,
    install_fingerprint: String,
    os: String,
    arch: String,
}

#[derive(Debug)]
pub struct PreparedHostBundle {
    pub workspace: PathBuf,
    pub install_required: bool,
}

/// Materializes or reuses one container's private workspace.
pub fn prepare(spec: &HostBundleSpec, host_root: &Path, key: &str) -> Result<PreparedHostBundle> {
    let source_fingerprint = source_fingerprint(&spec.install_dir)?;
    let container_dir = host_root.join(key);
    let workspace = container_dir.join("workspace");
    let expected_workspace = WorkspaceMeta {
        format_version: FORMAT_VERSION,
        source_fingerprint: source_fingerprint.clone(),
    };

    let workspace_replaced = !workspace_matches(&workspace, &expected_workspace);
    if workspace_replaced {
        fs::create_dir_all(&container_dir).map_err(|source| io_error(&container_dir, source))?;
        let staging = container_dir.join(format!("workspace.tmp-{}", uuid::Uuid::new_v4()));
        remove_any(&staging)?;
        fs::create_dir(&staging).map_err(|source| io_error(&staging, source))?;
        if let Err(error) = copy_tree(&spec.install_dir, &staging)
            .and_then(|_| write_json_atomic(&staging.join(WORKSPACE_META), &expected_workspace))
        {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        remove_any(&workspace)?;
        fs::rename(&staging, &workspace).map_err(|source| io_error(&workspace, source))?;
    }

    let expected_prepared = PreparedMeta {
        format_version: FORMAT_VERSION,
        source_fingerprint,
        install_fingerprint: install_fingerprint(spec.install.as_deref()),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    };
    let prepared_path = container_dir.join(PREPARED_FILE);
    let install_required = spec.install.is_some()
        && (workspace_replaced || !json_matches(&prepared_path, &expected_prepared));

    Ok(PreparedHostBundle {
        workspace,
        install_required,
    })
}

/// Records a successful install. An absent install is already prepared by
/// definition and deliberately needs no marker.
pub fn mark_prepared(spec: &HostBundleSpec, host_root: &Path, key: &str) -> Result<()> {
    let Some(_) = spec.install else {
        return Ok(());
    };
    let meta = PreparedMeta {
        format_version: FORMAT_VERSION,
        source_fingerprint: source_fingerprint(&spec.install_dir)?,
        install_fingerprint: install_fingerprint(spec.install.as_deref()),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    };
    write_json_atomic(&host_root.join(key).join(PREPARED_FILE), &meta)
}

fn source_fingerprint(install_dir: &Path) -> Result<String> {
    let path = install_dir.join(INTEGRITY_FILE);
    let bytes = fs::read(&path).map_err(|source| io_error(&path, source))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn install_fingerprint(install: Option<&str>) -> String {
    hex::encode(Sha256::digest(install.unwrap_or("").as_bytes()))
}

fn workspace_matches(path: &Path, expected: &WorkspaceMeta) -> bool {
    path.is_dir() && json_matches(&path.join(WORKSPACE_META), expected)
}

fn json_matches<T>(path: &Path, expected: &T) -> bool
where
    T: for<'de> Deserialize<'de> + PartialEq,
{
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<T>(&bytes).ok())
        .as_ref()
        == Some(expected)
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    let temp = parent.join(format!(".tmp-{}", uuid::Uuid::new_v4()));
    let bytes =
        serde_json::to_vec(value).map_err(|source| io_error(path, io::Error::other(source)))?;
    fs::write(&temp, bytes).map_err(|source| io_error(&temp, source))?;
    fs::rename(&temp, path).map_err(|source| {
        let _ = fs::remove_file(&temp);
        io_error(path, source)
    })
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let source_root = fs::canonicalize(source).map_err(|error| io_error(source, error))?;
    copy_tree_from(&source_root, destination, &source_root)
}

fn copy_tree_from(source: &Path, destination: &Path, source_root: &Path) -> Result<()> {
    let mut entries = fs::read_dir(source)
        .map_err(|error| io_error(source, error))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| io_error(source, error))?;
    entries.sort_by_key(fs::DirEntry::file_name);

    for entry in entries {
        if entry.file_name() == INTEGRITY_FILE {
            continue;
        }
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from).map_err(|error| io_error(&from, error))?;
        if metadata.is_dir() {
            fs::create_dir(&to).map_err(|error| io_error(&to, error))?;
            copy_tree_from(&from, &to, source_root)?;
            fs::set_permissions(&to, metadata.permissions())
                .map_err(|error| io_error(&to, error))?;
        } else if metadata.is_file() {
            fs::copy(&from, &to).map_err(|error| io_error(&to, error))?;
            fs::set_permissions(&to, metadata.permissions())
                .map_err(|error| io_error(&to, error))?;
        } else if metadata.file_type().is_symlink() {
            copy_symlink(&from, &to, source_root)?;
        } else {
            return Err(io_error(
                &from,
                io::Error::other("bundle contains an unsupported file type"),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path, source_root: &Path) -> Result<()> {
    let target = fs::read_link(source).map_err(|error| io_error(source, error))?;
    if target.is_absolute() {
        return Err(unsafe_symlink(source, "has an absolute target"));
    }
    let resolved = source.parent().unwrap_or(source_root).join(&target);
    let canonical = fs::canonicalize(&resolved).map_err(|error| {
        unsafe_symlink(
            source,
            &format!("has an unresolved or cyclic target {target:?}: {error}"),
        )
    })?;
    if !canonical.starts_with(source_root) {
        return Err(unsafe_symlink(source, "escapes the bundle root"));
    }
    std::os::unix::fs::symlink(target, destination).map_err(|error| io_error(destination, error))
}

#[cfg(unix)]
fn unsafe_symlink(source: &Path, reason: &str) -> ComposeError {
    io_error(source, io::Error::other(format!("bundle symlink {reason}")))
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, _destination: &Path, _source_root: &Path) -> Result<()> {
    Err(io_error(
        source,
        io::Error::new(io::ErrorKind::Unsupported, "bundle symlinks require unix"),
    ))
}

fn remove_any(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(path).map_err(|source| io_error(path, source))
        }
        Ok(_) => fs::remove_file(path).map_err(|source| io_error(path, source)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(path, source)),
    }
}

fn io_error(path: &Path, source: io::Error) -> ComposeError {
    ComposeError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn bundle(root: &Path, identity: &str, install: Option<&str>) -> HostBundleSpec {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join(INTEGRITY_FILE), identity).unwrap();
        fs::write(root.join("worker.js"), "first").unwrap();
        HostBundleSpec {
            install_dir: root.to_path_buf(),
            install: install.map(str::to_string),
            run: "node worker.js".to_string(),
            env: BTreeMap::new(),
            has_base_image: false,
            has_resources: false,
        }
    }

    #[test]
    fn copies_bundle_and_reuses_mutable_workspace() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let spec = bundle(source.path(), "one", Some("npm install"));
        let first = prepare(&spec, state.path(), "worker").unwrap();
        assert!(first.install_required);
        fs::write(first.workspace.join("generated"), "keep").unwrap();
        mark_prepared(&spec, state.path(), "worker").unwrap();

        let second = prepare(&spec, state.path(), "worker").unwrap();
        assert!(!second.install_required);
        assert_eq!(
            fs::read_to_string(second.workspace.join("generated")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn changed_bundle_replaces_workspace_and_requires_install() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let spec = bundle(source.path(), "one", Some("npm install"));
        let first = prepare(&spec, state.path(), "worker").unwrap();
        fs::write(first.workspace.join("generated"), "discard").unwrap();
        mark_prepared(&spec, state.path(), "worker").unwrap();

        fs::write(source.path().join(INTEGRITY_FILE), "two").unwrap();
        fs::write(source.path().join("worker.js"), "second").unwrap();
        let second = prepare(&spec, state.path(), "worker").unwrap();
        assert!(second.install_required);
        assert!(!second.workspace.join("generated").exists());
        assert_eq!(
            fs::read_to_string(second.workspace.join("worker.js")).unwrap(),
            "second"
        );
    }

    #[test]
    fn recreated_workspace_requires_install_even_with_a_current_marker() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let spec = bundle(source.path(), "one", Some("npm install"));
        let first = prepare(&spec, state.path(), "worker").unwrap();
        mark_prepared(&spec, state.path(), "worker").unwrap();
        fs::remove_dir_all(&first.workspace).unwrap();

        let recreated = prepare(&spec, state.path(), "worker").unwrap();
        assert!(recreated.install_required);
    }

    #[test]
    fn changed_install_requires_install_without_replacing_workspace() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let first_spec = bundle(source.path(), "one", Some("npm install"));
        let first = prepare(&first_spec, state.path(), "worker").unwrap();
        fs::write(first.workspace.join("generated"), "keep").unwrap();
        mark_prepared(&first_spec, state.path(), "worker").unwrap();

        let mut second_spec = first_spec.clone();
        second_spec.install = Some("npm ci".to_string());
        let second = prepare(&second_spec, state.path(), "worker").unwrap();
        assert!(second.install_required);
        assert!(second.workspace.join("generated").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_stay_private_and_cannot_escape_the_bundle() {
        use std::os::unix::fs::symlink;

        let outer = tempfile::tempdir().unwrap();
        let source = outer.path().join("safe-bundle");
        let state = tempfile::tempdir().unwrap();
        let spec = bundle(&source, "one", None);
        symlink("worker.js", source.join("worker-link.js")).unwrap();

        let prepared = prepare(&spec, state.path(), "safe").unwrap();
        fs::write(prepared.workspace.join("worker-link.js"), "workspace").unwrap();
        assert_eq!(
            fs::read_to_string(source.join("worker.js")).unwrap(),
            "first"
        );
        assert_eq!(
            fs::read_to_string(prepared.workspace.join("worker.js")).unwrap(),
            "workspace"
        );

        let unsafe_source = outer.path().join("unsafe-bundle");
        let unsafe_spec = bundle(&unsafe_source, "two", None);
        fs::write(outer.path().join("outside"), "shared").unwrap();
        symlink("../outside", unsafe_source.join("outside-link")).unwrap();
        let error = prepare(&unsafe_spec, state.path(), "unsafe").unwrap_err();
        assert!(error.to_string().contains("escapes the bundle root"));
    }
}
