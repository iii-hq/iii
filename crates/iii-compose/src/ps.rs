// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Every iii process this user runs: engines, compose daemons and workers, and
//! the compose each worker belongs to.
//!
//! Two signals identify a process, so neither has to be universal:
//!
//! - Compose gives every child `III_COMPOSE_FILE`, `III_WORKER_NAME` and
//!   `III_URL`. That finds a compose worker whatever SDK it was built with,
//!   including registry binaries released before anything here existed.
//! - Engines and SDK workers write a marker, `~/.iii/run/<kind>-<pid>*.json`,
//!   which finds them however they were started. A marker outlives a process
//!   that was killed, so one is trusted only while its PID is alive and
//!   started before the marker was written; anything else is deleted.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::process::{BirthIdentity, REAPER_ENV, birth_identity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Engine,
    Compose,
    Worker,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct IiiProcess {
    pub pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ppid: Option<u32>,
    pub kind: Kind,
    /// The worker's name. For a compose worker, its container key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Engine ports: the ones an engine listens on (one per
    /// `iii-worker-manager`), the one a worker was pointed at, the one a
    /// compose daemon's project uses.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compose_file: Option<PathBuf>,
    /// A compose worker whose compose daemon is no longer running: nothing
    /// supervises it and nothing will stop it.
    pub orphan: bool,
    /// Seconds since the unix epoch.
    pub started_at: u64,
    pub command: String,
    #[serde(skip)]
    #[schemars(skip)]
    birth: BirthIdentity,
}

/// What an engine or SDK worker writes about itself.
#[derive(Debug, Serialize, Deserialize)]
pub struct Marker {
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk: Option<String>,
    /// Milliseconds since the unix epoch.
    pub started_at: u64,
}

/// Where engines and SDK workers announce themselves.
pub fn run_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".iii").join("run"))
}

/// Removes this process's marker when dropped.
pub struct MarkerGuard(PathBuf);

impl Drop for MarkerGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Announces one listener of this engine. Best effort: a home directory that
/// cannot be written only makes the engine harder to find. One file per
/// listener, since an engine can serve several `iii-worker-manager`s.
pub fn announce_engine(url: &str) -> Option<MarkerGuard> {
    let dir = run_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let pid = std::process::id();
    let marker = Marker {
        pid,
        url: Some(url.to_string()),
        name: None,
        sdk: None,
        started_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or_default(),
    };
    let port = url_port(url).unwrap_or_default();
    let path = dir.join(format!("engine-{pid}-{port}.json"));
    std::fs::write(&path, serde_json::to_vec(&marker).ok()?).ok()?;
    Some(MarkerGuard(path))
}

/// Every iii process this user runs, engines first, grouped by compose file.
pub fn scan() -> Vec<IiiProcess> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cmd(UpdateKind::Always)
            .with_environ(UpdateKind::Always)
            .with_exe(UpdateKind::Always)
            .with_cwd(UpdateKind::Always)
            .with_user(UpdateKind::Always),
    );
    let me = sysinfo::get_current_pid()
        .ok()
        .and_then(|pid| system.process(pid))
        .and_then(|process| process.user_id().cloned());
    let markers = live_markers(&system);

    let mut found: Vec<IiiProcess> = system
        .processes()
        .values()
        .filter(|process| me.is_none() || process.user_id() == me.as_ref())
        .filter(|process| process.status() != sysinfo::ProcessStatus::Zombie)
        .filter_map(|process| classify(process, &markers))
        .collect();
    link(&mut found);
    found.sort_by(|a, b| {
        (&a.compose_file, a.kind, &a.name, a.pid).cmp(&(&b.compose_file, b.kind, &b.name, b.pid))
    });
    found
}

