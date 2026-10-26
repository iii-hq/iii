//! Compose's acceptance run on the real `sandbox-libkrun` adapter.
//!
//! A registry bundle and a `path://` worker with `runtime.base_image` each boot
//! in a microVM that belongs to the adapter, restart after their VM is killed,
//! stop with `compose::down` and are destroyed by `compose::remove`. The bundle
//! run also sends one `sandbox::exec` through the engine's router.
//!
//! Both tests pull the pinned registry packages and boot microVMs, so they need
//! `/dev/kvm` (Linux) or Hypervisor.framework (Apple Silicon) and are ignored by
//! default. CI runs them in `compose-sandbox-e2e-linux`; by hand:
//! `cargo test --locked -p iii --test compose_sandbox_e2e -- --ignored --nocapture`.
#![cfg(unix)]

use std::{
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use serde_json::{Value, json};

/// Pinned so a new registry release cannot turn iii's CI red (D5).
const SANDBOX_LIBKRUN: &str = "1.0.0";
const ONBOARDING: &str = "0.1.20";

const NAMESPACE: &str = "sbx-e2e";
/// The `stop_timeout` of every project below.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// How long past `stop_timeout` `compose::down` may take.
const DOWN_SLACK: Duration = Duration::from_secs(10);
/// A first `up` downloads the packages and pulls the images.
const FIRST_UP: Duration = Duration::from_secs(15 * 60);
const RECOVERY: Duration = Duration::from_secs(60);
/// A call that only reads, or answers from what is already running.
const SHORT: Duration = Duration::from_secs(30);
/// A call that pulls images or boots a VM.
const LONG: Duration = Duration::from_secs(10 * 60);

fn iii_bin() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_iii"));
    // These fixtures own their engine endpoint and their state. Never inherit
    // the operator's: it would override the managed engine, or move the state
    // out of the private `HOME` these tests inspect.
    command
        .env_remove("III_URL")
        .env_remove("III_COMPOSE_NAMESPACE")
        .env_remove("III_COMPOSE_STATE_DIR")
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1");
    command
}

/// A process from `ps`. It has no `Debug`: `ps` lists every process on the
/// host, and a failed assertion must not print their command lines.
struct Proc {
    pid: u32,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    ppid: u32,
    args: String,
}

fn processes() -> Vec<Proc> {
    let output = Command::new("ps")
        .args(["-axww", "-o", "pid=,ppid=,command="])
        .output()
        .expect("run ps");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (ppid, args) = rest.trim_start().split_once(char::is_whitespace)?;
            Some(Proc {
                pid: pid.parse().ok()?,
                ppid: ppid.parse().ok()?,
                args: args.trim().to_string(),
            })
        })
        .collect()
}

/// This test's `__vm-boot` processes. Their command line names paths under the
/// private `HOME` (the adapter binary in the package cache and the VM state
/// dir), so its unique directory name picks them out from any other VM on the
/// host.
fn vm_boots(home: &Path) -> Vec<Proc> {
    let tag = home
        .file_name()
        .and_then(|name| name.to_str())
        .expect("home directory name");
    processes()
        .into_iter()
        .filter(|process| {
            process.args.contains(tag)
                && process
                    .args
                    .split_whitespace()
                    .any(|arg| arg == "__vm-boot")
        })
        .collect()
}

/// The microVM is the adapter's child, not Compose's. `sandbox-libkrun`
/// re-executes itself as `__vm-boot`, so the parent runs the same executable.
#[cfg(target_os = "linux")]
fn assert_vm_belongs_to_the_adapter(vm: &Proc, compose_pid: u32) {
    let all = processes();
    let parent = all
        .iter()
        .find(|process| process.pid == vm.ppid)
        .expect("the __vm-boot process has no live parent");
    assert_ne!(parent.pid, compose_pid, "__vm-boot is a child of Compose");
    let exe = |process: &Proc| process.args.split_whitespace().next().map(str::to_owned);
    assert_eq!(
        exe(parent),
        exe(vm),
        "the parent of __vm-boot is not the sandbox-libkrun process that re-executed itself"
    );
}

fn sigkill(pid: u32) {
    kill(Pid::from_raw(pid as i32), Signal::SIGKILL).expect("SIGKILL the __vm-boot process");
}

