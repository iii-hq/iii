// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Strict parsing and offline validation for the root `worker-compose.yaml`.
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
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    dag,
    descriptor::{PackageDescriptor, WorkerDefinition},
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

pub const DEFAULT_ENGINE_URL: &str = "ws://127.0.0.1:49134";

pub const CONFIGURABLE_ENGINE_WORKERS: &[&str] = &[
    "configuration",
    "iii-worker-manager",
    "iii-http-functions",
    "iii-stream",
    "iii-sandbox",
];

/// Engine worker map keys may carry the engine's existing `#instance`
/// suffix so a strict YAML map can still represent more than one configured
/// instance of the same worker type.
pub fn engine_worker_type(name: &str) -> &str {
    name.split('#').next().unwrap_or(name)
}

/// Whether an engine worker key is either a bare type or one `#instance`.
pub fn valid_engine_worker_name(name: &str) -> bool {
    !name.contains('#')
        || name
            .split_once('#')
            .is_some_and(|(_, instance)| !instance.is_empty() && !instance.contains('#'))
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerSource {
    /// `package://<registry-host>/<name>`
    Package { reference: String },
    /// `catalog://<slug>`, compiled from the root worker catalog.
    Catalog {
        dir: PathBuf,
        declared: String,
        descriptor: PackageDescriptor,
    },
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
    pub start_after: Vec<String>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct EngineSpec {
    pub url: String,
    pub registration_namespace_grace_ms: Option<u64>,
    pub workers: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone)]
pub struct ComposeFile {
    pub stack: String,
    pub workers: IndexMap<String, PackageDescriptor>,
    /// Stable state identity for one stack within the root compose file.
    pub identity: PathBuf,
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
    /// Present when this Compose invocation owns the engine process. Absent
    /// projects must connect to an externally managed engine.
    pub engine: Option<EngineSpec>,
    pub containers: IndexMap<String, Container>,
}

impl ComposeFile {
    /// Reads and validates a compose file. Canonicalizes the path so the
    /// derived project namespace is stable regardless of how the operator
    /// spelled it.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self> {
        Self::load_stack(path, None)
    }

    pub fn load_stack(path: impl Into<PathBuf>, stack: Option<&str>) -> Result<Self> {
        let path = path.into();
        let text = std::fs::read_to_string(&path).map_err(|source| ComposeError::Io {
            path: path.clone(),
            source,
        })?;
        let canonical = std::fs::canonicalize(&path)
            .or_else(|_| std::path::absolute(&path))
            .map_err(|source| ComposeError::Io {
                path: path.clone(),
                source,
            })?;
        Self::parse_stack(&text, canonical, stack)
    }

    /// Compiles the package catalog without selecting a stack. Release tooling
    /// uses this path because a package descriptor is independent of runtime
    /// stack selection.
    pub fn load_catalog(path: impl Into<PathBuf>) -> Result<IndexMap<String, PackageDescriptor>> {
        let path = path.into();
        let text = std::fs::read_to_string(&path).map_err(|source| ComposeError::Io {
            path: path.clone(),
            source,
        })?;
        let canonical = std::fs::canonicalize(&path)
            .or_else(|_| std::path::absolute(&path))
            .map_err(|source| ComposeError::Io {
                path: path.clone(),
                source,
            })?;
        let base_dir = canonical
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let document = expanded_document(&text, &canonical)?;
        let expanded = serde_yaml::to_string(&document).map_err(|error| ComposeError::Yaml {
            path: canonical.clone(),
            message: error.to_string(),
        })?;
        let raw: RawComposeFile =
            serde_yaml::from_str(&expanded).map_err(|error| ComposeError::Yaml {
                path: canonical,
                message: error.to_string(),
            })?;
        let mut workers = IndexMap::with_capacity(raw.workers.len());
        for (name, definition) in raw.workers {
            workers.insert(
                name.clone(),
                PackageDescriptor::compile(&name, definition, &base_dir)?,
            );
        }
        Ok(workers)
    }

    /// Parses compose YAML that is already in memory. `path` is used for
    /// diagnostics and to resolve relative paths.
    pub fn parse(text: &str, path: impl Into<PathBuf>) -> Result<Self> {
        Self::parse_stack(text, path, None)
    }

    pub fn parse_stack(text: &str, path: impl Into<PathBuf>, stack: Option<&str>) -> Result<Self> {
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
        let document = expanded_document(text, &path)?;

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

        if raw.stacks.is_empty() {
            return Err(ComposeError::EmptyContainers);
        }

        let selected_stack = match stack {
            Some(stack) => stack.to_string(),
            None if raw.stacks.contains_key("default") => "default".to_string(),
            None if raw.stacks.len() == 1 => raw.stacks.keys().next().cloned().unwrap(),
            None => return Err(ComposeError::AmbiguousStack),
        };
        let raw_stack =
            raw.stacks
                .get(&selected_stack)
                .ok_or_else(|| ComposeError::UnknownStack {
                    stack: selected_stack.clone(),
                })?;
        let mut workers = IndexMap::with_capacity(raw.workers.len());
        for (name, definition) in raw.workers {
            workers.insert(
                name.clone(),
                PackageDescriptor::compile(&name, definition, &base_dir)?,
            );
        }

        // At load time, before a container starts: this is the namespace every
        // trigger against the project has to spell, so a value it cannot be is
        // a value nothing else should be built on.
        if let Some(name) = Some(raw_stack.namespace.as_str())
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

        let startup_timeout = DEFAULT_STARTUP_TIMEOUT;
        let stop_timeout = DEFAULT_STOP_TIMEOUT;
        let engine = None;

        let mut containers = IndexMap::with_capacity(raw_stack.containers.len());
        for (key, raw_container) in &raw_stack.containers {
            containers.insert(
                key.clone(),
                compile_container(key, raw_container, &workers, &base_dir, startup_timeout)?,
            );
        }

        let file = Self {
            stack: selected_stack.clone(),
            workers,
            identity: PathBuf::from(format!("{}#stack={selected_stack}", path.display())),
            namespace: Some(raw_stack.namespace.clone()),
            path,
            base_dir,
            startup_timeout,
            stop_timeout,
            engine,
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

/// Reads only the engine ownership section from a Compose document.
///
/// Mutation preflight and teardown paths use this to reject ownership changes
/// without requiring the container graph to be valid first. A cached project
/// must still be stoppable or repairable when an unrelated container edit is
/// temporarily invalid.
pub(crate) fn parse_engine_section(text: &str, path: &Path) -> Result<Option<EngineSpec>> {
    let _: RawComposeFile = serde_yaml::from_str(text).map_err(|err| ComposeError::Yaml {
        path: path.to_path_buf(),
        message: err.to_string(),
    })?;
    Ok(None)
}

fn compile_container(
    key: &str,
    raw: &RawContainer,
    workers: &IndexMap<String, PackageDescriptor>,
    base_dir: &Path,
    startup_timeout: Duration,
) -> Result<Container> {
    for dependency in &raw.start_after {
        if dependency == key {
            return Err(ComposeError::SelfDependency {
                container: key.to_string(),
            });
        }
    }
    let (worker, version, working_dir, environment, scripts) =
        if let Some(name) = raw.worker.strip_prefix("catalog://") {
            let descriptor = workers
                .get(name)
                .ok_or_else(|| ComposeError::UnknownCatalogWorker {
                    container: key.to_string(),
                    worker: name.to_string(),
                })?
                .clone();
            let dir = descriptor.source_dir(base_dir);
            let pre_run = (!descriptor.runtime.prepare.is_empty()).then(|| {
                descriptor
                    .runtime
                    .prepare
                    .iter()
                    .map(|command| shell_join(command))
                    .collect::<Vec<_>>()
                    .join(" && ")
            });
            (
                WorkerSource::Catalog {
                    dir: dir.clone(),
                    declared: raw.worker.clone(),
                    descriptor: descriptor.clone(),
                },
                Some(descriptor.version.clone()),
                Some(dir),
                descriptor.runtime.environment.clone(),
                Scripts {
                    pre_run,
                    ..Scripts::default()
                },
            )
        } else if let Some(reference) = raw.worker.strip_prefix("package://") {
            if reference.trim().is_empty() {
                return Err(ComposeError::UnsupportedWorkerSource {
                    container: key.into(),
                    source_uri: raw.worker.clone(),
                });
            }
            let (reference, version) = reference
                .rsplit_once('@')
                .map(|(name, version)| (name.to_string(), version.to_string()))
                .unwrap_or_else(|| (reference.to_string(), "latest".to_string()));
            (
                WorkerSource::Package { reference },
                Some(version),
                None,
                BTreeMap::new(),
                Scripts::default(),
            )
        } else {
            return Err(ComposeError::UnsupportedWorkerSource {
                container: key.into(),
                source_uri: raw.worker.clone(),
            });
        };
    Ok(Container {
        worker,
        version,
        start_after: raw.start_after.clone(),
        config_name: raw.config.as_ref().map(|_| key.to_string()),
        config_override: raw.config.clone(),
        scripts,
        working_dir,
        environment,
        env_file: Vec::new(),
        startup_timeout,
    })
}

fn shell_join(command: &[String]) -> String {
    command
        .iter()
        .map(|part| format!("'{}'", part.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn expanded_document(text: &str, path: &Path) -> Result<serde_yaml::Value> {
    let mut document: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|err| ComposeError::Yaml {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
    crate::interpolate::expand_tree(&mut document, path, &|name| std::env::var(name).ok())?;
    Ok(document)
}

impl Container {
    /// Directory of a local catalog worker. `None` for packages, which have no
    /// local directory until registry resolution exists.
    pub fn worker_dir(&self) -> Option<&std::path::Path> {
        match &self.worker {
            WorkerSource::Catalog { dir, .. } => Some(dir.as_path()),
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawComposeFile {
    #[serde(default, deserialize_with = "deserialize_unique_map")]
    #[schemars(with = "BTreeMap<String, WorkerDefinition>")]
    workers: IndexMap<String, WorkerDefinition>,
    #[serde(default, deserialize_with = "deserialize_unique_map")]
    #[schemars(with = "BTreeMap<String, RawStack>")]
    stacks: IndexMap<String, RawStack>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawStack {
    namespace: String,
    #[serde(default, deserialize_with = "deserialize_unique_map")]
    #[schemars(with = "BTreeMap<String, RawContainer>")]
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawContainer {
    worker: String,
    #[serde(default)]
    start_after: Vec<String>,
    #[serde(default)]
    #[schemars(with = "Option<serde_json::Value>")]
    config: Option<serde_yaml::Value>,
}

/// JSON Schema for the operator-authored `worker-compose.yaml` document.
///
/// The deserialization types are the source of truth, so a field added to the
/// parser also appears in `compose::schema` without a second hand-written
/// contract to update.
pub(crate) fn worker_compose_schema_json() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(RawComposeFile)).unwrap_or(serde_json::Value::Null)
}

/// A complete small project returned beside the file schema. The registry
/// package is deferred by offline validation, so the example does not require
/// a local worker directory to be useful.
pub(crate) fn worker_compose_example_json() -> serde_json::Value {
    serde_json::json!({
        "worker-compose.yaml": "workers: {}\nstacks:\n  default:\n    namespace: app\n    containers:\n      state:\n        worker: package://api.workers.iii.dev/state@0.21.4\n"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_top_level_shape_is_rejected() {
        let text = r#"
engine:
  url: ws://127.0.0.1:49134
containers:
  api:
    worker: path://./api
"#;

        assert!(parse_engine_section(text, Path::new("worker-compose.yaml")).is_err());
    }

    #[test]
    fn path_source_is_rejected_by_strict_stack_contract() {
        let text = "workers: {}\nstacks:\n  default:\n    namespace: app\n    containers:\n      api:\n        worker: path://./api\n";
        assert!(matches!(
            ComposeFile::parse(text, "/tmp/worker-compose.yaml"),
            Err(ComposeError::UnsupportedWorkerSource { .. })
        ));
    }

    #[test]
    fn worker_compose_schema_and_example_follow_the_parser() {
        let schema = worker_compose_schema_json();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["workers"].is_object());
        assert!(schema["properties"]["stacks"].is_object());
        assert!(schema["properties"]["engine"].is_null());

        let example = worker_compose_example_json();
        let text = example["worker-compose.yaml"].as_str().unwrap();
        let parsed = ComposeFile::parse(text, "/tmp/worker-compose.yaml").unwrap();
        assert!(parsed.containers.contains_key("state"));
    }
}