fn classify(process: &sysinfo::Process, markers: &Markers) -> Option<IiiProcess> {
    let pid = process.pid().as_u32();
    let cmd: Vec<String> = process
        .cmd()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let env = |name: &str| env_var(process.environ(), name);
    let mut found = IiiProcess {
        pid,
        ppid: process.parent().map(|pid| pid.as_u32()),
        kind: Kind::Worker,
        name: None,
        ports: Vec::new(),
        compose_file: None,
        orphan: false,
        started_at: process.start_time(),
        command: cmd.join(" "),
        birth: birth_identity(pid),
    };

    let marked = markers.get(&pid).map(Vec::as_slice).unwrap_or_default();
    let ports_of = |kind: &str| -> Vec<u16> {
        marked
            .iter()
            .filter(|(marked, _)| marked == kind)
            .filter_map(|(_, marker)| marker.url.as_deref().and_then(url_port))
            .collect()
    };

    if is_iii_binary(process, &cmd) {
        // The reaper is compose's own machinery, not something to list.
        if env(REAPER_ENV).is_some() {
            return None;
        }
        let cwd = process.cwd().unwrap_or(Path::new("."));
        let (role, args) = invocation(&cmd);
        match role {
            Role::Engine => {
                found.kind = Kind::Engine;
                found.ports = ports_of("engine");
                if found.ports.is_empty() {
                    found.ports = config_ports(cwd, &args);
                }
            }
            Role::Compose => {
                found.kind = Kind::Compose;
                found.compose_file = Some(compose_file_arg(cwd, &args));
            }
            Role::Other => return None,
        }
        return Some(found);
    }

    if let (Some(file), Some(name)) = (env("III_COMPOSE_FILE"), env("III_WORKER_NAME")) {
        found.compose_file = Some(PathBuf::from(file));
        found.name = Some(name.to_string());
        found.ports = env("III_URL").and_then(url_port).into_iter().collect();
        return Some(found);
    }

    let (kind, marker) = marked.first()?;
    found.kind = if kind == "engine" {
        Kind::Engine
    } else {
        Kind::Worker
    };
    found.name = marker.name.clone();
    found.ports = ports_of(kind);
    Some(found)
}

/// Orphans, managed engines' projects, and each daemon's engine ports.
fn link(found: &mut [IiiProcess]) {
    let daemons: BTreeMap<u32, PathBuf> = found
        .iter()
        .filter(|process| process.kind == Kind::Compose)
        .filter_map(|process| Some((process.pid, process.compose_file.clone()?)))
        .collect();
    let live: BTreeSet<&PathBuf> = daemons.values().collect();
    // Every port a project's engine listens on or its workers dial.
    let mut ports: BTreeMap<PathBuf, BTreeSet<u16>> = BTreeMap::new();

    for process in found.iter_mut() {
        match process.kind {
            Kind::Worker => {
                process.orphan = process
                    .compose_file
                    .as_ref()
                    .is_some_and(|file| !live.contains(file));
            }
            // A managed engine is the daemon's direct child.
            Kind::Engine => {
                process.compose_file = process.ppid.and_then(|ppid| daemons.get(&ppid).cloned());
            }
            Kind::Compose => continue,
        }
        if let Some(file) = &process.compose_file {
            ports
                .entry(file.clone())
                .or_default()
                .extend(&process.ports);
        }
    }
    for process in found
        .iter_mut()
        .filter(|process| process.kind == Kind::Compose)
    {
        process.ports = process
            .compose_file
            .as_ref()
            .and_then(|file| ports.get(file))
            .map(|ports| ports.iter().copied().collect())
            .unwrap_or_default();
    }
}

fn env_var<'a>(environ: &'a [std::ffi::OsString], name: &str) -> Option<&'a str> {
    environ.iter().find_map(|entry| {
        let entry = entry.to_str()?;
        let (key, value) = entry.split_once('=')?;
        (key == name && !value.is_empty()).then_some(value)
    })
}

fn is_iii_binary(process: &sysinfo::Process, cmd: &[String]) -> bool {
    let is_iii = |name: &OsStr| name == "iii" || name == "iii.exe";
    process.exe().and_then(Path::file_name).is_some_and(is_iii)
        || cmd.first().is_some_and(|arg0| {
            arg0.starts_with("iii:") || Path::new(arg0).file_name().is_some_and(is_iii)
        })
}

enum Role {
    Engine,
    Compose,
    Other,
}

/// What an `iii` command line is doing, and its arguments after argv[0].
fn invocation(cmd: &[String]) -> (Role, Vec<String>) {
    let args: Vec<String> = cmd.iter().skip(1).cloned().collect();
    // Linux labels the compose CLI and its managed engine in argv[0].
    match cmd.first().map(String::as_str) {
        Some(arg0) if arg0.starts_with("iii:e:") => return (Role::Engine, args),
        Some(arg0) if arg0.starts_with("iii:c:") => return (Role::Compose, args),
        _ => {}
    }
    let mut positional = args.iter().enumerate().filter(|(index, arg)| {
        !arg.starts_with('-')
            && !(*index > 0 && matches!(args[index - 1].as_str(), "-c" | "--config"))
    });
    let role = match positional.next().map(|(_, arg)| arg.as_str()) {
        None => Role::Engine,
        // `compose logs` and `compose build` are clients, not the daemon.
        Some("compose") if !args.iter().any(|arg| arg == "logs" || arg == "build") => Role::Compose,
        Some(_) => Role::Other,
    };
    (role, args)
}