fn wait_for<T>(timeout: Duration, what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(found) = probe() {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "timed out after {timeout:?} waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn copy_fixture(to: &Path) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sandbox-vm-worker");
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

/// A project with its own `HOME`, engine port and foreground `iii compose --up`.
struct Project {
    dir: tempfile::TempDir,
    home: tempfile::TempDir,
    file: PathBuf,
    port: u16,
    compose: Option<Child>,
}

impl Project {
    /// `containers` is the operator's share of the `containers:` mapping, each
    /// entry indented two spaces. The adapter follows exactly as Compose writes
    /// it, pre-declared because `up` would resolve `^1` against the live
    /// registry, which a required check cannot depend on (D5).
    fn new(containers: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("sbx-e2e-project-")
            .tempdir()
            .unwrap();
        let home = tempfile::Builder::new()
            .prefix("sbx-e2e-home-")
            .tempdir()
            .unwrap();
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let file = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &file,
            format!(
                "stop_timeout: {}s\nstartup_timeout: 120s\nengine:\n  url: ws://127.0.0.1:{port}\ncontainers:\n{containers}  # added by compose::add\n  sandbox-libkrun:\n    worker: package://api.workers.iii.dev/sandbox-libkrun\n    version: {SANDBOX_LIBKRUN}\n",
                STOP_TIMEOUT.as_secs()
            ),
        )
        .unwrap();
        Self {
            dir,
            home,
            file,
            port,
            compose: None,
        }
    }

    fn start(&mut self) {
        let log = File::create(self.home.path().join("compose.log")).unwrap();
        self.compose = Some(
            iii_bin()
                .current_dir(self.dir.path())
                .env("HOME", self.home.path())
                .args(["compose", "--namespace", NAMESPACE, "--up"])
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .expect("run iii compose --up"),
        );
    }

    /// `iii trigger` against the project's engine. Compose answers in its own
    /// namespace and the router in `default`, hence the optional namespace.
    fn trigger(
        &self,
        namespace: Option<&str>,
        function: &str,
        payload: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let mut command = iii_bin();
        command
            .env("HOME", self.home.path())
            .args(["trigger", function, "--engine"])
            .arg(format!("ws://127.0.0.1:{}", self.port))
            .args(["--timeout-ms", &timeout.as_millis().to_string()])
            .args(["--json", &payload.to_string()]);
        if let Some(namespace) = namespace {
            command.args(["--namespace", namespace]);
        }
        let output = command.output().expect("run iii trigger");
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(if stdout.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&stdout).expect("iii trigger prints JSON")
        })
    }

    fn compose_call(&self, function: &str, payload: Value, timeout: Duration) -> Value {
        self.trigger(Some(NAMESPACE), function, payload, timeout)
            .unwrap_or_else(|error| panic!("{function} failed: {error}"))
    }

    fn state(&self, key: &str) -> Option<String> {
        let status = self
            .trigger(Some(NAMESPACE), "compose::status", json!({}), SHORT)
            .ok()?;
        status["containers"]
            .as_array()?
            .iter()
            .find(|container| container["container"] == key)?["state"]
            .as_str()
            .map(str::to_owned)
    }

    fn wait_ready(&mut self, key: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            if self.state(key).as_deref() == Some("ready") {
                return;
            }
            if let Some(status) = self.compose.as_mut().and_then(|c| c.try_wait().unwrap()) {
                panic!("compose exited with {status} before {key} was ready");
            }
            assert!(
                Instant::now() < deadline,
                "{key} not ready within {timeout:?}"
            );
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// The one VM the project runs. Call it before anything else boots one,
    /// such as an ephemeral sandbox.
    fn only_vm(&self) -> Proc {
        wait_for(SHORT, "exactly one __vm-boot process", || {
            let mut boots = vm_boots(self.home.path());
            (boots.len() == 1).then(|| boots.remove(0))
        })
    }

    /// `restart: always` brings the worker back after its VM was killed.
    fn recovers_after_kill(&self, key: &str, killed: u32) {
        wait_for(
            RECOVERY,
            &format!("{key} ready again after its VM was killed"),
            || {
                // The killed process can linger until the adapter reaps it, and a
                // `ready` read before the exit is noticed is the old VM's. Only a
                // new VM proves the restart, and `ready` after it is the new one's.
                let reborn = vm_boots(self.home.path()).iter().any(|vm| vm.pid != killed);
                (reborn && self.state(key).as_deref() == Some("ready")).then_some(())
            },
        );
    }

    fn down(&self) {
        let began = Instant::now();
        let down = self.compose_call("compose::down", json!({}), SHORT);
        let took = began.elapsed();
        assert_eq!(down["status"], "ok", "{down}");
        assert!(
            took <= STOP_TIMEOUT + DOWN_SLACK,
            "compose::down took {took:?}, past stop_timeout {STOP_TIMEOUT:?} + {DOWN_SLACK:?}"
        );
        wait_for(DOWN_SLACK, "no __vm-boot process to remain", || {
            vm_boots(self.home.path()).is_empty().then_some(())
        });
    }

    fn up_again(&mut self, key: &str) {
        let up = self.compose_call("compose::up", json!({}), LONG);
        assert_eq!(up["status"], "ok", "{up}");
        self.wait_ready("sandbox-libkrun", SHORT);
        self.wait_ready(key, SHORT);
    }

    /// What `rm` destroys: one directory per VM the adapter has booted.
    fn vm_states(&self) -> Vec<std::ffi::OsString> {
        std::fs::read_dir(self.home.path().join(".iii/sandbox/workers"))
            .map(|entries| entries.map(|entry| entry.unwrap().file_name()).collect())
            .unwrap_or_default()
    }

    /// Removes the project's only VM container. Its VM state goes, and so does
    /// the generated adapter, because its last VM went (D11).
    fn remove_last_vm(&self, key: &str) {
        // Proves HOME reached the adapter and this is the directory it uses.
        assert!(
            !self.vm_states().is_empty(),
            "the adapter kept no VM state under $HOME/.iii/sandbox/workers before rm"
        );
        let accepted = self.compose_call("compose::remove", json!({ "worker": key }), LONG);
        let operation_id = accepted["operation_id"]
            .as_str()
            .unwrap_or_else(|| panic!("compose::remove returned no operation: {accepted}"))
            .to_owned();
        let finished = wait_for(LONG, "compose::remove to finish", || {
            let operation = self.compose_call(
                "compose::operation",
                json!({ "operation_id": operation_id }),
                SHORT,
            );
            (operation["status"] != "running").then_some(operation)
        });
        assert_eq!(finished["status"], "succeeded", "{finished}");

        let left = self.vm_states();
        assert!(left.is_empty(), "rm left VM state behind: {left:?}");
        let file = std::fs::read_to_string(&self.file).unwrap();
        assert!(
            !file.contains("sandbox-libkrun"),
            "the generated adapter outlived its last VM:\n{file}"
        );
    }

    fn shutdown(&mut self) {
        let pid = self.compose.as_ref().expect("compose is running").id();
        kill(Pid::from_raw(pid as i32), Signal::SIGTERM).expect("SIGTERM compose");
        let status = wait_for(
            Duration::from_secs(60),
            "compose to exit after SIGTERM",
            || self.compose.as_mut().and_then(|c| c.try_wait().unwrap()),
        );
        self.compose = None;
        assert!(status.success(), "compose exited with {status}");
    }

    fn compose_log_tail(&self) -> String {
        let log = std::fs::read_to_string(self.home.path().join("compose.log")).unwrap_or_default();
        let lines: Vec<&str> = log.lines().collect();
        lines[lines.len().saturating_sub(80)..].join("\n")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("--- compose output (tail) ---\n{}", self.compose_log_tail());
        }
        if let Some(mut compose) = self.compose.take() {
            let _ = kill(Pid::from_raw(compose.id() as i32), Signal::SIGTERM);
            let deadline = Instant::now() + Duration::from_secs(30);
            while compose.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = compose.kill();
            let _ = compose.wait();
        }
        // Never leave a microVM behind on a runner or a developer machine.
        for vm in vm_boots(self.home.path()) {
            let _ = kill(Pid::from_raw(vm.pid as i32), Signal::SIGKILL);
        }
    }
}

