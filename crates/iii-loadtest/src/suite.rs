// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii-loadtest suite`: every scenario on every machine profile.
//!
//! A profile is `host` (the engine as a plain process on this machine) or
//! `<vcpus>x<MiB>`: a microVM in which the engine gets that many vCPUs and is
//! capped at that much memory. The load generator runs in the same VM on vCPUs
//! of its own (`--load-vcpus`, `--load-ram`), so engine and load talk over
//! the guest's own loopback, as on one machine, and nothing proxies their
//! sockets. On the host, the engine's vCPU threads are pinned to one half of
//! the physical cores and the load generator's to the other, so the two never
//! share a core.
//!
//! Each scenario gets a fresh engine (a new VM or process, a clean working
//! directory). Output, under `--out`: `results.json`, `report.md`, and per
//! run `<profile>/<scenario>/` with the engine's config and log and, for a
//! VM, the guest console.

use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::rootfs::{self, BIN_DIR, RootfsArgs};
use crate::runner::{self, RunArgs, RunResult, Settings, format_cpus, parse_cpus, pin, pin_thread};
use crate::scenarios;

#[derive(clap::Args, Debug, Clone)]
pub struct SuiteArgs {
    /// The engine binary under test.
    #[arg(long, default_value = "target/release/iii")]
    pub engine: PathBuf,

    /// Machine profiles: `host`, or `<vcpus>x<MiB>` for a microVM whose
    /// engine gets that many vCPUs and that much memory.
    #[arg(long, value_delimiter = ',', default_value = "1x512,2x1024,4x2048")]
    pub profiles: Vec<String>,

    /// Scenarios to run, in order (see `iii-loadtest scenarios`).
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "rpc,fanout,filters,observers,emit,local,overload,connections"
    )]
    pub scenarios: Vec<String>,

    /// Results directory; defaults to `target/loadtest/<timestamp>`.
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// The engine's WebSocket port (inside a VM it is the guest's own).
    #[arg(long, default_value_t = 28101)]
    pub port: u16,

    /// vCPUs the load generator gets in each VM.
    #[arg(long, default_value_t = 6)]
    pub load_vcpus: u8,

    /// MiB of the VM's RAM for the load generator (on top of the engine's).
    #[arg(long, default_value_t = 2048)]
    pub load_ram: u32,

    /// Host CPUs for the engine (its vCPUs, or the `host` engine), e.g.
    /// `4-7,12-15`. Defaults to the second half of the physical cores.
    #[arg(long)]
    pub engine_cpus: Option<String>,

    /// Host CPUs for the load generator. Defaults to the first half.
    #[arg(long)]
    pub load_cpus: Option<String>,

    /// iii-init for the guest (default `~/.local/bin/iii-init`).
    #[arg(long)]
    pub init: Option<PathBuf>,

    /// libkrunfw (default `~/.iii/lib/libkrunfw.so`).
    #[arg(long)]
    pub krunfw: Option<PathBuf>,

    #[command(flatten)]
    pub settings: Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Profile {
    Host,
    Vm { vcpus: u8, ram_mib: u32 },
}

impl Profile {
    pub fn parse(s: &str) -> Result<Self> {
        if s == "host" {
            return Ok(Profile::Host);
        }
        let (cpus, ram) = s
            .split_once('x')
            .with_context(|| format!("profile {s:?}: expected `host` or `<vcpus>x<MiB>`"))?;
        let vcpus: u8 = cpus
            .parse()
            .with_context(|| format!("profile {s:?}: vcpus"))?;
        let ram_mib: u32 = ram
            .parse()
            .with_context(|| format!("profile {s:?}: memory in MiB"))?;
        if vcpus == 0 || ram_mib < 128 {
            bail!("profile {s:?}: at least 1 vCPU and 128 MiB");
        }
        Ok(Profile::Vm { vcpus, ram_mib })
    }

