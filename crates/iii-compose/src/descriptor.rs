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
        toolchain: FrontendTool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        frontends: Vec<FrontendBuild>,
    },
    JavascriptBundle {
        workspace_root: String,
        runtime: FrontendTool,
        package_manager: FrontendTool,
        lockfile: String,
        install_command: Vec<String>,
        build_command: Vec<String>,
        include: Vec<String>,
    },
    PythonBundle {
        workspace_root: String,
        runtime: FrontendTool,
        package_manager: FrontendTool,
        lockfile: String,
        install_command: Vec<String>,
        build_command: Vec<String>,
        include: Vec<String>,
    },
    OciImage {
        context: String,
        dockerfile: String,
        platforms: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrontendBuild {
    /// Package-manager workspace relative to the Compose root. Install runs here.
    pub workspace_root: String,
    /// Directory relative to the Compose root. It may be shared by multiple workers.
    /// Build runs here.
    pub source_path: String,
    pub runtime: FrontendTool,
    pub package_manager: FrontendTool,
    /// Lockfile relative to `workspace_root`, included in cache identity.
    pub lockfile: String,
    /// Exact dependency-install argv executed inside `workspace_root`.
    pub install_command: Vec<String>,
    /// Exact build argv executed inside `source_path`.
    pub build_command: Vec<String>,
    /// Explicit generated paths relative to `source_path`; no discovery or globs.
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrontendTool {
    pub name: String,
    pub version: String,
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
        let version = read_package_version(&manifest, &root).map_err(|message| {
            ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message,
            }
        })?;
        semver::Version::parse(&version).map_err(|error| {
            ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: format!("package version must be valid semver: {error}"),
            }
        })?;
        validate_definition(name, &definition, &source_dir, &root)?;
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
        hex::encode(Sha256::digest(canonical_json(self).as_bytes()))
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

fn validate_definition(
    name: &str,
    definition: &WorkerDefinition,
    source_dir: &Path,
    compose_root: &Path,
) -> Result<()> {
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
        Artifact::RustBinary {
            binary,
            targets,
            toolchain,
            frontends,
        } => {
            if binary.trim().is_empty() {
                return Err(ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: "artifact.binary must not be blank".into(),
                });
            }
            non_empty("artifact.targets", targets)?;
            validate_tool("artifact.toolchain", toolchain, name)?;
            for (index, frontend) in frontends.iter().enumerate() {
                validate_frontend_build(name, compose_root, index, frontend)?;
            }
        }
        Artifact::JavascriptBundle {
            workspace_root,
            runtime,
            package_manager,
            lockfile,
            install_command,
            build_command,
            include,
        } => {
            validate_bundle_toolchain(
                name,
                compose_root,
                workspace_root,
                runtime,
                package_manager,
                lockfile,
                install_command,
                definition.registry.publish,
            )?;
            non_empty("artifact.build_command", build_command)?;
            non_empty("artifact.include", include)?;
            validate_bundle_paths(name, include)?;
            validate_bundle_files(name, source_dir, include, true)?;
            if definition.registry.publish {
                require_bundle_base_image(name, &definition.runtime)?;
            }
        }
        Artifact::PythonBundle {
            workspace_root,
            runtime,
            package_manager,
            lockfile,
            install_command,
            build_command,
            include,
        } => {
            validate_bundle_toolchain(
                name,
                compose_root,
                workspace_root,
                runtime,
                package_manager,
                lockfile,
                install_command,
                definition.registry.publish,
            )?;
            non_empty("artifact.build_command", build_command)?;
            non_empty("artifact.include", include)?;
            validate_bundle_paths(name, include)?;
            // Python releases may materialize an explicit, deterministic
            // vendor/archive output during the build just like JavaScript
            // emits its bundle. The post-build packager still requires every
            // listed path to be a regular file before publication.
            validate_bundle_files(name, source_dir, include, true)?;
            if definition.registry.publish {
                require_bundle_base_image(name, &definition.runtime)?;
            }
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

fn require_bundle_base_image(name: &str, runtime: &Runtime) -> Result<()> {
    if runtime.base_image.is_none() {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: "bundle workers require a digest-pinned runtime.base_image".into(),
        });
    }
    Ok(())
}

fn validate_tool(field: &str, tool: &FrontendTool, name: &str) -> Result<()> {
    if tool.name.trim().is_empty() || tool.version.trim().is_empty() {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!("{field} requires non-blank name and version"),
        });
    }
    Ok(())
}

