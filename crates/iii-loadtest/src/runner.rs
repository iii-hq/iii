// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii-loadtest run`: one scenario against a fresh engine on this machine.
//!
//! Starts the engine pinned to `--engine-cpus` (in a cgroup of its own capped
//! at `--engine-mem` MiB, when this process may create one: as root inside
//! the VM), samples it, runs the scenario from `--load-cpus`, stops the
//! engine and writes what happened to `--out`. The suite runs this inside a
//! microVM for each VM profile, and calls [`execute`] directly for `host`.

use std::ffi::CString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::monitor::Monitor;
use crate::scenarios::{self, Env, Opts, Step};

#[derive(clap::Args, Debug, Clone)]
pub struct RunArgs {
    /// The scenario (see `iii-loadtest scenarios`).
    #[arg(long)]
    pub scenario: String,

    /// The engine binary.
    #[arg(long, default_value = "iii")]
    pub engine: PathBuf,

    /// The engine's working directory: its config, log and data go here.
    #[arg(long)]
    pub workdir: PathBuf,

    /// The engine's WebSocket port.
    #[arg(long, default_value_t = 28101)]
    pub port: u16,

    /// CPUs the engine may run on, e.g. `0-1`.
    #[arg(long)]
    pub engine_cpus: String,

    /// CPUs the load generator runs on.
    #[arg(long)]
    pub load_cpus: Option<String>,

    /// Cap the engine's memory at this many MiB (cgroup `memory.max`).
    #[arg(long)]
    pub engine_mem: Option<u64>,

    /// Measure an engine that is already running, at this pid, instead of
    /// starting one (`--port` is where it listens; `--engine-cpus` the CPUs it
    /// runs on, for "CPUs busy"). Its config is its own.
    #[arg(long)]
    pub attach: Option<u32>,

    #[command(flatten)]
    pub settings: Settings,

    /// Write the result (JSON) here.
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// The profile name recorded in the result.
    #[arg(long, default_value = "")]
    pub profile: String,
}

/// What every run of a suite shares.
#[derive(clap::Args, Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Measured seconds of each load step.
    #[arg(long, default_value_t = 10)]
    pub step_secs: u64,

    /// Unmeasured load before a scenario's first step.
    #[arg(long, default_value_t = 2)]
    pub warmup_secs: u64,

    /// Fewer, shorter steps: checks that everything works in a few minutes.
    #[arg(long)]
    pub quick: bool,

    /// Engine `observers:` setting, `key=value` (e.g. `max_per_connection=4096`).
    /// Repeatable.
    #[arg(long = "observers")]
    pub observers: Vec<String>,

    /// The engine's log level. `info` is what ships; it logs every
    /// registration, which `connections` and `observers` make by the thousand.
    #[arg(long, default_value = "info")]
    pub engine_log_level: String,

    /// `default` keeps the engine's observability as it ships (every span
    /// recorded, traces kept in memory and on disk); `off` turns it off, to
    /// see what it costs.
    #[arg(long, default_value = "default", value_parser = ["default", "off"])]
    pub observability: String,
}

impl Settings {
    pub fn opts(&self) -> Opts {
        Opts {
            step: Duration::from_secs(if self.quick {
                self.step_secs.min(3)
            } else {
                self.step_secs
            }),
            warmup: Duration::from_secs(if self.quick { 1 } else { self.warmup_secs }),
            quick: self.quick,
        }
    }

