// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `scripts.build` (spec 4.1): whether a container builds before it runs (its first build, or a
//! package change, spec 5.2 and 5.3), the warnings of a build that is out of date (5.5), and one
//! run of the command, for a start or for `compose::build` (7.4).

use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    sync::watch,
};

use crate::build_record as record;
use crate::{
    build_record::{BuildRecord, BuildSource, Change, SCHEMA},
    config::Container,
    error::{ComposeError, Result},
    logs::{LogSink, LogStream},
    manifest::StartSpec,
    sandbox::is_sandbox_build,
    shutdown::ShutdownSignal,
    spawn::{SpawnCtx, spawn_plan},
};

/// How many of the build's last output lines a failure carries (spec 4.1).
const TAIL_LINES: usize = 20;
/// What every build line starts with in the container's logs.
const PREFIX: &str = "[build] ";
/// What a VM build gets beyond `build_timeout`, so `iii sandbox build`, which enforces it, reports
/// its own `TIMED_OUT` first.
const VM_TIMEOUT_SLACK: Duration = Duration::from_secs(30);
/// How long the output of a build that ended may take to drain: a process it left behind can hold
/// the pipes open.
const DRAIN: Duration = Duration::from_secs(2);
/// The grace of a build that is stopped.
const STOP_GRACE: Duration = Duration::from_secs(10);
/// How long [`stop`] waits for a stopped build to end.
const STOP_WAIT: Duration = Duration::from_secs(30);

/// The builds running in this process, by build directory: what stops one, and what says it
/// ended. A container's VM cannot be destroyed while its build runs (S509), so `remove` stops it
/// first, through [`stop`].
type Running = BTreeMap<PathBuf, (watch::Sender<bool>, watch::Receiver<bool>)>;

fn running() -> &'static Mutex<Running> {
    static RUNNING: OnceLock<Mutex<Running>> = OnceLock::new();
    RUNNING.get_or_init(Mutex::default)
}

/// A build's entry in [`running`], dropped with the build: it says the build ended.
struct Registered {
    dir: PathBuf,
    ended: watch::Sender<bool>,
}

impl Registered {
    /// Registers the build of `dir`; the receiver turns `true` when [`stop`] asks it to stop.
    fn new(dir: &Path) -> (Self, watch::Receiver<bool>) {
        let (stop, stopped) = watch::channel(false);
        let (ended, ends) = watch::channel(false);
        running()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(dir.to_path_buf(), (stop, ends));
        (
            Self {
                dir: dir.to_path_buf(),
                ended,
            },
            stopped,
        )
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        let mut running = running()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if running
            .get(&self.dir)
            .is_some_and(|(_, ends)| ends.same_channel(&self.ended.subscribe()))
        {
            running.remove(&self.dir);
        }
        self.ended.send_replace(true);
    }
}

/// What this process knows of one `.build/<container>`.
#[derive(Default)]
struct Entry {
    /// Who decides on and runs its build now: see [`Slot`].
    slot: Arc<tokio::sync::Mutex<()>>,
    /// Removes of the container under way ([`Stopping`]): a taker of the slot builds nothing.
    stopping: usize,
    /// Rebuilt by an add or update, and not yet run: its container still runs its previous
    /// build (spec 5.3). Cleared when the container next starts.
    unrestarted: bool,
}

/// The [`Entry`] of every build directory this process met.
fn entries() -> &'static Mutex<BTreeMap<PathBuf, Entry>> {
    static ENTRIES: OnceLock<Mutex<BTreeMap<PathBuf, Entry>>> = OnceLock::new();
    ENTRIES.get_or_init(Mutex::default)
}

fn with_entry<T>(dir: &Path, f: impl FnOnce(&mut Entry) -> T) -> T {
    f(entries()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(dir.to_path_buf())
        .or_default())
}

/// The right to decide whether container `key` builds, and to run that build. A start, a rebuild
/// and `compose::build` each hold it from [`due`] to the end of the build, so one `.build/<c>`
/// never has two builds at once: the second would write over the first, or meet S509.
pub(crate) struct Slot {
    dir: PathBuf,
    _held: tokio::sync::OwnedMutexGuard<()>,
}

impl Slot {
    /// Whether the container is being removed: its taker then builds nothing, and gives up with
    /// [`Outcome::stopped`].
    pub(crate) fn stopping(&self) -> bool {
        with_entry(&self.dir, |entry| entry.stopping > 0)
    }

