// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ComposeError, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkerDefinition {
    pub source: PackageSource,
    pub artifact: Artifact,
    #[serde(default)]
    pub runtime: Runtime,
    pub registry: RegistryDefinition,
    pub validation: Validation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageSource {
    pub path: String,
    pub package_manifest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Artifact {
    RustBinary {
        binary: String,
        targets: Vec<String>,
    },
    JavascriptBundle {
        build_command: Vec<String>,
        include: Vec<String>,
    },
    PythonBundle {
        build_command: Vec<String>,
        include: Vec<String>,
    },
    OciImage {
        context: String,
        dockerfile: String,
        platforms: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Runtime {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_image: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prepare: Vec<Vec<String>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Resources>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryDefinition {
    pub description: String,
    pub license: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<RegistryConfigFile>,
    #[serde(default = "default_publish")]
    pub publish: bool,
}

fn default_publish() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryConfigFile {
    pub defaults_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryMetadata {
    pub description: String,
    pub license: String,
    pub tags: Vec<String>,
    pub dependencies: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<RegistryConfig>,
    pub publish: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryConfig {
    pub defaults: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Validation {
    pub interface: ValidationMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValidationMode {
    Required,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageDescriptor {
    pub name: String,
    pub version: String,
    pub source: PackageSource,
    pub artifact: Artifact,
    pub runtime: Runtime,
    pub registry: RegistryMetadata,
    pub validation: Validation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildUnit {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platforms: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseDescriptor {
    pub contract: String,
    pub worker: String,
    pub version: String,
    pub source_sha: String,
    pub descriptor_sha256: String,
    pub package: PackageDescriptor,
    pub build_units: Vec<BuildUnit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseDescriptorIndex {
    pub contract: String,
    pub source_sha: String,
    pub compiler_sha: String,
    pub workers: BTreeMap<String, ReleaseDescriptorIndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseDescriptorIndexEntry {
    pub path: String,
    pub digest: String,
    pub version: String,
    pub publish: bool,
}

impl PackageDescriptor {
    pub fn compile(name: &str, definition: WorkerDefinition, base_dir: &Path) -> Result<Self> {
        if !valid_name(name) {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: "worker map key must match [a-z0-9][a-z0-9_-]*".into(),
            });
        }
        validate_source_paths(name, &definition.source)?;
        let root =
            base_dir
                .canonicalize()
                .map_err(|error| ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: format!("cannot canonicalize compose root: {error}"),
                })?;
        let source_dir = resolve(&root, &definition.source.path)
            .canonicalize()
            .map_err(|_| ComposeError::MissingWorkerDirectory {
                container: name.into(),
                path: resolve(&root, &definition.source.path),
            })?;
        if !source_dir.starts_with(&root) {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: "source.path escapes the compose root".into(),
            });
        }
        let manifest = resolve(&source_dir, &definition.source.package_manifest)
            .canonicalize()
            .map_err(|error| ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: format!("cannot resolve package manifest: {error}"),
            })?;
        if !manifest.starts_with(&source_dir) {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: "source.package_manifest escapes source.path".into(),
            });
        }
        if !manifest.is_file() {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: "source.package_manifest must be a regular file".into(),
            });
        }
        let version = read_package_version(&manifest).map_err(|message| {
            ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message,
            }
        })?;
        validate_definition(name, &definition, &source_dir)?;
        let registry = compile_registry(name, definition.registry, &source_dir)?;
        Ok(Self {
            name: name.into(),
            version,
            source: definition.source,
            artifact: definition.artifact,
            runtime: definition.runtime,
            registry,
            validation: definition.validation,
        })
    }

    pub fn sha256(&self) -> String {
        let value = serde_json::to_value(self).expect("descriptor is JSON");
        hex::encode(Sha256::digest(canonical_json(&value).as_bytes()))
    }

    pub fn source_dir(&self, base_dir: &Path) -> PathBuf {
        resolve(base_dir, &self.source.path)
    }

    pub fn release_descriptor(&self, source_sha: impl Into<String>) -> ReleaseDescriptor {
        let build_units = match &self.artifact {
            Artifact::RustBinary { targets, .. } => targets
                .iter()
                .map(|target| BuildUnit {
                    id: format!("{}-{target}", self.name),
                    kind: "rust-binary".into(),
                    target: Some(target.clone()),
                    platforms: None,
                })
                .collect(),
            Artifact::JavascriptBundle { .. } => vec![BuildUnit {
                id: self.name.clone(),
                kind: "javascript-bundle".into(),
                target: None,
                platforms: None,
            }],
            Artifact::PythonBundle { .. } => vec![BuildUnit {
                id: self.name.clone(),
                kind: "python-bundle".into(),
                target: None,
                platforms: None,
            }],
            Artifact::OciImage { platforms, .. } => vec![BuildUnit {
                id: self.name.clone(),
                kind: "oci-image".into(),
                target: None,
                platforms: Some(platforms.clone()),
            }],
        };
        ReleaseDescriptor {
            contract: "release-descriptor".into(),
            worker: self.name.clone(),
            version: self.version.clone(),
            source_sha: source_sha.into(),
            descriptor_sha256: self.sha256(),
            package: self.clone(),
            build_units,
        }
    }
}

fn validate_definition(name: &str, definition: &WorkerDefinition, source_dir: &Path) -> Result<()> {
    let non_empty = |field: &str, values: &[String]| {
        if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
            Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: format!("{field} must contain non-blank values"),
            })
        } else {
            Ok(())
        }
    };
    if definition.registry.description.trim().is_empty() {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: "registry.description must not be blank".into(),
        });
    }
    if definition.registry.license.trim().is_empty() {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: "registry.license must not be blank".into(),
        });
    }
    if let Some(key) = definition
        .runtime
        .environment
        .keys()
        .find(|key| key.starts_with("III_"))
    {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!("runtime.environment cannot override reserved key {key}"),
        });
    }
    if let Some(image) = definition.runtime.base_image.as_deref()
        && !digest_pinned_image(image)
    {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: "runtime.base_image must be pinned by @sha256:<64 lowercase hex>".into(),
        });
    }
    match &definition.artifact {
        Artifact::RustBinary { binary, targets } => {
            if binary.trim().is_empty() {
                return Err(ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: "artifact.binary must not be blank".into(),
                });
            }
            non_empty("artifact.targets", targets)?;
        }
        Artifact::JavascriptBundle {
            build_command,
            include,
        } => {
            non_empty("artifact.build_command", build_command)?;
            non_empty("artifact.include", include)?;
            validate_bundle_paths(name, include)?;
            validate_bundle_files(name, source_dir, include, true)?;
        }
        Artifact::PythonBundle {
            build_command,
            include,
        } => {
            non_empty("artifact.build_command", build_command)?;
            non_empty("artifact.include", include)?;
            validate_bundle_paths(name, include)?;
            validate_bundle_files(name, source_dir, include, false)?;
        }
        Artifact::OciImage {
            context,
            dockerfile,
            platforms,
        } => {
            if context.trim().is_empty() || dockerfile.trim().is_empty() {
                return Err(ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: "OCI context and dockerfile must not be blank".into(),
                });
            }
            non_empty("artifact.platforms", platforms)?;
            for required in ["linux/amd64", "linux/arm64"] {
                if !platforms.iter().any(|platform| platform == required) {
                    return Err(ComposeError::InvalidPackageDescriptor {
                        worker: name.into(),
                        message: format!("artifact.platforms must include {required}"),
                    });
                }
            }
        }
    }
    if !matches!(definition.artifact, Artifact::OciImage { .. })
        && definition.runtime.exec.as_ref().is_none_or(Vec::is_empty)
    {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: "runtime.exec is required for non-OCI workers".into(),
        });
    }
    Ok(())
}