    /// The flags that give a `run` inside a VM these same settings.
    pub fn to_args(&self) -> Vec<String> {
        let mut args = vec![
            "--step-secs".to_string(),
            self.step_secs.to_string(),
            "--warmup-secs".to_string(),
            self.warmup_secs.to_string(),
            "--engine-log-level".to_string(),
            self.engine_log_level.clone(),
            "--observability".to_string(),
            self.observability.clone(),
        ];
        if self.quick {
            args.push("--quick".to_string());
        }
        for kv in &self.observers {
            args.push("--observers".to_string());
            args.push(kv.clone());
        }
        args
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub profile: String,
    pub scenario: String,
    pub ok: bool,
    pub error: Option<String>,
    /// The engine's memory cap, when it had one.
    pub engine_mem_cap_mb: Option<u64>,
    pub engine_cpus: usize,
    /// The kernel killed the engine for exceeding its memory cap.
    pub oom_killed: bool,
    pub steps: Vec<Step>,
}

/// `iii-loadtest run`: pins this process, runs, writes the result.
pub fn main(args: RunArgs) -> Result<()> {
    stop_children_on_signal();
    if let Some(cpus) = &args.load_cpus {
        pin(&parse_cpus(cpus)?)?;
    }
    bring_up_loopback();
    crate::vm::raise_nofile();
    let workers = args
        .load_cpus
        .as_deref()
        .map(|c| parse_cpus(c).map(|v| v.len()))
        .transpose()?
        .unwrap_or(4)
        .max(2);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()?;
    let result = runtime.block_on(execute(&args));
    let text = serde_json::to_vec_pretty(&result)?;
    match &args.out {
        Some(path) => {
            std::fs::write(path, text).with_context(|| format!("write {}", path.display()))?
        }
        None => println!("{}", String::from_utf8_lossy(&text)),
    }
    // Do not wait for the SDK clients' threads: the result is written.
    std::process::exit(0)
}

/// Runs the scenario; never fails, a failure is in the result.
pub async fn execute(args: &RunArgs) -> RunResult {
    let mut result = RunResult {
        profile: args.profile.clone(),
        scenario: args.scenario.clone(),
        ok: false,
        error: None,
        engine_mem_cap_mb: None,
        engine_cpus: 0,
        oom_killed: false,
        steps: Vec::new(),
    };
    let engine_cpus = match parse_cpus(&args.engine_cpus) {
        Ok(cpus) => cpus,
        Err(e) => {
            result.error = Some(format!("{e:#}"));
            return result;
        }
    };
    result.engine_cpus = engine_cpus.len();
    let (mut engine, pid, cgroup) = match args.attach {
        Some(pid) => (None, pid, None),
        None => {
            let cgroup = engine_cgroup(args.engine_mem);
            if cgroup.is_some() {
                result.engine_mem_cap_mb = args.engine_mem;
            }
            match spawn_engine(args, &engine_cpus, cgroup.as_deref()) {
                Ok(child) => {
                    track(&child);
                    (Some(child), 0, cgroup)
                }
                Err(e) => {
                    result.error = Some(format!("{e:#}"));
                    return result;
                }
            }
        }
    };
    let pid = engine.as_ref().map_or(pid, Child::id);
    let monitor = Monitor::start(pid, engine_cpus.clone(), cgroup.clone());
    let url = format!("ws://127.0.0.1:{}", args.port);
    let outcome = match wait_engine(&url, &monitor).await {
        Err(e) => Err(e),
        Ok(()) => {
            let env = Env {
                url,
                opts: args.settings.opts(),
                monitor: monitor.clone(),
                cpus: engine_cpus.len(),
                steps: Default::default(),
            };
            // A scenario that hangs (a lost observer, a wedged engine) must
            // not hold the whole suite.
            let limit = Duration::from_secs(if args.settings.quick { 600 } else { 3600 });
            let run = tokio::time::timeout(limit, scenarios::run(&args.scenario, &env)).await;
            result.steps = std::mem::take(&mut *env.steps.lock().unwrap());
            match run {
                Ok(r) => r,
                Err(_) => Err(anyhow::anyhow!("timed out after {limit:?}")),
            }
        }
    };
    result.oom_killed = monitor.oom_kills() > 0;
    monitor.stop();
    if let Some(engine) = &mut engine {
        stop(engine);
    }
    match outcome {
        Ok(()) => result.ok = true,
        Err(e) => {
            let mut message = format!("{e:#}");
            if result.oom_killed {
                message = format!(
                    "the engine was OOM-killed at its {} MiB cap ({message})",
                    args.engine_mem.unwrap_or(0)
                );
            }
            result.error = Some(message);
        }
    }
    result
}

/// The engine's config: the WebSocket port, and the run's `observers:`.
pub fn engine_config(port: u16, observers: &[String]) -> String {
    let mut yaml = format!(
        "workers:\n  - name: iii-worker-manager\n    config:\n      host: 127.0.0.1\n      port: {port}\n"
    );
    if !observers.is_empty() {
        yaml.push_str("observers:\n");
        for kv in observers {
            if let Some((k, v)) = kv.split_once('=') {
                yaml.push_str(&format!("  {k}: {v}\n"));
            }
        }
    }
    yaml
}

/// The engine's persisted `iii-observability` configuration entry, when the
/// run changes it: config.yaml may not declare that worker, and the
/// persisted entry is what the engine reads at boot.
pub fn observability_entry(settings: &Settings) -> Option<String> {
    let mut value = String::new();
    if settings.engine_log_level != "info" {
        value.push_str(&format!("  level: {}\n", settings.engine_log_level));
    }
    if settings.observability == "off" {
        value.push_str("  enabled: false\n");
    }
    (!value.is_empty()).then(|| {
        format!("id: iii-observability\nname: Observability\ndescription: set by iii-loadtest\nvalue:\n{value}")
    })
}

fn spawn_engine(args: &RunArgs, cpus: &[usize], cgroup: Option<&Path>) -> Result<Child> {
    let dir = &args.workdir;
    std::fs::create_dir_all(dir)?;
    // Each run starts from a clean slate: no entries or data from the last.
    for stale in ["config", "data"] {
        let _ = std::fs::remove_dir_all(dir.join(stale));
    }
    std::fs::write(
        dir.join("config.yaml"),
        engine_config(
            args.port,
            &scenarios::observer_settings(&args.scenario, &args.settings.observers),
        ),
    )?;
    if let Some(entry) = observability_entry(&args.settings) {
        std::fs::create_dir_all(dir.join("config"))?;
        std::fs::write(dir.join("config/iii-observability.yaml"), entry)?;
    }
    // The engine writes its log to stdout from its own runtime threads, so a
    // slow stdout slows the engine. It goes through a pipe that this process
    // drains into engine.log, as a container runtime would, not straight into
    // a file on the VM's shared directory.
    let (reader, writer) = std::io::pipe()?;
    let mut cmd = Command::new(&args.engine);
    cmd.args(["-c", "config.yaml"])
        .current_dir(dir)
        .env("III_TELEMETRY_ENABLED", "false")
        .env_remove("III_URL")
        .stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer);
    let log_path = dir.join("engine.log");
    std::thread::spawn(move || drain_log(reader, &log_path));

