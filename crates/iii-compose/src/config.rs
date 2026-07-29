// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `worker-compose.yaml` v1: strict parsing and offline validation.
//!
//! Unknown fields are hard errors in every position (top level, container,
//! scripts). Accepting an unknown key silently is how a typo becomes a
//! silently-ignored dependency.
//!
//! Parsing here never touches the filesystem beyond reading the compose file
//! itself: source existence and manifest resolution live in [`crate::manifest`]
//! so that schema tests stay hermetic.

use std::{path::PathBuf, time::Duration};

use indexmap::IndexMap;
use serde::Deserialize;

use crate::{
    dag,
    error::{ComposeError, Result},
};

/// Default `pre_start` budget. A blocking migration or asset build routinely
/// takes tens of seconds; anything past this is treated as hung.
pub const DEFAULT_PRE_START_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerSource {
    /// `package://<registry-host>/<name>`
    Package { reference: String },
    /// `path://<dir>`, resolved against the compose file's directory.
    Path { dir: PathBuf, declared: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scripts {
    pub pre_start: Option<String>,
    pub pre_start_timeout: Duration,
    pub run: Option<String>,
    pub post_run: Option<String>,
}

impl Default for Scripts {
    fn default() -> Self {
        Self {
            pre_start: None,
            pre_start_timeout: DEFAULT_PRE_START_TIMEOUT,
            run: None,
            post_run: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Container {
    pub worker: WorkerSource,
    pub version: Option<String>,
    pub depends_on: Vec<String>,
    /// Configuration entry to fetch from the configuration worker. Both
    /// `config_name` and the v1 `config_uri` form collapse into this name.
    pub config_name: Option<String>,
    pub config_override: Option<serde_yaml::Value>,
    pub scripts: Scripts,
    /// Declared working directory, resolved against the compose file's
    /// directory. `None` means "the worker's own directory".
    pub working_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ComposeFile {
    pub name: String,
    /// Canonical path of the compose file. Also the seed of the project
    /// namespace, so it must be resolved once and never re-derived.
    pub path: PathBuf,
    /// Directory every relative path in the file resolves against.
    pub base_dir: PathBuf,
    pub containers: IndexMap<String, Container>,
}

impl ComposeFile {
    /// Reads and validates a compose file. Canonicalizes the path so the
    /// derived project namespace is stable regardless of how the operator
    /// spelled it.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let text = std::fs::read_to_string(&path).map_err(|source| ComposeError::Io {
            path: path.clone(),
            source,
        })?;
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        Self::parse(&text, canonical)
    }

    /// Parses compose YAML that is already in memory. `path` is used for
    /// diagnostics and to resolve relative paths.
    pub fn parse(text: &str, path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let base_dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        let raw: RawComposeFile = serde_yaml::from_str(text).map_err(|err| ComposeError::Yaml {
            path: path.clone(),
            message: err.to_string(),
        })?;

        if raw.containers.is_empty() {
            return Err(ComposeError::EmptyContainers);
        }

        let mut containers = IndexMap::with_capacity(raw.containers.len());
        for (key, raw_container) in &raw.containers {
            containers.insert(
                key.clone(),
                validate_container(key, raw_container, &base_dir)?,
            );
        }

        let file = Self {
            name: raw.name,
            path,
            base_dir,
            containers,
        };
        dag::validate_dependencies(&file)?;
        Ok(file)
    }

    /// Containers in start order: every dependency precedes its dependents.
    pub fn start_order(&self) -> Result<Vec<String>> {
        dag::topo_order(self)
    }
}

fn validate_container(key: &str, raw: &RawContainer, base_dir: &PathBuf) -> Result<Container> {
    let worker = parse_worker_source(key, &raw.worker, base_dir)?;
    let is_package = matches!(worker, WorkerSource::Package { .. });

    if is_package && raw.version.is_none() {
        return Err(ComposeError::MissingVersionForPackage {
            container: key.to_string(),
        });
    }

    for dependency in &raw.depends_on {
        if dependency == key {
            return Err(ComposeError::SelfDependency {
                container: key.to_string(),
            });
        }
    }

    if raw.config_name.is_some() && raw.config_uri.is_some() {
        return Err(ComposeError::ConflictingConfigSource {
            container: key.to_string(),
        });
    }
    let config_name = match (&raw.config_name, &raw.config_uri) {
        (Some(name), _) => Some(name.clone()),
        (None, Some(uri)) => Some(parse_config_uri(key, uri)?),
        (None, None) => None,
    };

    let scripts = match &raw.scripts {
        None => Scripts::default(),
        Some(raw_scripts) => {
            if raw_scripts.run.is_some() && is_package {
                return Err(ComposeError::RunNotAllowedForPackage {
                    container: key.to_string(),
                });
            }
            if raw_scripts.pre_start_timeout.is_some() && raw_scripts.pre_start.is_none() {
                return Err(ComposeError::PreStartTimeoutWithoutPreStart {
                    container: key.to_string(),
                });
            }
            let pre_start_timeout = match &raw_scripts.pre_start_timeout {
                None => DEFAULT_PRE_START_TIMEOUT,
                Some(value) => {
                    parse_duration(value).ok_or_else(|| ComposeError::InvalidDuration {
                        container: key.to_string(),
                        value: value.clone(),
                    })?
                }
            };
            Scripts {
                pre_start: raw_scripts.pre_start.clone(),
                pre_start_timeout,
                run: raw_scripts.run.clone(),
                post_run: raw_scripts.post_run.clone(),
            }
        }
    };

    Ok(Container {
        worker,
        version: raw.version.clone(),
        depends_on: raw.depends_on.clone(),
        config_name,
        config_override: raw.config_override.clone(),
        scripts,
        working_dir: raw
            .working_dir
            .as_ref()
            .map(|dir| resolve_relative(base_dir, dir)),
    })
}

fn parse_worker_source(key: &str, value: &str, base_dir: &PathBuf) -> Result<WorkerSource> {
    if let Some(rest) = value.strip_prefix("path://") {
        if rest.is_empty() {
            return Err(ComposeError::UnsupportedWorkerSource {
                container: key.to_string(),
                source_uri: value.to_string(),
            });
        }
        return Ok(WorkerSource::Path {
            dir: resolve_relative(base_dir, &PathBuf::from(rest)),
            declared: value.to_string(),
        });
    }
    if let Some(rest) = value.strip_prefix("package://") {
        if rest.is_empty() {
            return Err(ComposeError::UnsupportedWorkerSource {
                container: key.to_string(),
                source_uri: value.to_string(),
            });
        }
        return Ok(WorkerSource::Package {
            reference: rest.to_string(),
        });
    }
    Err(ComposeError::UnsupportedWorkerSource {
        container: key.to_string(),
        source_uri: value.to_string(),
    })
}

/// v1 accepts exactly one `config_uri` shape: the configuration worker. Every
/// other scheme (`file://`, direct adapters) waits for its own phase, because
/// the configuration worker is the adapter-agnostic entry point.
fn parse_config_uri(key: &str, uri: &str) -> Result<String> {
    let name = uri
        .strip_prefix("worker://configuration/get/")
        .filter(|name| !name.is_empty() && !name.contains('/'));
    name.map(str::to_string)
        .ok_or_else(|| ComposeError::UnsupportedConfigUri {
            container: key.to_string(),
            uri: uri.to_string(),
        })
}

fn resolve_relative(base_dir: &PathBuf, path: &PathBuf) -> PathBuf {
    let joined = if path.is_absolute() {
        path.clone()
    } else {
        base_dir.join(path)
    };
    normalize(&joined)
}

/// Drops `.` components so diagnostics read `/srv/app/workers/api` instead of
/// `/srv/app/./workers/api`. `..` is left alone: resolving it lexically would
/// lie in the presence of symlinks, and these paths are shown to operators.
fn normalize(path: &PathBuf) -> PathBuf {
    use std::path::Component;

    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// `500ms`, `30s`, `2m`. Units are mandatory: a bare number reads as seconds to
/// one person and milliseconds to the next.
pub fn parse_duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    let (digits, factor_ms) = if let Some(rest) = value.strip_suffix("ms") {
        (rest, 1_u64)
    } else if let Some(rest) = value.strip_suffix('s') {
        (rest, 1_000)
    } else if let Some(rest) = value.strip_suffix('m') {
        (rest, 60_000)
    } else {
        return None;
    };
    let amount: u64 = digits.trim().parse().ok()?;
    amount.checked_mul(factor_ms).map(Duration::from_millis)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawComposeFile {
    name: String,
    #[serde(deserialize_with = "deserialize_unique_map")]
    containers: IndexMap<String, RawContainer>,
}

/// YAML mappings tolerate duplicate keys by keeping the last one, which would
/// silently drop a declared worker. Container keys are identities here, so a
/// repeat is a hard error.
fn deserialize_unique_map<'de, D, V>(
    deserializer: D,
) -> std::result::Result<IndexMap<String, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    use serde::de::{Error as DeError, MapAccess, Visitor};

    struct UniqueMap<V>(std::marker::PhantomData<V>);

    impl<'de, V: Deserialize<'de>> Visitor<'de> for UniqueMap<V> {
        type Value = IndexMap<String, V>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a mapping with unique keys")
        }

        fn visit_map<A: MapAccess<'de>>(
            self,
            mut access: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut out = IndexMap::new();
            while let Some((key, value)) = access.next_entry::<String, V>()? {
                if out.contains_key(&key) {
                    return Err(A::Error::custom(format!("duplicate key '{key}'")));
                }
                out.insert(key, value);
            }
            Ok(out)
        }
    }

    deserializer.deserialize_map(UniqueMap(std::marker::PhantomData))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContainer {
    worker: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    config_name: Option<String>,
    #[serde(default)]
    config_uri: Option<String>,
    #[serde(default)]
    config_override: Option<serde_yaml::Value>,
    #[serde(default)]
    scripts: Option<RawScripts>,
    #[serde(default)]
    working_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScripts {
    #[serde(default)]
    pre_start: Option<String>,
    #[serde(default)]
    pre_start_timeout: Option<String>,
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    post_run: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units_on_durations() {
        assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("2m"), Some(Duration::from_secs(120)));
        assert_eq!(parse_duration("30"), None);
        assert_eq!(parse_duration("later"), None);
    }

    #[test]
    fn config_uri_accepts_only_the_configuration_worker_form() {
        assert_eq!(
            parse_config_uri("api", "worker://configuration/get/orders-api").unwrap(),
            "orders-api"
        );
        let err = parse_config_uri("api", "file://./config/api.yaml").unwrap_err();
        assert_eq!(err.code(), "UNSUPPORTED_CONFIG_URI");
    }

    #[test]
    fn path_sources_resolve_against_the_compose_directory() {
        let source =
            parse_worker_source("api", "path://./workers/api", &PathBuf::from("/srv/app")).unwrap();
        match source {
            WorkerSource::Path { dir, .. } => {
                assert_eq!(dir, PathBuf::from("/srv/app/workers/api"))
            }
            other => panic!("expected a path source, got {other:?}"),
        }
    }
}