    pub fn name(&self) -> String {
        match self {
            Profile::Host => "host".to_string(),
            Profile::Vm { vcpus, ram_mib } => format!("{vcpus}x{ram_mib}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub started_unix: u64,
    pub engine: String,
    pub engine_version: String,
    pub git_rev: Option<String>,
    pub host_cpu: String,
    pub host_cpus: usize,
    pub host_mem_mb: u64,
    pub engine_host_cpus: Vec<usize>,
    pub load_host_cpus: Vec<usize>,
    pub load_vcpus: u8,
    pub load_ram_mb: u32,
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Results {
    pub meta: Meta,
    pub runs: Vec<RunResult>,
}

/// Splits the physical cores in two halves (SMT siblings stay together):
/// the load generator gets the first, the engine the second.
fn default_split() -> (Vec<usize>, Vec<usize>) {
    let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) }.max(1) as usize;
    let mut cores: Vec<Vec<usize>> = Vec::new();
    for cpu in 0..n {
        let siblings = std::fs::read_to_string(format!(
            "/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list"
        ))
        .ok()
        .and_then(|s| parse_cpus(s.trim()).ok())
        .unwrap_or_else(|| vec![cpu]);
        if !cores.contains(&siblings) {
            cores.push(siblings);
        }
    }
    let half = cores.len().div_ceil(2);
    let load = cores[..half].concat();
    let engine = if cores.len() > 1 {
        cores[half..].concat()
    } else {
        load.clone()
    };
    (load, engine)
}

pub fn run(args: SuiteArgs) -> Result<()> {
    let (default_load, default_engine) = default_split();
    let load_cpus = args
        .load_cpus
        .as_deref()
        .map(parse_cpus)
        .transpose()?
        .unwrap_or(default_load);
    let engine_cpus = args
        .engine_cpus
        .as_deref()
        .map(parse_cpus)
        .transpose()?
        .unwrap_or(default_engine);
    // Before the runtime starts, so every thread of this process inherits it.
    pin(&load_cpus)?;
    runner::stop_children_on_signal();
    crate::vm::raise_nofile();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(load_cpus.len().max(2))
        .enable_all()
        .build()?;
    runtime.block_on(run_all(args, engine_cpus, load_cpus))?;
    // SDK clients of the host runs may still hold threads: do not wait.
    std::process::exit(0)
}

async fn run_all(args: SuiteArgs, engine_cpus: Vec<usize>, load_cpus: Vec<usize>) -> Result<()> {
    let profiles: Vec<Profile> = args
        .profiles
        .iter()
        .map(|p| Profile::parse(p))
        .collect::<Result<_>>()?;
    for s in &args.scenarios {
        if !scenarios::ALL.contains(&s.as_str()) {
            bail!(
                "unknown scenario {s:?} (known: {})",
                scenarios::ALL.join(", ")
            );
        }
    }
    let engine = std::fs::canonicalize(&args.engine).with_context(|| {
        format!(
            "engine binary {} (build it with `cargo build --release -p iii`)",
            args.engine.display()
        )
    })?;
    if profiles.contains(&Profile::Host) {
        preflight(args.port)?;
    }

    let started_unix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("target/loadtest/{started_unix}")));
    std::fs::create_dir_all(&out)?;
    let out = std::fs::canonicalize(&out)?;