    let procs = cgroup
        .map(|c| CString::new(c.join("cgroup.procs").to_string_lossy().as_bytes()))
        .transpose()?;
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    for &c in cpus {
        unsafe { libc::CPU_SET(c, &mut set) };
    }
    unsafe {
        // Between fork and exec: only async-signal-safe calls.
        cmd.pre_exec(move || {
            // Own process group, so stopping it reaches every engine thread's
            // children too.
            libc::setpgid(0, 0);
            libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
            if let Some(path) = &procs {
                // "0" moves the writing process: the engine-to-be.
                let fd = libc::open(path.as_ptr(), libc::O_WRONLY);
                if fd >= 0 {
                    libc::write(fd, b"0".as_ptr().cast(), 1);
                    libc::close(fd);
                }
            }
            Ok(())
        });
    }
    cmd.spawn()
        .with_context(|| format!("spawn {}", args.engine.display()))
}

/// Most of the engine's log kept on disk; past it the log is drained and
/// counted, not written (a log storm must not fill the disk).
const LOG_CAP: u64 = 256 * 1024 * 1024;

/// Copies the engine's output to `path` until the engine closes it.
fn drain_log(mut from: std::io::PipeReader, path: &Path) {
    use std::io::{Read, Write};
    let Ok(file) = std::fs::File::create(path) else {
        let _ = std::io::copy(&mut from, &mut std::io::sink());
        return;
    };
    let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
    let mut buf = vec![0u8; 1 << 16];
    let (mut written, mut dropped) = (0u64, 0u64);
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if written + n as u64 <= LOG_CAP {
            if out.write_all(&buf[..n]).is_err() {
                break;
            }
            written += n as u64;
        } else {
            dropped += n as u64;
        }
    }
    if dropped > 0 {
        let _ = writeln!(
            out,
            "\n[iii-loadtest] log capped at {LOG_CAP} bytes; {dropped} more bytes not kept"
        );
    }
    let _ = out.flush();
}

