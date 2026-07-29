// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Structured compose errors.
//!
//! Every variant carries a stable machine code (`code()`), because compose
//! operations are also reachable remotely as `compose::*` and the operator
//! tooling matches on the code, not on the message text.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ComposeError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not valid compose YAML: {message}")]
    Yaml { path: PathBuf, message: String },

    #[error("containers must declare at least one worker")]
    EmptyContainers,

    #[error("container '{container}' depends on '{dependency}', which is not declared")]
    UnknownDependency {
        container: String,
        dependency: String,
    },

    #[error("container '{container}' depends on itself")]
    SelfDependency { container: String },

    #[error("dependency cycle: {path}")]
    DependencyCycle { path: String },

    #[error(
        "container '{container}': unsupported worker source '{source_uri}'. v1 supports \
         path:// and package://"
    )]
    UnsupportedWorkerSource {
        container: String,
        source_uri: String,
    },

    #[error("container '{container}': 'run' is only valid for path:// workers")]
    RunNotAllowedForPackage { container: String },

    #[error("container '{container}': 'pre_start_timeout' requires 'pre_start'")]
    PreStartTimeoutWithoutPreStart { container: String },

    #[error("container '{container}': invalid duration '{value}'. Use 30s, 500ms or 2m")]
    InvalidDuration { container: String, value: String },

    #[error("container '{container}': package workers require an explicit 'version'")]
    MissingVersionForPackage { container: String },

    #[error(
        "container '{container}': config_uri '{uri}' is not supported in v1. Use \
         worker://configuration/get/<name> or config_name"
    )]
    UnsupportedConfigUri { container: String, uri: String },

    #[error("container '{container}': set either 'config_name' or 'config_uri', not both")]
    ConflictingConfigSource { container: String },

    #[error(
        "container '{container}': no start command. Add 'run:' to the compose entry or \
         'scripts.start' to {manifest}"
    )]
    MissingStartCommand {
        container: String,
        manifest: PathBuf,
    },

    #[error("{path} is not a valid iii.worker.yaml: {message}")]
    InvalidManifest { path: PathBuf, message: String },

    #[error(
        "container '{container}': manifest at {path} declares name '{manifest_name}'; it must \
         match the compose container key"
    )]
    ManifestNameMismatch {
        container: String,
        path: PathBuf,
        manifest_name: String,
    },

    #[error("container '{container}': worker directory {path} does not exist")]
    MissingWorkerDirectory { container: String, path: PathBuf },

    #[error(
        "container '{container}': package:// resolution is not implemented yet. Point the \
         entry at a local path:// worker"
    )]
    PackageResolutionUnimplemented { container: String },

    #[error("`iii compose` requires {flag}")]
    MissingFlag { flag: &'static str },

    #[error(
        "daemon supervision is not implemented yet. `iii compose validate --file <PATH>` \
         validates a project offline"
    )]
    DaemonNotImplemented,
}

impl ComposeError {
    /// Stable machine-readable code. Operator tooling and `compose::*` callers
    /// match on this; the human message may be reworded freely.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "COMPOSE_FILE_UNREADABLE",
            Self::Yaml { .. } => "INVALID_COMPOSE_FILE",
            Self::EmptyContainers => "EMPTY_CONTAINERS",
            Self::UnknownDependency { .. } => "UNKNOWN_DEPENDENCY",
            Self::SelfDependency { .. } => "SELF_DEPENDENCY",
            Self::DependencyCycle { .. } => "DEPENDENCY_CYCLE",
            Self::UnsupportedWorkerSource { .. } => "UNSUPPORTED_WORKER_SOURCE",
            Self::RunNotAllowedForPackage { .. } => "RUN_NOT_ALLOWED_FOR_PACKAGE",
            Self::PreStartTimeoutWithoutPreStart { .. } => "PRE_START_TIMEOUT_WITHOUT_PRE_START",
            Self::InvalidDuration { .. } => "INVALID_DURATION",
            Self::MissingVersionForPackage { .. } => "MISSING_VERSION_FOR_PACKAGE",
            Self::UnsupportedConfigUri { .. } => "UNSUPPORTED_CONFIG_URI",
            Self::ConflictingConfigSource { .. } => "CONFLICTING_CONFIG_SOURCE",
            Self::MissingStartCommand { .. } => "MISSING_START_COMMAND",
            Self::InvalidManifest { .. } => "INVALID_MANIFEST",
            Self::ManifestNameMismatch { .. } => "MANIFEST_NAME_MISMATCH",
            Self::MissingWorkerDirectory { .. } => "MISSING_WORKER_DIRECTORY",
            Self::PackageResolutionUnimplemented { .. } => "PACKAGE_RESOLUTION_UNIMPLEMENTED",
            Self::MissingFlag { .. } => "MISSING_FLAG",
            Self::DaemonNotImplemented => "DAEMON_NOT_IMPLEMENTED",
        }
    }
}

pub type Result<T> = std::result::Result<T, ComposeError>;