    /// The container starts on the build this slot leaves: a rebuild it had not run is now its
    /// own.
    pub(crate) fn starting(&self) {
        with_entry(&self.dir, |entry| entry.unrestarted = false);
    }
}

/// Takes the build slot of container `key`, waiting while a start, rebuild or build holds it.
/// Callers race the wait with their cancel: dropping it gives nothing up.
pub(crate) async fn slot(compose_file: &Path, key: &str) -> Slot {
    let dir = build_dir(compose_file, key);
    let lock = with_entry(&dir, |entry| Arc::clone(&entry.slot));
    Slot {
        dir,
        _held: lock.lock_owned().await,
    }
}

/// Records that an add or update rebuilt container `key` while it runs its previous build: a
/// retried add or update restarts it ([`rebuilt_unrestarted`]) until it starts again.
pub(crate) fn mark_rebuilt(compose_file: &Path, key: &str) {
    with_entry(&build_dir(compose_file, key), |entry| {
        entry.unrestarted = true
    });
}

/// Whether an add or update rebuilt container `key`, and it has not started since. A build made
/// by `compose::build` or `iii sandbox build` is never this: its restart is the user's.
pub(crate) fn rebuilt_unrestarted(compose_file: &Path, key: &str) -> bool {
    with_entry(&build_dir(compose_file, key), |entry| entry.unrestarted)
}

/// A remove of one container under way, from [`stop`]: while it lives, whoever takes the
/// container's build slot builds nothing. Dropped when the remove ends.
#[must_use = "the container counts as being removed only while this lives"]
pub(crate) struct Stopping {
    dir: PathBuf,
}

impl Drop for Stopping {
    fn drop(&mut self) {
        with_entry(&self.dir, |entry| {
            entry.stopping = entry.stopping.saturating_sub(1);
        });
    }
}

/// Marks container `key` as being removed, then stops the build of it running in this process, if
/// one is, and waits for it to end: its process stops, so `iii sandbox build` closes its channel
/// and the adapter aborts the build (spec 6.2). The build fails with `BUILD_STOPPED`, and so does
/// any start, rebuild or build that takes the slot while the mark lives: one waiting for it, or
/// one that had it but had not started its build yet.
pub(crate) async fn stop(compose_file: &Path, key: &str) -> Stopping {
    let dir = build_dir(compose_file, key);
    with_entry(&dir, |entry| entry.stopping += 1);
    let running = running()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&dir)
        .cloned();
    if let Some((stop, mut ends)) = running {
        stop.send_replace(true);
        let _ = tokio::time::timeout(STOP_WAIT, ends.wait_for(|ended| *ended)).await;
    }
    Stopping { dir }
}

/// The compose file's directory, as `III_COMPOSE_DIR` names it.
fn compose_dir(compose_file: &Path) -> PathBuf {
    let file = crate::spawn::canonical_compose_file(compose_file);
    file.parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// `.build/<container>` beside the compose file.
pub(crate) fn build_dir(compose_file: &Path, key: &str) -> PathBuf {
    crate::build_record::build_dir(&compose_dir(compose_file), key)
}

/// What container `key` is built from: its `path://` directory, or its `.workers/<container>/`.
pub(crate) fn worker_dir(compose_file: &Path, key: &str, container: &Container) -> PathBuf {
    container.worker_dir().map_or_else(
        || crate::build_record::package_dir(&compose_dir(compose_file), key),
        Path::to_path_buf,
    )
}

/// Why a container builds before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reason {
    /// It has no `build.json` yet (spec 5.2).
    First,
    /// Its package is not the one `build.json` was made from (spec 5.3).
    PackageChanged { old: String, new: String },
}

impl Reason {
    /// The line logged before the build (spec 10).
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::First => "no build yet; running scripts.build".to_string(),
            Self::PackageChanged { old, new } => {
                format!("package changed ({old} -> {new}); rebuilding")
            }
        }
    }
}

fn describe_source(source: &BuildSource) -> String {
    match source {
        BuildSource::Package { name, version, .. } => format!("{name}@{version}"),
        BuildSource::Path { dir } => dir.clone(),
    }
}

/// Spec 5.2, 5.3 and 5.5 before container `key` runs: `Some` when it builds first, because it has
/// a non-empty `scripts.build` and no `build.json` yet, or its package changed. Logs the other
/// differences, once per start, and lets it run on its current build. The check only informs: one
/// that fails is logged and the container runs.
pub(crate) async fn due(compose_file: &Path, key: &str, container: &Container) -> Option<Reason> {
    assess(compose_file, key, container, true).await
}

