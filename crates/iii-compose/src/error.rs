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

    #[error("container '{container}': env_file {path} does not exist")]
    MissingEnvFile { container: String, path: PathBuf },

    #[error("container '{container}': cannot reach the registry at {registry}: {message}")]
    RegistryUnreachable {
        container: String,
        registry: String,
        message: String,
    },

    #[error("container '{container}': no version of '{name}' satisfies '{range}'. {message}")]
    PackageNotResolved {
        container: String,
        name: String,
        range: String,
        message: String,
    },

    #[error(
        "container '{container}': '{name}' is a {kind} worker. compose can install binary \
         workers; engine workers are built into the engine and image workers need the OCI \
         runtime"
    )]
    UnsupportedPackageKind {
        container: String,
        name: String,
        kind: String,
    },

    #[error(
        "container '{container}': {name} {version} has no build for {target}. It ships: \
         {available}"
    )]
    UnsupportedPlatform {
        container: String,
        name: String,
        version: String,
        target: String,
        available: String,
    },

    #[error("container '{container}': its package has not been installed yet")]
    PackageNotInstalled { container: String },

    #[error("container '{container}': could not download {url}: {message}")]
    PackageDownloadFailed {
        container: String,
        url: String,
        message: String,
    },

    /// The bytes are not what the registry promised. Not a retry: a different
    /// artefact than the one that was resolved.
    #[error(
        "container '{container}': {url} does not match its digest. Expected {expected}, got \
         {actual}"
    )]
    PackageDigestMismatch {
        container: String,
        url: String,
        expected: String,
        actual: String,
    },

    #[error("container '{container}': the archive for '{name}' held no executable ({path})")]
    PackageArtifactEmpty {
        container: String,
        name: String,
        path: PathBuf,
    },

    #[error(
        "container '{container}': '{name}' is reserved for the daemon and cannot be set by \
         environment or env_file"
    )]
    ReservedEnvOverride { container: String, name: String },

    #[error("configuration '{name}' could not be resolved: {message}")]
    ConfigFetchFailed { name: String, message: String },

    #[error("engine call {function} failed: {message}")]
    EngineCallFailed { function: String, message: String },

    #[error(
        "container '{container}' registered in '{expected}', but its function '{function}' \
         landed in '{found_in}': the function registrations reached the engine before the \
         namespace did, so they were filed without it"
    )]
    FunctionsInWrongNamespace {
        container: String,
        function: String,
        found_in: String,
        expected: String,
    },

    #[error("compose daemon '{id}' did not detach: {message}")]
    DetachFailed { id: String, message: String },

    #[error("container '{container}' was not ready after {seconds}s")]
    ReadinessTimeout { container: String, seconds: u64 },

    #[error(
        "container '{container}' never registered, but worker '{registered_as}' appeared in \
         '{namespace}' while it was starting: a worker that names itself ignores \
         III_WORKER_NAME, so the container has to carry that same name"
    )]
    WorkerNameMismatch {
        container: String,
        registered_as: String,
        namespace: String,
    },

    #[error(
        "container '{container}' connected to the engine but registered in namespace \
         '{found_in}' instead of '{expected}': it ignores III_NAMESPACE, so it was built \
         against an SDK that predates namespace routing"
    )]
    WorkerIgnoredNamespace {
        container: String,
        found_in: String,
        expected: String,
    },

    #[error("container '{container}' exited with {code} before it registered")]
    ChildExitedBeforeReady { container: String, code: i32 },

    #[error("container '{container}' could not start: {message}")]
    SpawnFailed { container: String, message: String },

    /// A container's hook refused to let it start. The hook's own code is
    /// carried through: "your pre_start exited 3" and "the process would not
    /// spawn" are different problems and must not share one code.
    #[error("container '{container}': {message}")]
    HookFailed {
        container: String,
        hook_code: &'static str,
        message: String,
    },

    #[error("no container named '{container}' in this project")]
    UnknownContainer { container: String },

    /// Something already answers to this container's name in this namespace.
    /// Starting anyway would hand readiness a stranger and leave our own
    /// process rejected by the engine's `(namespace, worker_name)` lease.
    #[error(
        "a worker named '{container}' is already registered in namespace '{namespace}'. \
         Another daemon owns this project, or a worker from an earlier run is still \
         connected. `iii trigger engine::workers::list` names it"
    )]
    ContainerNameTaken {
        container: String,
        namespace: String,
    },

    #[error("{path} is not valid daemon state: {message}")]
    InvalidState { path: PathBuf, message: String },

    #[error(
        "project '{project}' already holds state for {recorded}; it cannot be rebound to \
         {requested}. Use a different id="
    )]
    StateBindingMismatch {
        /// The project id from the call, not the daemon's namespace: what is
        /// already bound to a compose file is the project.
        project: String,
        recorded: PathBuf,
        requested: PathBuf,
    },

    #[error(
        "no project '{id}' here. Name the compose file on the first call: \
         iii trigger compose::up id={id} file=./worker-compose.yaml"
    )]
    UnknownProject { id: String },

    /// A relative `file=` that missed. The path is resolved by the daemon, in
    /// the directory the daemon was started in — which is rarely the directory
    /// the caller is standing in, and never obvious from the caller's side.
    #[error(
        "no compose file at '{}', resolved from the daemon's directory {}. \
         Pass an absolute path, or start the daemon where the file is.",
        .path.display(), .cwd.display()
    )]
    RelativeFileMissing { path: PathBuf, cwd: PathBuf },

    /// The engine already has a compose daemon.
    ///
    /// Named rather than forwarded as a registration failure: an engine holds
    /// one daemon by design, and the operator's next move is to use the one
    /// already there rather than to debug a rejection.
    #[error(
        "another compose daemon already serves {engine_url}. \
         Use it (iii trigger compose::list) or stop it (iii trigger compose::stop).\n  \
         the engine said: {detail}"
    )]
    DaemonAlreadyServing { engine_url: String, detail: String },

    #[error("{flags} cannot be used together")]
    ConflictingFlags { flags: &'static str },

    /// The id is the daemon's namespace *and* its state directory, so it is
    /// checked at parse time: the alternative is a daemon that starts, answers
    /// nothing an operator can address, and fails at its first write.
    #[error("'{namespace}' cannot be a namespace: {reason}")]
    InvalidNamespace {
        namespace: String,
        reason: &'static str,
    },

    #[error("cannot locate a home directory for the daemon state")]
    StateDirUnavailable,

    /// `--attach` with nothing to attach to, or too much. Never guessed:
    /// following the wrong daemon is a wrong answer that looks like a right
    /// one, and the namespaces are uuids nobody recalls.
    #[error(
        "{}",
        match .candidates {
            Some(found) => format!(
                "several daemons have run here. Name one: iii compose --attach --ns <NS>\n  \
                 {found}"
            ),
            None => "no detached daemon has run here. Start one with `iii compose -d`".to_string(),
        }
    )]
    NoDaemonToAttach { candidates: Option<String> },

    /// A project-scoped call that named no file, from a daemon whose own
    /// directory holds none either. The file *is* the project, so there is
    /// nothing to fall back to.
    #[error(
        "no {expected} here, and the call named no file. Pass file=<PATH>, or start the daemon \
         in a project directory"
    )]
    NoComposeFileHere { expected: &'static str },
}