/// `sandbox::create`, `sandbox::exec` and `sandbox::stop` through the engine's
/// router, which forwards them to `sandbox-libkrun::*` in the owner's namespace.
fn sandbox_exec_through_the_router(project: &Project) {
    let created = project
        .trigger(None, "sandbox::create", json!({ "image": "python" }), LONG)
        .expect("sandbox::create through the router");
    let sandbox_id = created["sandbox_id"]
        .as_str()
        .unwrap_or_else(|| panic!("sandbox::create returned no sandbox_id: {created}"))
        .to_owned();

    let exec = project
        .trigger(
            None,
            "sandbox::exec",
            json!({ "sandbox_id": sandbox_id, "cmd": "python3", "args": ["-c", "print(1)"] }),
            LONG,
        )
        .expect("sandbox::exec through the router");
    assert_eq!(exec["exit_code"], 0, "{exec}");
    assert_eq!(exec["stdout"], "1\n", "{exec}");

    project
        .trigger(
            None,
            "sandbox::stop",
            json!({ "sandbox_id": sandbox_id, "wait": true }),
            LONG,
        )
        .expect("sandbox::stop through the router");
}

#[test]
#[ignore = "boots microVMs; run by compose-sandbox-e2e-linux"]
#[serial_test::serial(sandbox_vm)]
fn a_registry_bundle_runs_through_sandbox_libkrun() {
    let mut project = Project::new(&format!(
        "  onboarding:\n    worker: package://onboarding\n    version: {ONBOARDING}\n    restart: always\n"
    ));
    let declared = std::fs::read(&project.file).unwrap();

    project.start();
    project.wait_ready("sandbox-libkrun", FIRST_UP);
    project.wait_ready("onboarding", FIRST_UP);
    // Compose takes the generated adapter as its own and does not re-resolve it.
    assert_eq!(
        std::fs::read(&project.file).unwrap(),
        declared,
        "up rewrote a file that already declares the adapter"
    );

    let vm = project.only_vm();
    #[cfg(target_os = "linux")]
    assert_vm_belongs_to_the_adapter(&vm, project.compose.as_ref().unwrap().id());
    let bundle_vm = vm.pid;

    wait_for(SHORT, "an onboarding log entry", || {
        let logs = project.compose_call(
            "compose::logs",
            json!({ "container": "onboarding", "tail": 50, "wait_ms": 2000 }),
            SHORT,
        );
        let entries = logs["containers"][0]["entries"].as_array()?;
        (!entries.is_empty()).then_some(())
    });

    sandbox_exec_through_the_router(&project);

    sigkill(bundle_vm);
    project.recovers_after_kill("onboarding", bundle_vm);

    project.down();
    project.up_again("onboarding");
    project.remove_last_vm("onboarding");
    project.shutdown();
}