/// Whether running container `key`'s build was made from another package than the one locked
/// now: a rebuild for a new package left undone, which a retried add or update finishes. Logs
/// nothing.
pub(crate) async fn package_changed(compose_file: &Path, key: &str, container: &Container) -> bool {
    matches!(
        assess(compose_file, key, container, false).await,
        Some(Reason::PackageChanged { .. })
    )
}

/// [`due`], saying what it finds only when `log` is set.
async fn assess(
    compose_file: &Path,
    key: &str,
    container: &Container,
    log: bool,
) -> Option<Reason> {
    container.scripts.build_command()?;
    let dir = build_dir(compose_file, key);
    if !dir.join("build.json").is_file() {
        return Some(Reason::First);
    }
    let (file, owned_key, worker) = (
        crate::spawn::canonical_compose_file(compose_file),
        key.to_string(),
        worker_dir(compose_file, key, container),
    );
    // Blocking: the dependency files are hashed, and a lock file can be large.
    let checked = tokio::task::spawn_blocking(move || -> Result<Option<Vec<Change>>> {
        let Some(record) = record::read(&dir)? else {
            return Ok(None);
        };
        record::changes(&record, &file, &owned_key, &worker).map(Some)
    })
    .await;
    let changes = match checked {
        Ok(Ok(Some(changes))) => changes,
        Ok(Ok(None)) => return Some(Reason::First),
        Ok(Err(_)) | Err(_) if !log => return None,
        Ok(Err(error)) => {
            stale_check_failed(key, &error.to_string());
            return None;
        }
        // A panic is a bug, not a build that cannot be read: said apart, so it is not taken for
        // one.
        Err(error) => {
            crate::report::daemon_line(
                &format!(
                    "{key}: the check whether its build is up to date panicked (a bug): {error}"
                ),
                true,
            );
            return None;
        }
    };
    if let Some(Change::Source { old, new }) = changes
        .iter()
        .find(|change| matches!(change, Change::Source { .. }))
    {
        return Some(Reason::PackageChanged {
            old: describe_source(old),
            new: describe_source(new),
        });
    }
    for change in changes {
        let line = match change {
            Change::Dependencies(files) => format!(
                "{key}: dependencies changed since the last build ({}); run `iii sandbox build {key}`",
                files.join(", ")
            ),
            Change::BaseImage { old, new } => format!(
                "{key}: base image changed since the last build ({} -> {}); run `iii sandbox build \
                 {key}`",
                old.as_deref().unwrap_or("the adapter's default"),
                new.as_deref().unwrap_or("the adapter's default")
            ),
            Change::Source { .. } => continue,
        };
        if log {
            crate::report::daemon_line(&line, true);
        }
    }
    None
}

fn stale_check_failed(key: &str, error: &str) {
    crate::report::daemon_line(
        &format!("{key}: could not check whether its build is up to date: {error}"),
        true,
    );
}

/// How one run of `scripts.build` ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Exited 0; `build_id` is the image's, for a VM build whose `build.json` names one.
    Built { build_id: Option<String> },
    /// The `code` and `message` of spec 4.1.
    Failed { code: String, message: String },
    /// Stopped by `cancel`.
    Cancelled,
}

impl Outcome {
    /// The start failure of container `key`: `BUILD_FAILED` with the build's own code.
    /// A build stopped, or given up, because its container is being removed.
    pub(crate) fn stopped() -> Self {
        Self::Failed {
            code: "BUILD_STOPPED".to_string(),
            message: "stopped: the container is being removed".to_string(),
        }
    }

    pub(crate) fn into_error(self, key: &str) -> Option<ComposeError> {
        match self {
            Self::Failed { code, message } => Some(ComposeError::BuildFailed {
                container: key.to_string(),
                code,
                message,
            }),
            Self::Built { .. } | Self::Cancelled => None,
        }
    }
}

/// The end of a build process.
enum End {
    Exited(ExitStatus),
    TimedOut,
    Cancelled,
    /// By [`stop`].
    Stopped,
}