impl ComposeError {
    /// Stable machine-readable code. Operator tooling and `compose::*` callers
    /// match on this; the human message may be reworded freely.
    pub fn code(&self) -> &'static str {
        match self {
            // Any read or write compose attempted. `RelativeFileMissing` is
            // the one that really is about a compose file.
            Self::Io { .. } => "IO_ERROR",
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
            Self::MissingEnvFile { .. } => "MISSING_ENV_FILE",
            Self::RegistryUnreachable { .. } => "REGISTRY_UNREACHABLE",
            Self::PackageNotResolved { .. } => "PACKAGE_NOT_RESOLVED",
            Self::UnsupportedPackageKind { .. } => "UNSUPPORTED_PACKAGE_KIND",
            Self::UnsupportedPlatform { .. } => "UNSUPPORTED_PLATFORM",
            Self::PackageNotInstalled { .. } => "PACKAGE_NOT_INSTALLED",
            Self::PackageDownloadFailed { .. } => "PACKAGE_DOWNLOAD_FAILED",
            Self::PackageDigestMismatch { .. } => "PACKAGE_DIGEST_MISMATCH",
            Self::PackageArtifactEmpty { .. } => "PACKAGE_ARTIFACT_EMPTY",
            Self::ReservedEnvOverride { .. } => "RESERVED_ENV_OVERRIDE",
            Self::ConfigFetchFailed { .. } => "CONFIG_FETCH_FAILED",
            Self::EngineCallFailed { .. } => "ENGINE_CALL_FAILED",
            Self::FunctionsInWrongNamespace { .. } => "FUNCTIONS_IN_WRONG_NAMESPACE",
            Self::DetachFailed { .. } => "DETACH_FAILED",
            Self::ReadinessTimeout { .. } => "STARTUP_TIMEOUT",
            Self::WorkerIgnoredNamespace { .. } => "WORKER_IGNORED_NAMESPACE",
            Self::WorkerNameMismatch { .. } => "WORKER_NAME_MISMATCH",
            Self::ChildExitedBeforeReady { .. } => "CHILD_EXITED_BEFORE_REGISTRATION",
            Self::SpawnFailed { .. } => "SPAWN_FAILED",
            Self::HookFailed { hook_code, .. } => hook_code,
            Self::UnknownContainer { .. } => "UNKNOWN_CONTAINER",
            Self::ContainerNameTaken { .. } => "CONTAINER_NAME_TAKEN",
            Self::InvalidState { .. } => "INVALID_STATE_FILE",
            Self::StateBindingMismatch { .. } => "STATE_BINDING_MISMATCH",
            Self::UnknownProject { .. } => "UNKNOWN_PROJECT",
            Self::RelativeFileMissing { .. } => "COMPOSE_FILE_UNREADABLE",
            Self::DaemonAlreadyServing { .. } => "DAEMON_ALREADY_SERVING",
            Self::ConflictingFlags { .. } => "CONFLICTING_FLAGS",
            Self::InvalidNamespace { .. } => "INVALID_NAMESPACE",
            Self::StateDirUnavailable => "STATE_DIR_UNAVAILABLE",
            Self::NoDaemonToAttach { .. } => "NO_DAEMON_TO_ATTACH",
            Self::NoComposeFileHere { .. } => "NO_COMPOSE_FILE",
        }
    }
}

pub type Result<T> = std::result::Result<T, ComposeError>;