    let meta = Meta {
        started_unix,
        engine: engine.display().to_string(),
        engine_version: command_line(&engine, &["--version"]).unwrap_or_default(),
        git_rev: command_line(Path::new("git"), &["rev-parse", "--short", "HEAD"]),
        host_cpu: host_cpu_model(),
        host_cpus: unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) }.max(0) as usize,
        host_mem_mb: host_mem_mb(),
        engine_host_cpus: engine_cpus.clone(),
        load_host_cpus: load_cpus.clone(),
        load_vcpus: args.load_vcpus,
        load_ram_mb: args.load_ram,
        settings: args.settings.clone(),
    };
    eprintln!(
        "iii-loadtest: engine {} | engine on host CPUs {:?}, load on {:?} | results in {}",
        meta.engine_version,
        engine_cpus,
        load_cpus,
        out.display()
    );

    let rootfs = out.join("rootfs");
    if profiles.iter().any(|p| matches!(p, Profile::Vm { .. })) {
        rootfs::build(&RootfsArgs {
            out: rootfs.clone(),
            init: args.init.clone(),
            bins: vec![engine.clone(), std::env::current_exe()?],
        })?;
    }

    let mut results = Results {
        meta,
        runs: Vec::new(),
    };
    for profile in &profiles {
        for scenario in &args.scenarios {
            eprintln!(
                "\n▶ {} / {scenario}: {}",
                profile.name(),
                scenarios::describe(scenario)
            );
            let dir = out.join(profile.name()).join(scenario);
            std::fs::create_dir_all(&dir)?;
            let result = match profile {
                Profile::Host => {
                    runner::execute(&RunArgs {
                        scenario: scenario.clone(),
                        engine: engine.clone(),
                        workdir: dir.clone(),
                        port: args.port,
                        engine_cpus: format_cpus(&engine_cpus),
                        load_cpus: None,
                        engine_mem: None,
                        attach: None,
                        settings: args.settings.clone(),
                        out: None,
                        profile: profile.name(),
                    })
                    .await
                }
                Profile::Vm { vcpus, ram_mib } => {
                    let vm = VmRun {
                        args: &args,
                        name: profile.name(),
                        vcpus: *vcpus,
                        ram_mib: *ram_mib,
                        scenario,
                        dir: &dir,
                        engine: &engine,
                        rootfs: &rootfs,
                        engine_cpus: &engine_cpus,
                        load_cpus: &load_cpus,
                    };
                    let vm_result = tokio::task::block_in_place(|| vm.execute());
                    vm_result.unwrap_or_else(|e| RunResult {
                        profile: profile.name(),
                        scenario: scenario.clone(),
                        ok: false,
                        error: Some(format!("{e:#}")),
                        engine_mem_cap_mb: Some(u64::from(*ram_mib)),
                        engine_cpus: usize::from(*vcpus),
                        oom_killed: false,
                        steps: Vec::new(),
                    })
                }
            };
            if let Some(e) = &result.error {
                eprintln!("  ✗ {e}");
            }
            std::fs::write(dir.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
            results.runs.push(result);
            std::fs::write(
                out.join("results.json"),
                serde_json::to_vec_pretty(&results)?,
            )?;
            std::fs::write(out.join("report.md"), crate::report::render(&results))?;
        }
    }
    eprintln!("\nreport: {}", out.join("report.md").display());
    Ok(())
}

struct VmRun<'a> {
    args: &'a SuiteArgs,
    name: String,
    vcpus: u8,
    ram_mib: u32,
    scenario: &'a str,
    dir: &'a Path,
    engine: &'a Path,
    rootfs: &'a Path,
    engine_cpus: &'a [usize],
    load_cpus: &'a [usize],
}