fn validate_bundle_toolchain(
    name: &str,
    compose_root: &Path,
    workspace_root: &str,
    runtime: &FrontendTool,
    package_manager: &FrontendTool,
    lockfile: &str,
    install_command: &[String],
    require_lockfile: bool,
) -> Result<()> {
    if !safe_relative_path(workspace_root) || !safe_relative_path(lockfile) {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: "bundle workspace_root and lockfile must be safe relative paths".into(),
        });
    }
    validate_tool("artifact.runtime", runtime, name)?;
    validate_tool("artifact.package_manager", package_manager, name)?;
    if install_command.is_empty() || install_command.iter().any(|arg| arg.trim().is_empty()) {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: "artifact.install_command must contain non-blank argv".into(),
        });
    }
    let workspace = compose_root
        .join(workspace_root)
        .canonicalize()
        .map_err(|error| ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!("artifact.workspace_root cannot be resolved: {error}"),
        })?;
    if !workspace.starts_with(compose_root) || !workspace.is_dir() {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: "artifact.workspace_root must be a directory within the Compose root".into(),
        });
    }
    let authored_lockfile = workspace.join(lockfile);
    let lockfile = match authored_lockfile.canonicalize() {
        Ok(path) => path,
        Err(error) if !require_lockfile && error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: format!("artifact.lockfile cannot be resolved: {error}"),
            });
        }
    };
    if !lockfile.starts_with(&workspace) || !lockfile.is_file() {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: "artifact.lockfile must be a regular file within workspace_root".into(),
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
    if !safe_relative_path(value) {
        return false;
    }
    let path = Path::new(value);
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

fn safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.trim().is_empty()
        && !value.contains(['*', '?', '[', ']'])
        && !path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

fn validate_frontend_build(
    name: &str,
    compose_root: &Path,
    index: usize,
    frontend: &FrontendBuild,
) -> Result<()> {
    if !safe_relative_path(&frontend.workspace_root)
        || !safe_relative_path(&frontend.source_path)
        || !safe_relative_path(&frontend.lockfile)
    {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!(
                "artifact.frontends[{index}].workspace_root, source_path and lockfile must be safe relative paths"
            ),
        });
    }
    if frontend.runtime.name.trim().is_empty()
        || frontend.runtime.version.trim().is_empty()
        || frontend.package_manager.name.trim().is_empty()
        || frontend.package_manager.version.trim().is_empty()
    {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!(
                "artifact.frontends[{index}] runtime and package_manager require name and version"
            ),
        });
    }
    for (field, command) in [
        ("install_command", &frontend.install_command),
        ("build_command", &frontend.build_command),
    ] {
        if command.is_empty() || command.iter().any(|value| value.trim().is_empty()) {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: format!("artifact.frontends[{index}].{field} must contain non-blank argv"),
            });
        }
    }
    if frontend.outputs.is_empty()
        || frontend
            .outputs
            .iter()
            .any(|path| !safe_bundle_include(path))
    {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!(
                "artifact.frontends[{index}].outputs must contain explicit safe relative paths"
            ),
        });
    }
    let frontend_dir = compose_root
        .join(&frontend.source_path)
        .canonicalize()
        .map_err(|error| ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!("artifact.frontends[{index}].source_path cannot be resolved: {error}"),
        })?;
    if !frontend_dir.starts_with(compose_root) || !frontend_dir.is_dir() {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!(
                "artifact.frontends[{index}].source_path must be a directory within the Compose root"
            ),
        });
    }
    let workspace_dir = compose_root
        .join(&frontend.workspace_root)
        .canonicalize()
        .map_err(|error| ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!(
                "artifact.frontends[{index}].workspace_root cannot be resolved: {error}"
            ),
        })?;
    if !workspace_dir.starts_with(compose_root) || !workspace_dir.is_dir() {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!(
                "artifact.frontends[{index}].workspace_root must be a directory within the Compose root"
            ),
        });
    }
    let lockfile = workspace_dir
        .join(&frontend.lockfile)
        .canonicalize()
        .map_err(|error| ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!("artifact.frontends[{index}].lockfile cannot be resolved: {error}"),
        })?;
    if !lockfile.starts_with(&workspace_dir) || !lockfile.is_file() {
        return Err(ComposeError::InvalidPackageDescriptor {
            worker: name.into(),
            message: format!(
                "artifact.frontends[{index}].lockfile must be a regular file within workspace_root"
            ),
        });
    }
    for output in &frontend.outputs {
        let candidate = frontend_dir.join(output);
        let existing = match candidate.canonicalize() {
            Ok(existing) => existing,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                canonical_existing_parent(&candidate).ok_or_else(|| {
                    ComposeError::InvalidPackageDescriptor {
                        worker: name.into(),
                        message: format!(
                            "artifact.frontends[{index}].output '{output}' has no resolvable parent"
                        ),
                    }
                })?
            }
            Err(error) => {
                return Err(ComposeError::InvalidPackageDescriptor {
                    worker: name.into(),
                    message: format!(
                        "artifact.frontends[{index}].output '{output}' cannot be resolved: {error}"
                    ),
                });
            }
        };
        if !existing.starts_with(&frontend_dir) {
            return Err(ComposeError::InvalidPackageDescriptor {
                worker: name.into(),
                message: format!(
                    "artifact.frontends[{index}].output '{output}' escapes its frontend path"
                ),
            });
        }
    }
    Ok(())
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