/// A cgroup of the engine's own, capped at `mem_mib`, when this process may
/// make one (root inside the VM). `None` on the host: the engine is then
/// measured, not capped.
fn engine_cgroup(mem_mib: Option<u64>) -> Option<PathBuf> {
    let root = Path::new("/sys/fs/cgroup");
    let controllers = std::fs::read_to_string(root.join("cgroup.subtree_control")).ok()?;
    if !controllers.split_whitespace().any(|c| c == "memory") {
        std::fs::write(root.join("cgroup.subtree_control"), "+memory").ok()?;
    }
    let dir = root.join("iii-engine");
    std::fs::create_dir_all(&dir).ok()?;
    if let Some(mib) = mem_mib {
        std::fs::write(dir.join("memory.max"), (mib * 1024 * 1024).to_string()).ok()?;
        // No swap: the cap is the memory the engine has.
        let _ = std::fs::write(dir.join("memory.swap.max"), "0");
    }
    Some(dir)
}

/// The process group the run has running (an engine, or a VM), so a suite
/// stopped by a signal takes it down with it: each lives in a group of its
/// own and would outlive the suite otherwise.
pub static CHILD_PGID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Records `child` as the group to stop on a signal.
pub fn track(child: &Child) {
    CHILD_PGID.store(child.id() as i32, std::sync::atomic::Ordering::SeqCst);
}

/// Stops the tracked group and exits, on SIGTERM or SIGINT. Call before any
/// thread starts: the signals are blocked in the caller, so every thread
/// inherits that and only the waiting thread receives them.
pub fn stop_children_on_signal() {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        std::thread::spawn(move || {
            let mut sig: libc::c_int = 0;
            libc::sigwait(&set, &mut sig);
            let pgid = CHILD_PGID.load(std::sync::atomic::Ordering::SeqCst);
            if pgid > 0 {
                libc::kill(-pgid, libc::SIGTERM);
            }
            std::process::exit(128 + sig);
        });
    }
}

/// SIGTERM to the engine's process group, SIGKILL if it is still there 5 s
/// later.
fn stop(child: &mut Child) {
    let pgid = child.id() as i32;
    unsafe { libc::kill(-pgid, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe { libc::kill(-pgid, libc::SIGKILL) };
    let _ = child.wait();
}

/// Waits until the engine answers a WebSocket with `workerregistered`.
async fn wait_engine(url: &str, monitor: &Arc<Monitor>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if monitor.exited() {
            bail!("the engine exited while starting: see engine.log");
        }
        if let Ok(Ok((mut ws, _))) = tokio::time::timeout(
            Duration::from_secs(2),
            tokio_tungstenite::connect_async(url),
        )
        .await
            && let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_secs(5), ws.next()).await
            && frame
                .to_text()
                .is_ok_and(|t| t.contains("workerregistered"))
        {
            let _ = ws.close(None).await;
            return Ok(());
        }
        if Instant::now() > deadline {
            bail!("the engine never accepted a worker on {url}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Pins the calling thread, and every thread it starts afterwards, to `cpus`.
pub fn pin(cpus: &[usize]) -> Result<()> {
    pin_thread(0, cpus)
}

/// Pins thread (or process) `tid` to `cpus`.
pub fn pin_thread(tid: i32, cpus: &[usize]) -> Result<()> {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        for &c in cpus {
            libc::CPU_SET(c, &mut set);
        }
        if libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set) != 0 {
            bail!(
                "sched_setaffinity({tid}, {cpus:?}): {}",
                std::io::Error::last_os_error()
            );
        }
    }
    Ok(())
}

pub fn parse_cpus(list: &str) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for part in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => out.extend(a.parse::<usize>()?..=b.parse::<usize>()?),
            None => out.push(part.parse()?),
        }
    }
    if out.is_empty() {
        bail!("empty CPU list {list:?}");
    }
    Ok(out)
}