/// Runs `container`'s `scripts.build` once (spec 4.1), as `sh -c` with what `spawn` gives its
/// `scripts.run`: environment and working directory. Its output goes into the container's logs,
/// each line prefixed with `[build] `. `scripts.build_timeout` bounds it, outside
/// `startup_timeout`, and `cancel` stops it. A host build records `build.json` once it exits 0;
/// `iii sandbox build` records its own.
pub(crate) async fn run(
    spawn: &SpawnCtx<'_>,
    container: &Container,
    logs: &LogSink,
    cancel: Option<&ShutdownSignal>,
) -> Outcome {
    let Some(command) = container.scripts.build_command() else {
        return Outcome::Built { build_id: None };
    };
    let vm = is_sandbox_build(command);
    let key = spawn.container_key;
    let dir = build_dir(spawn.compose_file, key);
    // Registered before anything runs, then the remove mark read: [`stop`] marks first and then
    // looks for the build, so it either finds this one or marked the container before this read.
    let (_registered, mut stopped) = Registered::new(&dir);
    if *stopped.borrow() || with_entry(&dir, |entry| entry.stopping > 0) {
        return Outcome::stopped();
    }
    if let Err(error) = prepare_build_dir(&dir) {
        return Outcome::Failed {
            code: error.code().to_string(),
            message: error.to_string(),
        };
    }
    let start = StartSpec::Shell(command.to_string());
    let plan = spawn_plan(&SpawnCtx {
        start: &start,
        ..spawn.clone()
    });
    let Some(process) = plan.command() else {
        return Outcome::Failed {
            code: "SPAWN_FAILED".to_string(),
            message: "scripts.build has no program to run".to_string(),
        };
    };
    let (child, output) = match crate::process::spawn_supervised_piped(process) {
        Ok(spawned) => spawned,
        Err(error) => {
            return Outcome::Failed {
                code: "SPAWN_FAILED".to_string(),
                message: error.to_string(),
            };
        }
    };
    let tail = Arc::new(Mutex::new(Tail::default()));
    let mut readers = Vec::new();
    if let Some(stdout) = output.stdout {
        readers.push(tokio::spawn(capture(
            stdout,
            LogStream::Stdout,
            logs.clone(),
            tail.clone(),
        )));
    }
    if let Some(stderr) = output.stderr {
        readers.push(tokio::spawn(capture(
            stderr,
            LogStream::Stderr,
            logs.clone(),
            tail.clone(),
        )));
    }

    let budget =
        container.scripts.build_timeout + if vm { VM_TIMEOUT_SLACK } else { Duration::ZERO };
    let waited = async {
        tokio::select! {
            waited = tokio::time::timeout(budget, child.wait()) => match waited {
                Ok(status) => End::Exited(status),
                Err(_) => End::TimedOut,
            },
            _ = stopped.wait_for(|stopped| *stopped) => End::Stopped,
        }
    };
    let end = match cancel {
        Some(signal) => signal.run(waited).await.unwrap_or(End::Cancelled),
        None => waited.await,
    };
    if !matches!(end, End::Exited(_)) {
        // Stopping `iii sandbox build` closes its channel, which aborts the build (spec 6.2).
        child.stop(STOP_GRACE).await;
    }
    for reader in readers {
        let _ = tokio::time::timeout(DRAIN, reader).await;
    }
    let tail = std::mem::take(
        &mut *tail
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let lines: Vec<String> = tail.lines.into_iter().collect();

    match end {
        End::Cancelled => Outcome::Cancelled,
        End::Stopped => Outcome::stopped(),
        End::TimedOut => Outcome::Failed {
            code: "TIMED_OUT".to_string(),
            message: lines.join("\n"),
        },
        End::Exited(status) if status.success() => {
            if vm {
                return Outcome::Built {
                    build_id: recorded_build_id(dir).await,
                };
            }
            let worker = worker_dir(spawn.compose_file, key, container);
            match record_host_build(spawn.compose_file, key, command, &worker, &dir).await {
                Ok(()) => Outcome::Built { build_id: None },
                Err(error) => Outcome::Failed {
                    code: error.code().to_string(),
                    message: format!("the build ran, but build.json could not be written: {error}"),
                },
            }
        }
        // The command names its failure on its last line (spec 8.1), whichever stream it went to:
        // stderr's is asked first, as the other stream's reader may still have queued output.
        End::Exited(status) => match [tail.last_stderr, tail.last_stdout]
            .iter()
            .flatten()
            .find_map(|line| cli_failure(line))
            .filter(|_| vm)
        {
            Some((code, message)) => Outcome::Failed { code, message },
            None => Outcome::Failed {
                code: format!("EXIT_{}", exit_number(status)),
                message: lines.join("\n"),
            },
        },
    }
}

/// `.build/<container>/`, and `.build/.gitignore` with `*`, so the project's own `.gitignore`
/// needs no edit (spec 3.2).
fn prepare_build_dir(dir: &Path) -> Result<()> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ComposeError::Io { path, source }
    };
    std::fs::create_dir_all(dir).map_err(io(dir))?;
    if let Some(root) = dir.parent() {
        let ignore = root.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n").map_err(io(&ignore))?;
        }
    }
    Ok(())
}

