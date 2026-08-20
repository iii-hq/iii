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

    #[error("container '{container}': 'pre_run_timeout' requires 'pre_run'")]
    PreRunTimeoutWithoutPreRun { container: String },

    #[error("container '{container}': invalid duration '{value}'. Use 30s, 500ms or 2m")]
    InvalidDuration { container: String, value: String },

    #[error("container '{container}': package workers require an explicit 'version'")]
    MissingVersionForPackage { container: String },

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

    /// The registry answered with a name or version compose will not put in a
    /// path. Everything installed lands in a directory built from these, so a
    /// value carrying a separator or `..` writes outside the cache.
    #[error(
        "container '{container}': the registry at {registry} answered with an unusable {field} \
         '{value}'. A package {field} may hold only letters, digits, '.', '_' and '-'"
    )]
    RegistryNameRefused {
        container: String,
        registry: String,
        field: String,
        value: String,
    },

    #[error(
        "container '{container}': '{name}' is a {kind} worker. compose can install binary and \
         bundle workers; engine workers are built into the engine and image workers need the OCI \
         runtime"
    )]
    UnsupportedPackageKind {
        container: String,
        name: String,
        kind: String,
    },

    /// `compose::add` would turn a declaration into a different kind of thing.
    /// A registry package and a local directory are not two versions of one
    /// worker, and replacing one with the other loses whatever the operator
    /// wrote.
    #[error(
        "container '{container}' is already declared as a {from} worker, and '{name}' resolves to \
         a {to} one. Remove the entry first, or add the new worker under another name"
    )]
    WorkerSourceChanged {
        container: String,
        name: String,
        from: String,
        to: String,
    },

    /// `iii compose up` could not bring its project up. The container that
    /// failed has already reported itself; this is the command saying so with
    /// an exit code.
    #[error("{path} did not start")]
    ProjectDidNotStart { path: std::path::PathBuf },

    /// `${VAR}` in a compose file, with nothing to put there.
    #[error(
        "{path}: ${{{name}}} is not set in this environment. Export it, or write \
         ${{{name}:-<default>}} to give it one"
    )]
    UndefinedVariable {
        path: std::path::PathBuf,
        name: String,
    },

    #[error("{path}: '{reference}' opens a ${{...}} reference that is never closed")]
    UnterminatedReference {
        path: std::path::PathBuf,
        reference: String,
    },

    #[error(
        "{path}: '${{{name}}}' is not a variable name. A name holds letters, digits and \
         '_', and does not start with a digit. To write it through untouched, double the \
         sign: $${{{name}}}"
    )]
    InvalidReference {
        path: std::path::PathBuf,
        name: String,
    },

    /// `compose::update` was given a container it cannot move.
    #[error(
        "container '{container}' is a {kind} worker, and only a registry package carries a \
         version to update. Edit the file, or use compose::add to declare a package under \
         another name"
    )]
    NotAPackageContainer { container: String, kind: String },

    /// `worker=` did not name anything installable.
    #[error("cannot read worker '{spec}': {reason}")]
    InvalidWorkerSpec { spec: String, reason: String },

    /// A bundle on a platform with no VM to put it in.
    ///
    /// Separate from [`Self::UnsupportedPackageKind`] because the answer is
    /// about the machine, not the worker: the same compose file works on linux
    /// and macOS, and saying "compose cannot install bundle workers" would send
    /// an operator looking for a fault in their project.
    #[error(
        "container '{container}': '{name}' is a bundle worker, and bundles run in a VM, which \
         windows has no support for. Run compose under WSL, where the VM has KVM to run on, or \
         ask the worker's publisher for a binary build"
    )]
    BundleNeedsAVm { container: String, name: String },

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

    /// The resolved configuration could not be written back. Fatal for the
    /// same reason the fetch is: the container would boot on whatever was
    /// stored before, which is the silent downgrade this path exists to stop.
    #[error("configuration '{name}' could not be published: {message}")]
    ConfigPublishFailed { name: String, message: String },

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

    /// The child died on its way up. The exit code alone names nothing an
    /// operator can act on — a worker that refused its configuration and one
    /// that could not bind a port both exit non-zero — so what it printed
    /// comes with it.
    #[error(
        "container '{container}' exited with {code} before it registered{}",
        match .tail {
            Some(tail) => format!(". It last said:\n{tail}"),
            None => String::new(),
        }
    )]
    ChildExitedBeforeReady {
        container: String,
        code: i32,
        /// Last lines of the container's own log. Never env values: the log
        /// holds what the child chose to print, and compose adds nothing.
        tail: Option<String>,
    },

    #[error("container '{container}' could not start: {message}")]
    SpawnFailed { container: String, message: String },

    /// A container's hook refused to let it start. The hook's own code is
    /// carried through: "your pre_run exited 3" and "the process would not
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
            Self::PreRunTimeoutWithoutPreRun { .. } => "PRE_RUN_TIMEOUT_WITHOUT_PRE_RUN",
            Self::InvalidDuration { .. } => "INVALID_DURATION",
            Self::MissingVersionForPackage { .. } => "MISSING_VERSION_FOR_PACKAGE",
            Self::MissingStartCommand { .. } => "MISSING_START_COMMAND",
            Self::InvalidManifest { .. } => "INVALID_MANIFEST",
            Self::MissingWorkerDirectory { .. } => "MISSING_WORKER_DIRECTORY",
            Self::MissingEnvFile { .. } => "MISSING_ENV_FILE",
            Self::RegistryUnreachable { .. } => "REGISTRY_UNREACHABLE",
            Self::PackageNotResolved { .. } => "PACKAGE_NOT_RESOLVED",
            Self::RegistryNameRefused { .. } => "REGISTRY_NAME_REFUSED",
            Self::UnsupportedPackageKind { .. } => "UNSUPPORTED_PACKAGE_KIND",
            Self::BundleNeedsAVm { .. } => "BUNDLE_NEEDS_A_VM",
            Self::InvalidWorkerSpec { .. } => "INVALID_WORKER_SPEC",
            Self::UndefinedVariable { .. } => "UNDEFINED_VARIABLE",
            Self::UnterminatedReference { .. } => "UNTERMINATED_REFERENCE",
            Self::InvalidReference { .. } => "INVALID_REFERENCE",
            Self::ProjectDidNotStart { .. } => "PROJECT_DID_NOT_START",
            Self::WorkerSourceChanged { .. } => "WORKER_SOURCE_CHANGED",
            Self::NotAPackageContainer { .. } => "NOT_A_PACKAGE_CONTAINER",
            Self::UnsupportedPlatform { .. } => "UNSUPPORTED_PLATFORM",
            Self::PackageNotInstalled { .. } => "PACKAGE_NOT_INSTALLED",
            Self::PackageDownloadFailed { .. } => "PACKAGE_DOWNLOAD_FAILED",
            Self::PackageDigestMismatch { .. } => "PACKAGE_DIGEST_MISMATCH",
            Self::PackageArtifactEmpty { .. } => "PACKAGE_ARTIFACT_EMPTY",
            Self::ReservedEnvOverride { .. } => "RESERVED_ENV_OVERRIDE",
            Self::ConfigFetchFailed { .. } => "CONFIG_FETCH_FAILED",
            Self::ConfigPublishFailed { .. } => "CONFIG_PUBLISH_FAILED",
            Self::EngineCallFailed { .. } => "ENGINE_CALL_FAILED",
            Self::FunctionsInWrongNamespace { .. } => "FUNCTIONS_IN_WRONG_NAMESPACE",
            Self::ReadinessTimeout { .. } => "STARTUP_TIMEOUT",
            Self::WorkerIgnoredNamespace { .. } => "WORKER_IGNORED_NAMESPACE",
            Self::WorkerNameMismatch { .. } => "WORKER_NAME_MISMATCH",
            Self::ChildExitedBeforeReady { .. } => "CHILD_EXITED_BEFORE_REGISTRATION",
            Self::SpawnFailed { .. } => "SPAWN_FAILED",
            Self::HookFailed { hook_code, .. } => hook_code,
            Self::UnknownContainer { .. } => "UNKNOWN_CONTAINER",
            Self::ContainerNameTaken { .. } => "CONTAINER_NAME_TAKEN",
            Self::InvalidState { .. } => "INVALID_STATE_FILE",
            Self::UnknownProject { .. } => "UNKNOWN_PROJECT",
            Self::RelativeFileMissing { .. } => "COMPOSE_FILE_UNREADABLE",
            Self::DaemonAlreadyServing { .. } => "DAEMON_ALREADY_SERVING",
            Self::InvalidNamespace { .. } => "INVALID_NAMESPACE",
            Self::StateDirUnavailable => "STATE_DIR_UNAVAILABLE",
            Self::NoComposeFileHere { .. } => "NO_COMPOSE_FILE",
        }
    }
}

pub type Result<T> = std::result::Result<T, ComposeError>;