impl VmRun<'_> {
    /// Boots the VM, which runs the scenario and writes `result.json` into
    /// the shared directory, and powers off.
    fn execute(&self) -> Result<RunResult> {
        let result_path = self.dir.join("result.json");
        for stale in ["result.json", "console.log", "engine.log"] {
            let _ = std::fs::remove_file(self.dir.join(stale));
        }
        let mut child = self.spawn()?;
        runner::track(&child);
        let pinned = pin_vcpus(
            child.id(),
            usize::from(self.vcpus),
            self.engine_cpus,
            self.load_cpus,
        );
        if pinned < usize::from(self.vcpus) + usize::from(self.args.load_vcpus) {
            eprintln!(
                "  (pinned {pinned} vCPU threads of {})",
                self.vcpus + self.args.load_vcpus
            );
        }

        let limit = Duration::from_secs(if self.args.settings.quick { 600 } else { 3600 } + 180);
        let started = Instant::now();
        let mut console = ConsoleTail::new(self.dir.join("console.log"));
        loop {
            console.print_new();
            if let Some(status) = child.try_wait()? {
                console.print_new();
                if !result_path.exists() {
                    bail!(
                        "the VM exited ({status}) without a result; console:\n{}",
                        console.last_lines(15)
                    );
                }
                break;
            }
            if started.elapsed() > limit {
                stop_vm(&mut child);
                bail!(
                    "the VM ran past {limit:?}; console:\n{}",
                    console.last_lines(15)
                );
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let text = std::fs::read_to_string(&result_path)?;
        Ok(serde_json::from_str(&text)?)
    }

    fn spawn(&self) -> Result<std::process::Child> {
        let me = std::env::current_exe()?;
        let engine_name = self
            .engine
            .file_name()
            .context("engine file name")?
            .to_string_lossy();
        let (n, k) = (usize::from(self.vcpus), usize::from(self.args.load_vcpus));
        let bin = format!("/{BIN_DIR}");
        // The guest's RAM: the engine's cap, the load generator's share, and
        // room for the guest kernel.
        let ram = self.ram_mib + self.args.load_ram + 256;
        let mut cmd = Command::new(&me);
        cmd.args([
            "vm",
            "--vcpus",
            &(n + k).to_string(),
            "--ram",
            &ram.to_string(),
        ])
        .arg("--rootfs")
        .arg(self.rootfs)
        .arg("--mount")
        .arg(format!("{}:/bench/run", self.dir.display()))
        .arg("--console")
        .arg(self.dir.join("console.log"))
        .args(["--env", "III_TELEMETRY_ENABLED=false"]);
        if let Some(k) = &self.args.krunfw {
            cmd.arg("--krunfw").arg(k);
        }
        cmd.arg("--")
            .arg(format!("{bin}/iii-loadtest"))
            .args(["run", "--scenario", self.scenario])
            .arg("--engine")
            .arg(format!("{bin}/{engine_name}"))
            .args([
                "--workdir",
                "/bench/run",
                "--port",
                &self.args.port.to_string(),
            ])
            .args(["--engine-cpus", &format!("0-{}", n - 1)])
            .args(["--load-cpus", &format!("{n}-{}", n + k - 1)])
            .args(["--engine-mem", &self.ram_mib.to_string()])
            .args(["--out", "/bench/run/result.json", "--profile", &self.name])
            .args(self.args.settings.to_args());
        let log = std::fs::File::create(self.dir.join("vm.log"))?;
        cmd.stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let load = self.load_cpus.to_vec();
        let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        for &c in &load {
            unsafe { libc::CPU_SET(c, &mut set) };
        }
        unsafe {
            cmd.pre_exec(move || {
                libc::setpgid(0, 0);
                // Every VM thread starts on the load generator's half; the
                // engine's vCPUs are moved to the other once they exist.
                libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
                Ok(())
            });
        }
        cmd.spawn()
            .with_context(|| format!("spawn {}", me.display()))
    }
}

/// Pins the VM's vCPU threads (`fc_vcpu <i>`): the engine's (`i < engine`)
/// to `engine_cpus`, the load generator's to `load_cpus`. Returns how many it
/// pinned; waits up to 15 s for them to appear.
fn pin_vcpus(pid: u32, engine: usize, engine_cpus: &[usize], load_cpus: &[usize]) -> usize {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut pinned = std::collections::HashSet::new();
    loop {
        if let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) {
            for task in tasks.flatten() {
                let Ok(tid) = task.file_name().to_string_lossy().parse::<i32>() else {
                    continue;
                };
                let comm = std::fs::read_to_string(task.path().join("comm")).unwrap_or_default();
                let Some(Ok(i)) = comm
                    .trim()
                    .strip_prefix("fc_vcpu ")
                    .map(str::parse::<usize>)
                else {
                    continue;
                };
                if pinned.contains(&tid) {
                    continue;
                }
                let cpus = if i < engine { engine_cpus } else { load_cpus };
                if pin_thread(tid, cpus).is_ok() {
                    pinned.insert(tid);
                }
            }
        }
        // vCPUs start together; once some exist and a moment passed, done.
        if (!pinned.is_empty() && Instant::now() + Duration::from_secs(13) > deadline)
            || Instant::now() > deadline
        {
            return pinned.len();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn stop_vm(child: &mut std::process::Child) {
    let pgid = child.id() as i32;
    unsafe { libc::kill(-pgid, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    unsafe { libc::kill(-pgid, libc::SIGKILL) };
    let _ = child.wait();
}

/// Follows the guest console and prints the runner's lines (not the
/// kernel's).
struct ConsoleTail {
    path: PathBuf,
    offset: u64,
    partial: String,
    lines: Vec<String>,
}

impl ConsoleTail {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            partial: String::new(),
            lines: Vec::new(),
        }
    }

    fn print_new(&mut self) {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return;
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        let Ok(n) = file.read_to_end(&mut buf) else {
            return;
        };
        self.offset += n as u64;
        self.partial.push_str(&String::from_utf8_lossy(&buf));
        while let Some(at) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=at).collect();
            let line = line.trim_end().to_string();
            if line.starts_with("    ") || line.starts_with("  ✗") {
                eprintln!("{line}");
            }
            self.lines.push(line);
        }
    }

    fn last_lines(&self, n: usize) -> String {
        let from = self.lines.len().saturating_sub(n);
        self.lines[from..].join("\n")
    }
}

/// Refuses a port that is taken, or that a running process (a worker left
/// over from another engine, say) is set to connect to: it would join the
/// engine under test and add load nobody accounts for.
fn preflight(port: u16) -> Result<()> {
    if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
        bail!("port {port} is in use: pick another with --port");
    }
    let needle = format!(":{port}");
    let mut stale = Vec::new();
    for entry in std::fs::read_dir("/proc")?.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(environ) = std::fs::read(entry.path().join("environ")) else {
            continue;
        };
        let targets_port = environ.split(|b| *b == 0).any(|var| {
            let var = String::from_utf8_lossy(var);
            var.starts_with("III_URL=")
                && (var.ends_with(&needle) || var.contains(&format!("{needle}/")))
        });
        if targets_port {
            stale.push(pid);
        }
    }
    if !stale.is_empty() {
        bail!(
            "processes {stale:?} have III_URL pointing at port {port} and would connect to the engine \
             under test: pick another --port"
        );
    }
    Ok(())
}

fn command_line(bin: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn host_cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_default()
}

fn host_mem_mb() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        })
        .map_or(0, |kb| kb / 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_parse() {
        assert_eq!(Profile::parse("host").unwrap(), Profile::Host);
        assert_eq!(
            Profile::parse("2x1024").unwrap(),
            Profile::Vm {
                vcpus: 2,
                ram_mib: 1024
            }
        );
        assert_eq!(Profile::parse("2x1024").unwrap().name(), "2x1024");
        for bad in ["2", "ax1024", "0x512", "1x64"] {
            assert!(Profile::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_console_tail_keeps_the_runners_lines() {
        let dir = std::env::temp_dir().join(format!("iii-loadtest-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("console.log");
        std::fs::write(&path, "[    0.1] kernel\n    step one\n  ✗ fail").unwrap();
        let mut tail = ConsoleTail::new(path.clone());
        tail.print_new();
        assert_eq!(tail.lines, vec!["[    0.1] kernel", "    step one"]);
        std::fs::write(&path, "[    0.1] kernel\n    step one\n  ✗ failed\n").unwrap();
        tail.print_new();
        assert_eq!(tail.last_lines(1), "  ✗ failed");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