/// The exit code a failure is named after: the code, or 128 plus the signal that ended it.
fn exit_number(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status
            .code()
            .or_else(|| status.signal().map(|signal| 128 + signal))
            .unwrap_or(-1)
    }
    #[cfg(not(unix))]
    {
        status.code().unwrap_or(-1)
    }
}

/// The last line `iii sandbox build` prints on failure (spec 8.1):
/// `iii sandbox: build failed: <code>: <message>`.
fn cli_failure(line: &str) -> Option<(String, String)> {
    let (code, message) = line
        .trim()
        .strip_prefix("iii sandbox: build failed: ")?
        .split_once(": ")?;
    (!code.is_empty()).then(|| (code.to_string(), message.to_string()))
}

/// What a build's output leaves for its failure: its last [`TAIL_LINES`], and the last line of each
/// stream.
#[derive(Default)]
struct Tail {
    lines: VecDeque<String>,
    last_stdout: Option<String>,
    last_stderr: Option<String>,
}

/// Writes each line of `reader` into `logs` with [`PREFIX`], keeping it in `tail`.
async fn capture(
    reader: impl AsyncRead + Unpin,
    stream: LogStream,
    logs: LogSink,
    tail: Arc<Mutex<Tail>>,
) {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buffer)
            .trim_end_matches(['\n', '\r'])
            .to_string();
        logs.write_line(stream, &format!("{PREFIX}{line}")).await;
        let mut tail = tail
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if tail.lines.len() == TAIL_LINES {
            tail.lines.pop_front();
        }
        tail.lines.push_back(line.clone());
        match stream {
            LogStream::Stdout => tail.last_stdout = Some(line),
            LogStream::Stderr => tail.last_stderr = Some(line),
        }
    }
}

/// The `build_id` the `build.json` of a VM build names, read without blocking the runtime.
async fn recorded_build_id(dir: PathBuf) -> Option<String> {
    tokio::task::spawn_blocking(move || record::read(&dir))
        .await
        .ok()?
        .ok()?
        .and_then(|record| record.image)
        .map(|image| image.build_id)
}