fn flag_value<'a>(args: &'a [String], short: &str, long: &str) -> Option<&'a str> {
    let long_eq = format!("{long}=");
    args.iter().enumerate().find_map(|(index, arg)| {
        if arg == short || arg == long {
            args.get(index + 1).map(String::as_str)
        } else {
            arg.strip_prefix(&long_eq)
        }
    })
}

fn compose_file_arg(cwd: &Path, args: &[String]) -> PathBuf {
    let file =
        cwd.join(flag_value(args, "-f", "--file").unwrap_or(crate::cli::DEFAULT_COMPOSE_FILE));
    file.canonicalize().unwrap_or(file)
}

/// The ports an engine without a marker listens on, read from its config.
/// Only engines older than the marker need this.
fn config_ports(cwd: &Path, args: &[String]) -> Vec<u16> {
    let config = cwd.join(flag_value(args, "-c", "--config").unwrap_or("config.yaml"));
    let Some(config) = std::fs::read_to_string(config)
        .ok()
        .and_then(|text| serde_yaml::from_str::<serde_yaml::Value>(&text).ok())
    else {
        return Vec::new();
    };
    let workers = config
        .get("workers")
        .and_then(|workers| workers.as_sequence());
    workers
        .into_iter()
        .flatten()
        .filter(|worker| {
            worker
                .get("name")
                .and_then(|name| name.as_str())
                .is_some_and(|name| name.split('#').next() == Some("iii-worker-manager"))
        })
        .filter_map(
            |manager| match manager.get("config").and_then(|config| config.get("port")) {
                Some(port) => port.as_u64().and_then(|port| u16::try_from(port).ok()),
                None => Some(49134),
            },
        )
        .collect()
}

fn url_port(url: &str) -> Option<u16> {
    url::Url::parse(url).ok()?.port_or_known_default()
}

/// Each live PID's markers, with the kind its file name gives.
type Markers = BTreeMap<u32, Vec<(String, Marker)>>;

/// Markers whose process is still the one that wrote them. The rest are
/// deleted: their writer is gone, or its PID now belongs to somebody else.
fn live_markers(system: &System) -> Markers {
    let Some(entries) = run_dir().and_then(|dir| std::fs::read_dir(dir).ok()) else {
        return BTreeMap::new();
    };
    let mut live = Markers::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some((kind, _)) = path
            .file_stem()
            .and_then(OsStr::to_str)
            .and_then(|stem| stem.split_once('-'))
        else {
            continue;
        };
        let marker: Option<Marker> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let alive = marker.as_ref().is_some_and(|marker| {
            system
                .process(sysinfo::Pid::from_u32(marker.pid))
                // Start times are whole seconds; allow the one it rounds into.
                .is_some_and(|process| process.start_time() <= marker.started_at / 1000 + 1)
        });
        match marker {
            Some(marker) if alive => {
                live.entry(marker.pid)
                    .or_default()
                    .push((kind.to_string(), marker));
            }
            _ => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    live
}

/// Stops `targets` and returns the PIDs still running afterwards.
///
/// Compose daemons go first, each with SIGTERM, so they run their own teardown
/// and stop their projects in dependency order. Whatever is left then gets
/// SIGTERM and, after `grace`, SIGKILL. `force` sends SIGKILL to everything at
/// once. A PID is only ever signalled while it is still the process that was
/// scanned.
pub async fn stop(targets: &[IiiProcess], grace: Duration, force: bool) -> Vec<u32> {
    let (daemons, rest): (Vec<&IiiProcess>, Vec<&IiiProcess>) = targets
        .iter()
        .partition(|process| process.kind == Kind::Compose);
    for batch in [daemons, rest] {
        for process in &batch {
            signal(process, force);
        }
        if !wait_gone(&batch, grace).await && !force {
            for process in &batch {
                signal(process, true);
            }
            wait_gone(&batch, Duration::from_secs(2)).await;
        }
    }
    targets
        .iter()
        .filter(|process| is_running(process))
        .map(|process| process.pid)
        .collect()
}

fn is_running(process: &IiiProcess) -> bool {
    crate::process::is_running(process.pid)
        && process.birth.matches(&birth_identity(process.pid))
        && !is_zombie(process.pid)
}

/// A zombie has exited and only waits for its parent to collect it. Signal 0
/// still answers for it, and under a PID 1 that never collects orphans (a
/// container without an init) it answers forever. macOS cannot read a
/// zombie's start time, so there it already fails the birth check.
fn is_zombie(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| Some(stat.rsplit_once(')')?.1.trim_start().starts_with('Z')))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        false
    }
}