fn read_package_version(path: &Path, compose_root: &Path) -> std::result::Result<String, String> {
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
        let parsed = toml::from_str::<toml::Value>(&text).ok();
        let direct = parsed.as_ref().and_then(|value| {
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
        });
        let inherits_workspace = parsed.as_ref().is_some_and(|value| {
            value
                .get("package")
                .and_then(|package| package.get("version"))
                .and_then(toml::Value::as_table)
                .and_then(|version| version.get("workspace"))
                .and_then(toml::Value::as_bool)
                == Some(true)
        });
        direct.or_else(|| {
            inherits_workspace
                .then(|| workspace_package_version(path, compose_root))
                .flatten()
        })
    };
    version
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{} does not declare a package version", path.display()))
}

fn workspace_package_version(manifest: &Path, compose_root: &Path) -> Option<String> {
    let mut directory = manifest.parent()?;
    loop {
        if !directory.starts_with(compose_root) {
            return None;
        }
        let workspace_manifest = directory.join("Cargo.toml");
        if workspace_manifest != manifest
            && let Ok(text) = std::fs::read_to_string(&workspace_manifest)
            && let Ok(value) = toml::from_str::<toml::Value>(&text)
            && let Some(version) = value
                .get("workspace")
                .and_then(|workspace| workspace.get("package"))
                .and_then(|package| package.get("version"))
                .and_then(toml::Value::as_str)
        {
            return Some(version.to_owned());
        }
        if directory == compose_root {
            return None;
        }
        directory = directory.parent()?;
    }
}

fn canonical_json<T: Serialize + ?Sized>(value: &T) -> String {
    serde_jcs::to_string(value).expect("validated package descriptor is RFC 8785 JSON")
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

    #[test]
    fn registry_jcs_fixture_matches_cross_language_digest() {
        let descriptor: PackageDescriptor = serde_json::from_str(include_str!(
            "../tests/fixtures/package-descriptor-jcs.json"
        ))
        .unwrap();

        let canonical = canonical_json(&descriptor);
        assert!(canonical.contains("\"cpu\":2,"));
        assert!(canonical.contains("\"fractional\":0.125"));
        assert!(canonical.find("😀").unwrap() < canonical.find('�').unwrap());
        assert_eq!(
            descriptor.sha256(),
            "46e9a0abc6dcded74e3cf39382fd6c830aae32c04664adbe86f98b95e7fdad08"
        );
    }

    #[test]
    fn registry_schema_parity_digest_is_stable() {
        let descriptor = PackageDescriptor {
            name: "iii-compose".into(),
            version: "0.1.0".into(),
            source: PackageSource {
                path: "crates/iii-compose".into(),
                package_manifest: "Cargo.toml".into(),
            },
            artifact: Artifact::RustBinary {
                binary: "iii-compose".into(),
                targets: vec!["x86_64-unknown-linux-gnu".into()],
                toolchain: FrontendTool {
                    name: "rust".into(),
                    version: "1.90.0".into(),
                },
                frontends: Vec::new(),
            },
            runtime: Runtime {
                exec: Some(vec!["./iii-compose".into()]),
                ..Runtime::default()
            },
            registry: RegistryMetadata {
                description: "Compose compiler fixture".into(),
                license: "Elastic-2.0".into(),
                tags: Vec::new(),
                dependencies: BTreeMap::new(),
                config: None,
                publish: false,
            },
            validation: Validation {
                interface: ValidationMode::Skipped,
            },
        };
        assert_eq!(
            descriptor.sha256(),
            "9bbce3a7ece8338149eaed4681d2e6952d3a2196f513f2bf96664a7402600509"
        );
    }
}