fn compile_registry(
    name: &str,
    authored: RegistryDefinition,
    source_dir: &Path,
) -> Result<RegistryMetadata> {
    let config = match authored.config {
        None => None,
        Some(config) => {
            let authored_path = Path::new(&config.defaults_file);
            if config.defaults_file.trim().is_empty()
                || authored_path.is_absolute()
                || authored_path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
            {
                return Err(ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: "registry.config.defaults_file must be a relative path without '..'"
                        .into(),
                });
            }
            let path = source_dir
                .join(authored_path)
                .canonicalize()
                .map_err(|error| ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: format!("cannot resolve registry config defaults: {error}"),
                })?;
            if !path.starts_with(source_dir) || !path.is_file() {
                return Err(ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: "registry config defaults must be a regular file within source.path"
                        .into(),
                });
            }
            let text = std::fs::read_to_string(&path).map_err(|error| {
                ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: format!("cannot read registry config defaults: {error}"),
                }
            })?;
            let value: serde_json::Value =
                if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                    serde_json::from_str(&text).map_err(|error| error.to_string())
                } else {
                    serde_yaml::from_str::<serde_yaml::Value>(&text)
                        .map_err(|error| error.to_string())
                        .and_then(|value| {
                            serde_json::to_value(value).map_err(|error| error.to_string())
                        })
                }
                .map_err(|error| ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: format!("registry config defaults are invalid: {error}"),
                })?;
            let serde_json::Value::Object(defaults) = value else {
                return Err(ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: "registry config defaults must be a JSON object".into(),
                });
            };
            validate_config_defaults(name, &serde_json::Value::Object(defaults.clone()), "")?;
            Some(RegistryConfig { defaults })
        }
    };
    Ok(RegistryMetadata {
        description: authored.description,
        license: authored.license,
        tags: authored.tags,
        dependencies: authored.dependencies,
        config,
        publish: authored.publish,
    })
}