async fn wait_gone(batch: &[&IiiProcess], grace: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + grace;
    loop {
        if !batch.iter().any(|process| is_running(process)) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(unix)]
fn signal(process: &IiiProcess, kill: bool) {
    use nix::{
        sys::signal::{Signal, kill as kill_pid, killpg},
        unistd::{Pid, getpgid},
    };

    if !is_running(process) {
        return;
    }
    let signal = if kill {
        Signal::SIGKILL
    } else {
        Signal::SIGTERM
    };
    let pid = Pid::from_raw(process.pid as i32);
    // A compose child leads its own group, which reaches everything it
    // spawned. Anything else may share a terminal job with strangers.
    if process.compose_file.is_some()
        && process.kind == Kind::Worker
        && getpgid(Some(pid)) == Ok(pid)
    {
        let _ = killpg(pid, signal);
    } else {
        let _ = kill_pid(pid, signal);
    }
}

#[cfg(not(unix))]
fn signal(process: &IiiProcess, _kill: bool) {
    if !is_running(process) {
        return;
    }
    let mut system = System::new();
    let pid = sysinfo::Pid::from_u32(process.pid);
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    if let Some(found) = system.process(pid) {
        found.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn command_lines_say_what_an_iii_process_is() {
        let role = |line| invocation(&args(line)).0;
        assert!(matches!(role("iii"), Role::Engine));
        assert!(matches!(
            role("/usr/bin/iii --config compose"),
            Role::Engine
        ));
        assert!(matches!(
            role("iii -c x.yaml --no-update-check"),
            Role::Engine
        ));
        assert!(matches!(role("iii compose --up"), Role::Compose));
        assert!(matches!(role("iii compose -f a.yaml --up"), Role::Compose));
        assert!(matches!(role("iii compose --up -n orders"), Role::Compose));
        assert!(matches!(role("iii:c:orders compose --up"), Role::Compose));
        assert!(matches!(role("iii:e:orders --config x"), Role::Engine));
        for other in [
            "iii compose logs",
            "iii compose build",
            "iii call x::y",
            "iii trigger x::y",
            "iii ps",
        ] {
            assert!(matches!(role(other), Role::Other), "{other}");
        }
    }

    #[test]
    fn a_compose_file_flag_wins_over_the_default() {
        let cwd = Path::new("/nowhere");
        assert_eq!(
            compose_file_arg(cwd, &args("compose --up")),
            cwd.join("worker-compose.yaml")
        );
        assert_eq!(
            compose_file_arg(cwd, &args("compose --up -f other.yaml")),
            cwd.join("other.yaml")
        );
        assert_eq!(
            compose_file_arg(cwd, &args("compose --up --file=/abs/c.yaml")),
            PathBuf::from("/abs/c.yaml")
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_exited_process_nobody_collected_is_not_running() {
        let mut child = std::process::Command::new("sleep")
            .arg("0.2")
            .spawn()
            .unwrap();
        let pid = child.id();
        let birth = birth_identity(pid);
        // Exited but not waited for: a zombie until `wait` below.
        std::thread::sleep(Duration::from_millis(600));
        assert!(crate::process::is_running(pid), "signal 0 still answers");
        let zombie = IiiProcess {
            pid,
            ppid: None,
            kind: Kind::Worker,
            name: None,
            ports: Vec::new(),
            compose_file: None,
            orphan: false,
            started_at: 0,
            command: String::new(),
            birth,
        };
        assert!(!is_running(&zombie));
        child.wait().unwrap();
    }

    #[test]
    fn workers_of_a_dead_daemon_are_orphans() {
        let process = |pid, kind, file: &str| IiiProcess {
            pid,
            ppid: Some(1),
            kind,
            name: None,
            ports: vec![5000],
            compose_file: Some(PathBuf::from(file)),
            orphan: false,
            started_at: 0,
            command: String::new(),
            birth: BirthIdentity::Unavailable,
        };
        let mut found = vec![
            process(1, Kind::Compose, "/live.yaml"),
            process(2, Kind::Worker, "/live.yaml"),
            process(3, Kind::Worker, "/dead.yaml"),
            // The daemon's managed engine, with a second listener.
            process(4, Kind::Engine, ""),
        ];
        found[0].ports.clear();
        found[3].ports.push(5001);
        link(&mut found);
        assert!(!found[1].orphan);
        assert!(found[2].orphan);
        assert_eq!(
            found[0].ports,
            [5000, 5001],
            "a daemon reports every port of its project"
        );
    }
}