/// The `build.json` of a host build (spec 5.4): the same keys, without `image`.
async fn record_host_build(
    compose_file: &Path,
    key: &str,
    command: &str,
    worker_dir: &Path,
    dir: &Path,
) -> Result<()> {
    let (file, key, command, worker_dir, dir) = (
        crate::spawn::canonical_compose_file(compose_file),
        key.to_string(),
        command.to_string(),
        worker_dir.to_path_buf(),
        dir.to_path_buf(),
    );
    let at = dir.join("build.json");
    tokio::task::spawn_blocking(move || {
        let record = BuildRecord {
            schema: SCHEMA,
            source: record::source(&file, &key)?,
            container: key,
            command,
            dependency_files: record::dependency_files(&worker_dir)?,
            image: None,
            built_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        record::write(&dir, &record)
    })
    .await
    .map_err(|error| ComposeError::Io {
        path: at,
        source: std::io::Error::other(error.to_string()),
    })?
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Instant};

    use super::*;
    use crate::{
        build_record::{BuildAdapter, BuildBaseImage, BuildContract, BuildImage},
        config::ComposeFile,
        logs::{LogEntry, LogStore},
    };

    struct Project {
        dir: tempfile::TempDir,
        file: ComposeFile,
        logs: LogStore,
    }

    /// `api`, a `path://` worker declared with `fields`.
    fn project(fields: &str) -> Project {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("api")).unwrap();
        std::fs::write(
            dir.path().join("api/iii.worker.yaml"),
            "scripts: {start: run}\n",
        )
        .unwrap();
        let path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &path,
            format!("containers:\n  api: {{worker: path://./api, {fields}}}\n"),
        )
        .unwrap();
        let file = ComposeFile::load(&path).unwrap();
        let logs = LogStore::open(dir.path().join("logs")).unwrap();
        Project { dir, file, logs }
    }

    impl Project {
        async fn build(&self, path: Option<&Path>, cancel: Option<&ShutdownSignal>) -> Outcome {
            let container = &self.file.containers["api"];
            let start = StartSpec::Shell(String::new());
            let mut user_env = BTreeMap::new();
            if let Some(path) = path {
                user_env.insert(
                    "PATH".to_string(),
                    format!("{}:/usr/bin:/bin", path.display()),
                );
            }
            let working_dir = container.worker_dir().unwrap().to_path_buf();
            let spawn = SpawnCtx {
                engine_url: "ws://127.0.0.1:1",
                namespace: "shop",
                compose_namespace: "compose",
                compose_file: &self.file.path,
                container_key: "api",
                start: &start,
                config_name: None,
                working_dir: &working_dir,
                user_env: &user_env,
            };
            run(&spawn, container, &self.logs.sink("api"), cancel).await
        }

        async fn due(&self) -> Option<Reason> {
            due(&self.file.path, "api", &self.file.containers["api"]).await
        }

        fn record(&self) -> Option<BuildRecord> {
            record::read(&build_dir(&self.file.path, "api")).unwrap()
        }

        async fn logs(&self) -> Vec<LogEntry> {
            // The writer is a task: let it catch up.
            tokio::time::sleep(Duration::from_millis(200)).await;
            crate::logs::tests::entries(&self.logs, "api", 50).await
        }
    }

    /// Spec 5.2 and 4.1: no `build.json` yet, so it builds; the output lands in the container's
    /// logs as `[build] `; a host build records `build.json` without an image.
    #[tokio::test]
    async fn a_first_build_runs_into_the_logs_and_records_a_host_build() {
        let project = project("scripts: {build: 'echo built; echo warned >&2', run: x}");
        assert_eq!(project.due().await, Some(Reason::First));

        assert_eq!(
            project.build(None, None).await,
            Outcome::Built { build_id: None }
        );

        let record = project.record().unwrap();
        assert_eq!(record.command, "echo built; echo warned >&2");
        assert_eq!(
            record.source,
            BuildSource::Path {
                dir: "./api".into()
            }
        );
        assert_eq!(record.image, None);
        assert_eq!(project.due().await, None);
        let logs = project.logs().await;
        assert!(logs.contains(&LogEntry {
            stream: LogStream::Stdout,
            message: "[build] built".into()
        }));
        assert!(logs.contains(&LogEntry {
            stream: LogStream::Stderr,
            message: "[build] warned".into()
        }));
        assert_eq!(
            std::fs::read_to_string(project.dir.path().join(".build/.gitignore")).unwrap(),
            "*\n"
        );
    }

    /// `scripts.build: ""` declares there is nothing to build: nothing runs, no `build.json`.
    #[tokio::test]
    async fn nothing_to_build_runs_nothing() {
        let project = project("scripts: {build: '', run: x}");
        assert_eq!(project.due().await, None);
        assert_eq!(
            project.build(None, None).await,
            Outcome::Built { build_id: None }
        );
        assert!(!project.dir.path().join(".build").exists());
    }

    /// Spec 4.1: another command fails as `EXIT_<status>` with its last 20 lines, and the start
    /// as `BUILD_FAILED` carrying them.
    #[tokio::test]
    async fn a_failed_build_carries_its_code_and_its_last_lines() {
        let project = project(
            "scripts: {build: 'for i in $(seq 1 25); do echo line $i; done; exit 3', run: x}",
        );

        let outcome = project.build(None, None).await;

        let expected = (6..=25)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            outcome,
            Outcome::Failed {
                code: "EXIT_3".into(),
                message: expected.clone()
            }
        );
        assert_eq!(project.record(), None);
        let error = outcome.into_error("api").unwrap();
        assert_eq!(error.code(), "BUILD_FAILED");
        let reported = crate::lifecycle::OpError::from(&error);
        let cause = reported.cause.unwrap();
        assert_eq!((cause.code.as_str(), cause.message), ("EXIT_3", expected));
    }

    /// `iii sandbox build` names its failure on its last line (spec 8.1), and Compose records
    /// nothing: the command writes its own `build.json`.
    // Writes an executable shell script, which needs Unix permissions.
    #[cfg(unix)]
    #[tokio::test]
    async fn iii_sandbox_build_names_its_own_failure() {
        let project = project("scripts: {build: iii sandbox build, run: iii sandbox run}");
        let bin = project.dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(
            bin.join("iii"),
            "#!/bin/sh\necho \"$III_BUILD_DIR\"\necho 'iii sandbox: build failed: S507: setup exited with 1'\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(
            bin.join("iii"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        assert_eq!(
            project.build(Some(&bin), None).await,
            Outcome::Failed {
                code: "S507".into(),
                message: "setup exited with 1".into()
            }
        );
        assert_eq!(project.record(), None);
    }

    /// `scripts.build_timeout`, `compose::cancel` and a removal each end a build at once.
    #[tokio::test]
    async fn a_build_ends_at_its_timeout_its_cancel_or_its_stop() {
        let project = project("scripts: {build: 'sleep 30', build_timeout: 1s, run: x}");
        let began = Instant::now();
        let Outcome::Failed { code, .. } = project.build(None, None).await else {
            panic!("the build ran past its timeout");
        };
        assert_eq!(code, "TIMED_OUT");
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "{:?}",
            began.elapsed()
        );

        let project = self::project("scripts: {build: 'sleep 30', run: x}");
        let controller = crate::shutdown::ShutdownController::default();
        let signal = controller.signal();
        let began = Instant::now();
        let (outcome, ()) = tokio::join!(project.build(None, Some(&signal)), async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            controller.cancel();
        });
        assert_eq!(outcome, Outcome::Cancelled);
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "{:?}",
            began.elapsed()
        );

        let began = Instant::now();
        let (outcome, stopped) = tokio::join!(project.build(None, None), async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            stop(&project.file.path, "api").await
        });
        drop(stopped);
        assert!(
            matches!(outcome, Outcome::Failed { ref code, .. } if code == "BUILD_STOPPED"),
            "{outcome:?}"
        );
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "{:?}",
            began.elapsed()
        );
        assert!(
            !running()
                .lock()
                .unwrap()
                .contains_key(&build_dir(&project.file.path, "api")),
            "a stopped build stayed registered"
        );
    }

    /// A remove that marked the container before its build registered still stops that build,
    /// which then runs nothing.
    #[tokio::test]
    async fn a_remove_marked_before_the_build_registers_stops_it_unrun() {
        let project = project("scripts: {build: 'touch built', run: x}");
        let stopping = stop(&project.file.path, "api").await;
        let outcome = project.build(None, None).await;
        drop(stopping);
        assert!(
            matches!(outcome, Outcome::Failed { ref code, .. } if code == "BUILD_STOPPED"),
            "{outcome:?}"
        );
        assert!(!project.dir.path().join("api/built").exists());
        assert!(
            !running()
                .lock()
                .unwrap()
                .contains_key(&build_dir(&project.file.path, "api")),
            "a build that never ran stayed registered"
        );
    }

    /// Spec 5.3 and 5.5: a new source rebuilds; changed dependency files or base image only
    /// warn, and the container runs on its build.
    #[tokio::test]
    async fn only_a_new_source_rebuilds() {
        let project = project("scripts: {build: 'true', run: x}");
        project.build(None, None).await;
        let dir = build_dir(&project.file.path, "api");
        let built = project.record().unwrap();

        let mut moved = built.clone();
        moved.source = BuildSource::Path {
            dir: "./old".into(),
        };
        record::write(&dir, &moved).unwrap();
        assert_eq!(
            project.due().await,
            Some(Reason::PackageChanged {
                old: "./old".into(),
                new: "./api".into()
            })
        );
        assert_eq!(
            Reason::PackageChanged {
                old: "a@1.0.0".into(),
                new: "a@1.1.0".into()
            }
            .describe(),
            "package changed (a@1.0.0 -> a@1.1.0); rebuilding"
        );

        record::write(&dir, &built).unwrap();
        std::fs::write(project.dir.path().join("api/package.json"), "{}").unwrap();
        assert_eq!(project.due().await, None);

        let mut imaged = built.clone();
        imaged.image = Some(BuildImage {
            file: "image.erofs".into(),
            build_id: "sha256:1".into(),
            base_image: BuildBaseImage {
                declared: Some("docker.io/iiidev/python".into()),
                reference: "docker.io/iiidev/python:latest".into(),
                digest: None,
            },
            adapter: BuildAdapter {
                name: "sandbox-libkrun".into(),
                version: "0.4.0".into(),
            },
            contract: BuildContract { major: 1, minor: 3 },
        });
        record::write(&dir, &imaged).unwrap();
        assert_eq!(project.due().await, None);
    }

    /// The failure line on stderr names the failure even when stdout's output comes after it.
    // Writes an executable shell script, which needs Unix permissions.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_failure_line_on_stderr_wins_over_later_stdout() {
        let project = project("scripts: {build: iii sandbox build, run: iii sandbox run}");
        let bin = project.dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(
            bin.join("iii"),
            "#!/bin/sh\necho 'iii sandbox: build failed: S507: setup exited with 1' >&2\nseq 1 200\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(
            bin.join("iii"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        assert_eq!(
            project.build(Some(&bin), None).await,
            Outcome::Failed {
                code: "S507".into(),
                message: "setup exited with 1".into()
            }
        );
    }

    /// Spec 5.5 and 10: each kind of change is one line, worded as the spec says, and the
    /// container runs on its build.
    #[tokio::test]
    async fn an_out_of_date_build_warns_once_per_kind_and_runs() {
        let project = project("scripts: {build: 'true', run: x}");
        project.build(None, None).await;
        let mut record = project.record().unwrap();
        record.image = Some(BuildImage {
            file: "image.erofs".into(),
            build_id: "sha256:1".into(),
            base_image: BuildBaseImage {
                declared: Some("docker.io/iiidev/python".into()),
                reference: "docker.io/iiidev/python:latest".into(),
                digest: None,
            },
            adapter: BuildAdapter {
                name: "sandbox-libkrun".into(),
                version: "0.4.0".into(),
            },
            contract: BuildContract { major: 1, minor: 3 },
        });
        record::write(&build_dir(&project.file.path, "api"), &record).unwrap();
        std::fs::write(project.dir.path().join("api/package.json"), "{}").unwrap();

        let (due, lines) = crate::report::capture_daemon_lines(project.due()).await;

        assert_eq!(due, None);
        assert_eq!(
            lines,
            [
                "api: dependencies changed since the last build (package.json); run `iii sandbox build api`",
                "api: base image changed since the last build (docker.io/iiidev/python -> the adapter's default); run `iii sandbox build api`",
            ]
        );
    }

    /// One build at a time per `.build/<container>`: a second taker waits for the first, the
    /// wait gives way to a cancel, and another container's slot is free meanwhile.
    #[tokio::test]
    async fn one_build_slot_per_directory_and_its_wait_gives_way_to_a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("worker-compose.yaml");
        std::fs::write(&file, "containers: {}\n").unwrap();
        let held = slot(&file, "api").await;
        let other = tokio::time::timeout(Duration::from_secs(1), slot(&file, "web"))
            .await
            .expect("another directory's slot waited");

        let controller = crate::shutdown::ShutdownController::default();
        let signal = controller.signal();
        let cancelled = tokio::spawn({
            let file = file.clone();
            async move { signal.run(slot(&file, "api")).await.is_none() }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!cancelled.is_finished(), "a second build took a held slot");
        controller.cancel();
        assert!(cancelled.await.unwrap(), "the wait ignored the cancel");

        let next = tokio::spawn({
            let file = file.clone();
            async move {
                let _ = slot(&file, "api").await;
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!next.is_finished());
        drop(held);
        tokio::time::timeout(Duration::from_secs(1), next)
            .await
            .expect("the slot was not handed on")
            .unwrap();
        drop(other);
    }

    /// A remove marks the slot: whoever takes it while the remove lasts, having waited for it or
    /// not, gives up. The mark ends with the last remove.
    #[tokio::test]
    async fn a_remove_under_way_turns_away_whoever_takes_the_slot() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("worker-compose.yaml");
        std::fs::write(&file, "containers: {}\n").unwrap();
        let held = slot(&file, "api").await;
        let waiting = tokio::spawn({
            let file = file.clone();
            async move { slot(&file, "api").await.stopping() }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let first = stop(&file, "api").await;
        let second = stop(&file, "api").await;
        assert!(held.stopping(), "the holder does not see the remove");
        drop(held);
        assert!(
            waiting.await.unwrap(),
            "a taker that waited missed the remove"
        );

        drop(first);
        assert!(
            slot(&file, "api").await.stopping(),
            "the mark ended with the first remove"
        );
        assert!(!slot(&file, "web").await.stopping());
        drop(second);
        assert!(!slot(&file, "api").await.stopping());
    }

    /// What an add or update rebuilt is pending until the container starts again; a build of
    /// another kind never is.
    #[tokio::test]
    async fn a_rebuild_is_pending_until_its_container_starts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("worker-compose.yaml");
        std::fs::write(&file, "containers: {}\n").unwrap();
        assert!(!rebuilt_unrestarted(&file, "api"));
        mark_rebuilt(&file, "api");
        assert!(rebuilt_unrestarted(&file, "api"));
        assert!(!rebuilt_unrestarted(&file, "web"));
        slot(&file, "api").await.starting();
        assert!(!rebuilt_unrestarted(&file, "api"));
    }
}