fn validate_config_defaults(name: &str, value: &serde_json::Value, prefix: &str) -> Result<()> {
    if let serde_json::Value::Array(values) = value {
        for (index, nested) in values.iter().enumerate() {
            validate_config_defaults(name, nested, &format!("{prefix}[{index}]"))?;
        }
        return Ok(());
    }
    let serde_json::Value::Object(values) = value else {
        return Ok(());
    };
    for (key, nested) in values {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if key.starts_with("III_") {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: format!("registry config defaults cannot set reserved key {path}"),
            });
        }
        let upper = key.to_ascii_uppercase();
        let secret_like = [
            "TOKEN",
            "SECRET",
            "PASSWORD",
            "API_KEY",
            "CREDENTIAL",
            "CREDENTIALS",
        ]
        .iter()
        .any(|word| upper == *word || upper.ends_with(&format!("_{word}")));
        if secret_like && nested.as_str().is_none_or(|value| !value.is_empty()) {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: format!("secret-like registry config default {path} must be empty"),
            });
        }
        validate_config_defaults(name, nested, &path)?;
    }
    Ok(())
}

fn validate_source_paths(name: &str, source: &PackageSource) -> Result<()> {
    for (field, value) in [
        ("source.path", &source.path),
        ("source.package_manifest", &source.package_manifest),
    ] {
        let path = Path::new(value);
        if value.trim().is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: format!("{field} must be a relative path without '..'"),
            });
        }
    }
    Ok(())
}

fn digest_pinned_image(image: &str) -> bool {
    let Some((_, digest)) = image.rsplit_once("@sha256:") else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn safe_bundle_include(value: &str) -> bool {
    let path = Path::new(value);
    if value.trim().is_empty()
        || value.contains(['*', '?', '[', ']'])
        || path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return false;
    }
    !path
        .components()
        .filter_map(|part| match part {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .any(|part| {
            matches!(
                part,
                "node_modules" | "test" | "tests" | "doc" | "docs" | "cache" | ".cache"
            )
        })
}

fn validate_bundle_paths(name: &str, include: &[String]) -> Result<()> {
    if let Some(path) = include.iter().find(|path| !safe_bundle_include(path)) {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!(
                "artifact.include path '{path}' is unsafe, a glob, or includes excluded build/test content"
            ),
        });
    }
    Ok(())
}

fn validate_bundle_files(
    name: &str,
    source_dir: &Path,
    include: &[String],
    allow_missing_build_output: bool,
) -> Result<()> {
    for authored in include {
        let candidate = source_dir.join(authored);
        match candidate.canonicalize() {
            Ok(resolved) if resolved.starts_with(source_dir) && resolved.is_file() => {}
            Ok(_) => {
                return Err(ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: format!(
                        "artifact.include '{authored}' must be a regular file within source.path"
                    ),
                });
            }
            Err(error)
                if allow_missing_build_output && error.kind() == std::io::ErrorKind::NotFound =>
            {
                let parent = canonical_existing_parent(&candidate).ok_or_else(|| {
                    ComposeError::InvalidPackageDescriptor {
                        worker: name.into(),
                        message: format!(
                            "artifact.include '{authored}' has no resolvable parent within source.path"
                        ),
                    }
                })?;
                if !parent.starts_with(source_dir) {
                    return Err(ComposeError::InvalidPackageDescriptor {
                        worker: name.into(),
                        message: format!("artifact.include '{authored}' escapes source.path"),
                    });
                }
            }
            Err(error) => {
                return Err(ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: format!(
                        "artifact.include file '{authored}' cannot be resolved: {error}"
                    ),
                });
            }
        }
    }
    Ok(())
}

fn canonical_existing_parent(path: &Path) -> Option<PathBuf> {
    let mut parent = path.parent()?;
    loop {
        match parent.canonicalize() {
            Ok(resolved) => return Some(resolved),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                parent = parent.parent()?;
            }
            Err(_) => return None,
        }
    }
}

fn read_package_version(path: &Path) -> std::result::Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let version = if name == "package.json" {
        serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| value.get("version")?.as_str().map(str::to_owned))
    } else {
        toml::from_str::<toml::Value>(&text).ok().and_then(|value| {
            value
                .get("package")
                .and_then(|package| package.get("version"))
                .or_else(|| {
                    value
                        .get("project")
                        .and_then(|project| project.get("version"))
                })
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        })
    };
    version
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{} does not declare a package version", path.display()))
}

fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            format!(
                "{{{}}}",
                entries
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical_json(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        serde_json::Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        other => serde_json::to_string(other).unwrap(),
    }
}

fn resolve(base: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.into()
    } else {
        base.join(path)
    }
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'_' | b'-'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_digest_ignores_mapping_insertion_order() {
        let a = serde_json::json!({"b": 2, "a": {"z": 1, "c": 3}});
        let b = serde_json::json!({"a": {"c": 3, "z": 1}, "b": 2});
        assert_eq!(canonical_json(&a), canonical_json(&b));
    }
}
