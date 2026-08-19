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

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use indexmap::IndexMap;
use serde::Deserialize;

use crate::{
    dag,
    error::{ComposeError, Result},
    spawn::RESERVED_ENV,
};

/// Default `pre_run` budget. A blocking migration or asset build routinely
/// takes tens of seconds; anything past this is treated as hung.
pub const DEFAULT_PRE_RUN_TIMEOUT: Duration = Duration::from_secs(60);

/// Default readiness budget: how long `up` waits for a spawned container to
/// show up in the engine before calling it failed.
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

/// Default teardown grace between the polite stop and the forced kill.
pub const DEFAULT_STOP_TIMEOUT: Duration = crate::process::DEFAULT_STOP_GRACE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerSource {
    /// `package://<registry-host>/<name>`
    Package { reference: String },
    /// `path://<dir>`, resolved against the compose file's directory.
    Path { dir: PathBuf, declared: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scripts {
    pub pre_run: Option<String>,
    pub pre_run_timeout: Duration,
    pub run: Option<String>,
    pub post_run: Option<String>,
}

impl Default for Scripts {
    fn default() -> Self {
        Self {
            pre_run: None,
            pre_run_timeout: DEFAULT_PRE_RUN_TIMEOUT,
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
    /// The configuration entry this container owns.
    ///
    /// Not a source. Compose fetches it as the base, publishes the merged
    /// result back to it, and tells the child which entry is its own through
    /// `III_CONFIG_NAME`. *How* a configuration is read and stored belongs to
    /// the configuration worker, which has its own adapter for that, so this
    /// says which configuration and nothing about where it lives.
    pub config_name: Option<String>,
    pub config_override: Option<serde_yaml::Value>,
    pub scripts: Scripts,
    /// Declared working directory, resolved against the compose file's
    /// directory. `None` means "the worker's own directory".
    pub working_dir: Option<PathBuf>,
    /// Literal environment for this container. Never contains a reserved key:
    /// those are rejected at parse time.
    pub environment: BTreeMap<String, String>,
    /// Env files in declaration order; a later file wins. Resolved against the
    /// compose file's directory and read at spawn time, never at parse time —
    /// they routinely hold secrets.
    pub env_file: Vec<PathBuf>,
    /// Readiness budget for this container: its own override, else the file's.
    pub startup_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct ComposeFile {
    /// The namespace this project registers in, when it declares one. Nothing
    /// is derived from it: what the file says is what the engine sees.
    pub namespace: Option<String>,
    /// Canonical path of the compose file. Resolved once so the state binding
    /// is stable regardless of how the operator spelled the path.
    pub path: PathBuf,
    /// Directory every relative path in the file resolves against.
    pub base_dir: PathBuf,
    /// Project-wide readiness budget; a container may override it.
    pub startup_timeout: Duration,
    /// Grace between the polite stop and the forced kill, project-wide.
    pub stop_timeout: Duration,
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

        // Read as a document, expanded, then read as a compose file. The two
        // steps are what let `config_override` keep its own `${VAR}`: which
        // block a reference sits in is only knowable once the shape is.
        //
        // The file on disk is never rewritten. This is the text compose reads,
        // not the text the operator keeps.
        let mut document: serde_yaml::Value =
            serde_yaml::from_str(text).map_err(|err| ComposeError::Yaml {
                path: path.clone(),
                message: err.to_string(),
            })?;
        crate::interpolate::expand_tree(&mut document, &path, &|name| std::env::var(name).ok())?;

        // Back to text before the compose file is read out of it. Reading the
        // document directly would tighten the types: YAML says `30` is a
        // number, and `startup_timeout: 30` would fail as a malformed file
        // rather than as the duration without a unit that it is.
        let expanded = serde_yaml::to_string(&document).map_err(|err| ComposeError::Yaml {
            path: path.clone(),
            message: err.to_string(),
        })?;
        let raw: RawComposeFile =
            serde_yaml::from_str(&expanded).map_err(|err| ComposeError::Yaml {
                path: path.clone(),
                message: err.to_string(),
            })?;

        if raw.containers.is_empty() {
            return Err(ComposeError::EmptyContainers);
        }

        // At load time, before a container starts: this is the namespace every
        // trigger against the project has to spell, so a value it cannot be is
        // a value nothing else should be built on.
        if let Some(name) = raw
            .namespace
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            && let Err(reason) = crate::namespace::check(name)
        {
            return Err(ComposeError::InvalidNamespace {
                namespace: name.to_string(),
                reason,
            });
        }

        let startup_timeout = file_duration(
            "startup_timeout",
            &raw.startup_timeout,
            DEFAULT_STARTUP_TIMEOUT,
        )?;
        let stop_timeout = file_duration("stop_timeout", &raw.stop_timeout, DEFAULT_STOP_TIMEOUT)?;

        let mut containers = IndexMap::with_capacity(raw.containers.len());
        for (key, raw_container) in &raw.containers {
            containers.insert(
                key.clone(),
                validate_container(key, raw_container, &base_dir, startup_timeout)?,
            );
        }

        let file = Self {
            namespace: raw.namespace,
            path,
            base_dir,
            startup_timeout,
            stop_timeout,
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

fn validate_container(
    key: &str,
    raw: &RawContainer,
    base_dir: &Path,
    file_startup_timeout: Duration,
) -> Result<Container> {
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

    let config_name = raw.config_name.clone();

    let scripts = match &raw.scripts {
        None => Scripts::default(),
        Some(raw_scripts) => {
            if raw_scripts.run.is_some() && is_package {
                return Err(ComposeError::RunNotAllowedForPackage {
                    container: key.to_string(),
                });
            }
            if raw_scripts.pre_run_timeout.is_some() && raw_scripts.pre_run.is_none() {
                return Err(ComposeError::PreRunTimeoutWithoutPreRun {
                    container: key.to_string(),
                });
            }
            let pre_run_timeout = match &raw_scripts.pre_run_timeout {
                None => DEFAULT_PRE_RUN_TIMEOUT,
                Some(value) => {
                    parse_duration(value).ok_or_else(|| ComposeError::InvalidDuration {
                        container: key.to_string(),
                        value: value.clone(),
                    })?
                }
            };
            Scripts {
                pre_run: raw_scripts.pre_run.clone(),
                pre_run_timeout,
                run: raw_scripts.run.clone(),
                post_run: raw_scripts.post_run.clone(),
            }
        }
    };

    // The reserved contract is the daemon's to set. Silently dropping a
    // user-supplied III_URL would look like it took effect.
    let mut environment = BTreeMap::new();
    for (name, value) in &raw.environment {
        if RESERVED_ENV.contains(&name.as_str()) {
            return Err(ComposeError::ReservedEnvOverride {
                container: key.to_string(),
                name: name.clone(),
            });
        }
        environment.insert(name.clone(), value.clone());
    }

    let startup_timeout = match &raw.startup_timeout {
        None => file_startup_timeout,
        Some(value) => parse_duration(value).ok_or_else(|| ComposeError::InvalidDuration {
            container: key.to_string(),
            value: value.clone(),
        })?,
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
        environment,
        env_file: raw
            .env_file
            .iter()
            .map(|path| resolve_relative(base_dir, path))
            .collect(),
        startup_timeout,
    })
}

impl Container {
    /// Directory of a `path://` worker. `None` for packages, which have no
    /// local directory until registry resolution exists.
    pub fn worker_dir(&self) -> Option<&std::path::Path> {
        match &self.worker {
            WorkerSource::Path { dir, .. } => Some(dir.as_path()),
            WorkerSource::Package { .. } => None,
        }
    }

    /// The user-defined environment for this container: env files in listed
    /// order, then literal `environment` values on top.
    ///
    /// Read at spawn time, not at parse time: env files hold secrets, and
    /// holding them in memory for the daemon's whole life buys nothing.
    pub fn resolve_user_env(&self, container_key: &str) -> Result<BTreeMap<String, String>> {
        let mut env = BTreeMap::new();
        for path in &self.env_file {
            let text = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
                path: path.clone(),
                source,
            })?;
            for (name, value) in parse_env_file(&text) {
                if RESERVED_ENV.contains(&name.as_str()) {
                    return Err(ComposeError::ReservedEnvOverride {
                        container: container_key.to_string(),
                        name,
                    });
                }
                env.insert(name, value);
            }
        }
        env.extend(self.environment.clone());
        Ok(env)
    }
}

/// `KEY=VALUE` lines. Blank lines and `#` comments are skipped, a leading
/// `export ` is tolerated, and one layer of matching quotes is stripped.
pub(crate) fn parse_env_file(text: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|rest| rest.strip_suffix('\''))
            })
            .unwrap_or(value);
        entries.push((name.to_string(), value.to_string()));
    }
    entries
}

