// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Configuration merge and delivery.
//!
//! Config is resolved by the daemon *before* the child starts and handed over as
//! a file path in `III_CONFIG`, so a worker never needs credentials to fetch its
//! own configuration and the daemon can fail a container before spawning it.
//!
//! Fetching the base entry from the configuration worker needs an engine
//! connection and is not wired yet; merge and delivery are.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use crate::error::{ComposeError, Result};

/// Merges `config_override` onto a fetched base.
///
/// Maps merge key by key. Arrays and scalars replace wholesale — half-merged
/// lists are never what an operator means. An explicit `null` is a value that
/// replaces, not a delete operator.
pub fn merge(base: serde_yaml::Value, override_value: serde_yaml::Value) -> serde_yaml::Value {
    match (base, override_value) {
        (serde_yaml::Value::Mapping(mut base), serde_yaml::Value::Mapping(overrides)) => {
            for (key, value) in overrides {
                // Merged in place so an override never reshuffles the document:
                // the delivered file stays diffable against the base.
                match base.get_mut(&key) {
                    Some(slot) => {
                        let existing = std::mem::replace(slot, serde_yaml::Value::Null);
                        *slot = merge(existing, value);
                    }
                    None => {
                        base.insert(key, value);
                    }
                }
            }
            serde_yaml::Value::Mapping(base)
        }
        (_, override_value) => override_value,
    }
}

/// A resolved configuration file owned by the daemon.
///
/// Created `0600` because resolved configuration routinely contains secrets:
/// the child reads it, nothing else on the host should.
#[derive(Debug)]
pub struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    pub fn write(dir: &Path, container_key: &str, value: &serde_yaml::Value) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|source| ComposeError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        // `create_dir_all` takes the process umask, which is usually 0755. The
        // files inside are 0600, but a directory anyone can list and enter is
        // half a boundary — and compose publishes this directory into a bundle
        // VM, so its mode travels with it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(
                |source| ComposeError::Io {
                    path: dir.to_path_buf(),
                    source,
                },
            )?;
        }

        let path = dir.join(format!("{container_key}.yaml"));
        let text = serde_yaml::to_string(value).map_err(|err| ComposeError::Yaml {
            path: path.clone(),
            message: err.to_string(),
        })?;

        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|source| ComposeError::Io {
            path: path.clone(),
            source,
        })?;
        // `mode` above applies to a file this call creates. One already there
        // keeps whatever it had, so a run that wrote it under a looser umask
        // would leave resolved secrets readable. Set it on the handle every
        // time, before anything is written into it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|source| ComposeError::Io {
                    path: path.clone(),
                    source,
                })?;
        }
        file.write_all(text.as_bytes())
            .map_err(|source| ComposeError::Io {
                path: path.clone(),
                source,
            })?;

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(text: &str) -> serde_yaml::Value {
        serde_yaml::from_str(text).unwrap()
    }

    #[test]
    fn maps_merge_key_by_key() {
        let merged = merge(
            yaml("server:\n  port: 8080\n  host: a\n"),
            yaml("server:\n  port: 3000\n"),
        );
        assert_eq!(merged, yaml("server:\n  port: 3000\n  host: a\n"));
    }

    #[test]
    fn arrays_and_scalars_replace() {
        assert_eq!(
            merge(yaml("hosts: [a, b, c]\n"), yaml("hosts: [d]\n")),
            yaml("hosts: [d]\n")
        );
        assert_eq!(
            merge(yaml("level: info\n"), yaml("level: debug\n")),
            yaml("level: debug\n")
        );
    }

    #[test]
    fn explicit_null_is_a_value_not_a_delete() {
        let merged = merge(yaml("server:\n  tls: on\n"), yaml("server:\n  tls: null\n"));
        assert_eq!(merged, yaml("server:\n  tls: null\n"));
    }

    #[test]
    fn new_keys_are_added() {
        let merged = merge(yaml("a: 1\n"), yaml("b: 2\n"));
        assert_eq!(merged, yaml("a: 1\nb: 2\n"));
    }

    #[test]
    fn overriding_a_key_keeps_its_position() {
        let merged = merge(yaml("a: 1\nb: 2\nc: 3\n"), yaml("b: 20\n"));
        let keys: Vec<String> = match &merged {
            serde_yaml::Value::Mapping(map) => map
                .keys()
                .map(|key| key.as_str().unwrap().to_string())
                .collect(),
            other => panic!("expected a mapping, got {other:?}"),
        };

        assert_eq!(keys, vec!["a", "b", "c"]);
        assert_eq!(merged, yaml("a: 1\nb: 20\nc: 3\n"));
    }

    #[test]
    fn the_written_file_is_owner_only() {
        let tmp = tempfile::tempdir().unwrap();
        let file = ConfigFile::write(
            &tmp.path().join("state"),
            "api",
            &yaml("server:\n  port: 3000\n"),
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(file.path()).unwrap(),
            "server:\n  port: 3000\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "resolved config must be owner-only");
        }
    }
    #[cfg(unix)]
    #[test]
    fn resolved_configuration_is_owner_only_even_when_it_was_already_there() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("config");

        // A file left by an earlier run under a looser umask. `OpenOptions`
        // only applies its mode when it creates the file, so this is the case
        // that used to keep the wide mode and the resolved secrets with it.
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("api.yaml");
        std::fs::write(&path, "stale: true\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        ConfigFile::write(&dir, "api", &yaml("server:\n  port: 3000\n")).unwrap();

        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600, "the file must not stay group readable");
        assert_eq!(
            mode(&dir),
            0o700,
            "nor may the directory holding it be listable"
        );
    }
}