#[test]
#[ignore = "boots microVMs; run by compose-sandbox-e2e-linux"]
#[serial_test::serial(sandbox_vm)]
fn a_local_base_image_worker_runs_and_syncs_its_source() {
    let mut project = Project::new(
        "  sandbox-vm-worker:\n    worker: path://./sandbox-vm-worker\n    restart: always\n",
    );
    let worker_dir = project.dir.path().join("sandbox-vm-worker");
    copy_fixture(&worker_dir);
    let reply = worker_dir.join("reply.txt");

    // The guest registers `sandbox-vm-worker::hello` a moment after the worker
    // does, and a restart replaces the text only once the new VM is up.
    let expect_reply = |project: &Project, expected: &str| {
        let mut seen = Value::Null;
        let deadline = Instant::now() + SHORT;
        while Instant::now() < deadline {
            match project.trigger(
                Some(NAMESPACE),
                "sandbox-vm-worker::hello",
                json!({}),
                SHORT,
            ) {
                Ok(answer) if answer == expected => return,
                Ok(answer) => seen = answer,
                Err(error) => seen = Value::String(error),
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        panic!("sandbox-vm-worker::hello never returned {expected:?}; last answer {seen}");
    };

    project.start();
    project.wait_ready("sandbox-libkrun", FIRST_UP);
    project.wait_ready("sandbox-vm-worker", FIRST_UP);
    expect_reply(&project, &std::fs::read_to_string(&reply).unwrap());

    // The restart syncs the host copy of the source into the VM again.
    let rewritten = "hello from the second sync";
    std::fs::write(&reply, rewritten).unwrap();
    let restarted = project.compose_call(
        "compose::restart",
        json!({ "worker": "sandbox-vm-worker" }),
        LONG,
    );
    assert_eq!(restarted["status"], "ok", "{restarted}");
    expect_reply(&project, rewritten);

    let worker_vm = project.only_vm().pid;
    sigkill(worker_vm);
    project.recovers_after_kill("sandbox-vm-worker", worker_vm);

    project.down();
    project.up_again("sandbox-vm-worker");
    project.remove_last_vm("sandbox-vm-worker");
    project.shutdown();
}