/// Parses a file-level duration, which has no container to blame in the error.
fn file_duration(field: &str, raw: &Option<String>, default: Duration) -> Result<Duration> {
    match raw {
        None => Ok(default),
        Some(value) => parse_duration(value).ok_or_else(|| ComposeError::InvalidDuration {
            container: format!("<{field}>"),
            value: value.clone(),
        }),
    }
}

fn parse_worker_source(key: &str, value: &str, base_dir: &Path) -> Result<WorkerSource> {
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

fn resolve_relative(base_dir: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    normalize(&joined)
}

/// Drops `.` components so diagnostics read `/srv/app/workers/api` instead of
/// `/srv/app/./workers/api`. `..` is left alone: resolving it lexically would
/// lie in the presence of symlinks, and these paths are shown to operators.
fn normalize(path: &Path) -> PathBuf {
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
    // `ms` first: `s` is a suffix of it.
    let (digits, factor_ms) = None
        .or_else(|| value.strip_suffix("ms").map(|rest| (rest, 1_u64)))
        .or_else(|| value.strip_suffix('s').map(|rest| (rest, 1_000)))
        .or_else(|| value.strip_suffix('m').map(|rest| (rest, 60_000)))?;
    let amount: u64 = digits.trim().parse().ok()?;
    amount.checked_mul(factor_ms).map(Duration::from_millis)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawComposeFile {
    /// Optional: a project that names itself nowhere lands in `default`, the
    /// same rule the rest of the engine follows.
    ///
    /// Spelled `namespace:` rather than `name:` because that is what it sets.
    /// The value is typed back into `iii trigger --namespace` and into every
    /// `worker.trigger` call, so the field is named after the coordinate it
    /// feeds rather than read as a display label the project happens to carry.
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    startup_timeout: Option<String>,
    #[serde(default)]
    stop_timeout: Option<String>,
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
    config_override: Option<serde_yaml::Value>,
    #[serde(default)]
    scripts: Option<RawScripts>,
    #[serde(default)]
    working_dir: Option<PathBuf>,
    #[serde(default, deserialize_with = "deserialize_unique_map")]
    environment: IndexMap<String, String>,
    #[serde(default)]
    env_file: Vec<PathBuf>,
    #[serde(default)]
    startup_timeout: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScripts {
    #[serde(default)]
    pre_run: Option<String>,
    #[serde(default)]
    pre_run_timeout: Option<String>,
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