pub fn format_cpus(cpus: &[usize]) -> String {
    cpus.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// iii-init leaves `lo` down when the VM has no network device. On the host
/// (or without the privilege) this changes nothing.
fn bring_up_loopback() {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if fd < 0 {
            return;
        }
        let mut req: libc::ifreq = std::mem::zeroed();
        for (dst, src) in req.ifr_name.iter_mut().zip(b"lo\0") {
            *dst = *src as libc::c_char;
        }
        if libc::ioctl(fd, libc::SIOCGIFFLAGS, &mut req) == 0 {
            let flags = req.ifr_ifru.ifru_flags;
            if flags & libc::IFF_UP as libc::c_short == 0 {
                req.ifr_ifru.ifru_flags = flags | libc::IFF_UP as libc::c_short;
                libc::ioctl(fd, libc::SIOCSIFFLAGS, &req);
            }
        }
        libc::close(fd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists_parse_and_format() {
        assert_eq!(
            parse_cpus("4-7,12-15").unwrap(),
            vec![4, 5, 6, 7, 12, 13, 14, 15]
        );
        assert_eq!(parse_cpus("0,8").unwrap(), vec![0, 8]);
        assert_eq!(format_cpus(&[0, 1, 8]), "0,1,8");
        assert!(parse_cpus("").is_err());
    }

    #[test]
    fn the_engine_config_carries_the_port_and_observer_settings() {
        let yaml: serde_json::Value =
            serde_yaml::from_str(&engine_config(28101, &["max_per_connection=4096".into()]))
                .unwrap();
        assert_eq!(yaml["workers"][0]["name"], "iii-worker-manager");
        assert_eq!(yaml["workers"][0]["config"]["port"], 28101);
        assert_eq!(yaml["observers"]["max_per_connection"], 4096);
    }

    #[test]
    fn a_scenario_raises_the_caps_it_needs_unless_the_run_sets_them() {
        let config = |run: &[&str]| -> serde_json::Value {
            let run: Vec<String> = run.iter().map(|kv| kv.to_string()).collect();
            serde_yaml::from_str(&engine_config(
                28101,
                &scenarios::observer_settings("match-keys", &run),
            ))
            .unwrap()
        };
        assert_eq!(config(&[])["observers"]["max_per_function"], 10240);
        let own = config(&["max_per_function=20000", "max_per_connection=4096"]);
        assert_eq!(own["observers"]["max_per_function"], 20000);
        assert_eq!(own["observers"]["max_per_connection"], 4096);
        assert!(scenarios::observer_settings("rpc", &[]).is_empty());
    }

    #[test]
    fn the_observability_entry_is_written_only_when_it_changes_something() {
        let mut settings = Settings {
            step_secs: 1,
            warmup_secs: 1,
            quick: false,
            observers: vec![],
            engine_log_level: "info".into(),
            observability: "default".into(),
        };
        assert!(observability_entry(&settings).is_none());
        settings.engine_log_level = "warn".into();
        settings.observability = "off".into();
        let entry: serde_json::Value =
            serde_yaml::from_str(&observability_entry(&settings).unwrap()).unwrap();
        assert_eq!(entry["id"], "iii-observability");
        assert_eq!(entry["value"]["level"], "warn");
        assert_eq!(entry["value"]["enabled"], false);
    }

    #[test]
    fn settings_round_trip_through_the_vm_command_line() {
        #[derive(clap::Parser)]
        struct Cli {
            #[command(flatten)]
            settings: Settings,
        }
        let settings = Settings {
            step_secs: 7,
            warmup_secs: 3,
            quick: true,
            observers: vec!["max_queue_per_observer=64".into()],
            engine_log_level: "warn".into(),
            observability: "off".into(),
        };
        let argv = std::iter::once("x".to_string()).chain(settings.to_args());
        let parsed = <Cli as clap::Parser>::parse_from(argv).settings;
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            serde_json::to_value(&settings).unwrap()
        );
    }
}
