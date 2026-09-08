//! Process labels as observed by Linux, through the real iii CLI.

#![cfg(target_os = "linux")]

use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use nix::{sys::signal::Signal, unistd::Pid};

struct Process {
    child: Child,
    log: std::path::PathBuf,
}

impl Process {
    fn start(dir: &Path, args: &[&str]) -> Self {
        let log = dir.join("process.log");
        let output = fs::File::create(&log).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_iii"))
            .current_dir(dir)
            .args(args)
            .env("III_TELEMETRY_ENABLED", "false")
            .env("III_COMPOSE_STATE_DIR", dir.join("state"))
            .env("TOKIO_WORKER_THREADS", "2")
            .env("NO_COLOR", "1")
            .env_remove("III_URL")
            .stdin(Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .spawn()
            .unwrap();
        Self { child, log }
    }

    fn wait_until(&mut self, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if ready() {
                return;
            }
            assert!(
                self.child.try_wait().unwrap().is_none() && Instant::now() < deadline,
                "process did not become ready:\n{}",
                fs::read_to_string(&self.log).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn wait_for_compose(&mut self) {
        let log = self.log.clone();
        self.wait_until(|| {
            fs::read_to_string(&log)
                .unwrap_or_default()
                .contains("compose serving")
        });
    }

    fn stop(&mut self, signal: Signal) {
        nix::sys::signal::kill(Pid::from_raw(self.child.id() as i32), signal).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "process failed during shutdown:\n{}",
                    fs::read_to_string(&self.log).unwrap_or_default()
                );
                return;
            }
            assert!(Instant::now() < deadline, "process did not stop");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = nix::sys::signal::kill(Pid::from_raw(self.child.id() as i32), Signal::SIGTERM);
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn unused_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn comm(pid: u32) -> String {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .unwrap()
        .trim_end()
        .to_string()
}

fn cmdline(pid: u32) -> Vec<String> {
    fs::read(format!("/proc/{pid}/cmdline"))
        .unwrap()
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8(arg.to_vec()).unwrap())
        .collect()
}

#[test]
fn compose_instances_show_resolved_namespaces_in_both_process_fields() {
    let engine_dir = tempfile::tempdir().unwrap();
    let port = unused_port();
    let address = format!("ws://127.0.0.1:{port}");
    fs::write(
        engine_dir.path().join("config.yaml"),
        format!(
            "workers:\n  - name: iii-worker-manager\n    config:\n      host: 127.0.0.1\n      port: {port}\n"
        ),
    )
    .unwrap();
    let mut engine = Process::start(engine_dir.path(), &["--no-update-check"]);
    engine.wait_until(|| TcpStream::connect(("127.0.0.1", port)).is_ok());
    let engine_name = comm(engine.child.id());
    let engine_args = cmdline(engine.child.id());

    let mut daemons = Vec::new();
    for (namespace, explicit) in [
        ("orders", false),
        ("billing", true),
        ("production-orders", false),
        ("production-billing", true),
        ("default", false),
    ] {
        let dir = tempfile::Builder::new()
            .prefix("compose names ")
            .tempdir()
            .unwrap();
        if namespace != "default" {
            let declared = if explicit { "ignored" } else { namespace };
            fs::write(
                dir.path().join("worker-compose.yaml"),
                format!("namespace: {declared}\ncontainers:\n  api:\n    worker: path://./api\n"),
            )
            .unwrap();
        }
        let mut args = vec!["compose", "--engine", &address];
        if explicit {
            args.extend(["--namespace", namespace]);
        }
        let mut daemon = Process::start(dir.path(), &args);
        let pid = daemon.child.id();
        daemon.wait_for_compose();
        let command = cmdline(pid);
        assert_eq!(command[0], format!("iii:c:{namespace}"));
        assert_eq!(command[1..], args);
        let name = comm(pid);
        if namespace.starts_with("production-") {
            assert_eq!(name.len(), 15);
            assert!(name.starts_with("iii:c:pr~"));
        } else {
            assert_eq!(name, format!("iii:c:{namespace}"));
        }
        daemons.push((dir, daemon));
    }

    // All five daemons are alive together on one engine, with distinct labels.
    let names: std::collections::HashSet<_> = daemons
        .iter()
        .map(|(_, daemon)| comm(daemon.child.id()))
        .collect();
    assert_eq!(names.len(), daemons.len());
    for (_, daemon) in &mut daemons {
        daemon.stop(Signal::SIGTERM);
    }
    assert!(engine.child.try_wait().unwrap().is_none());
    assert_eq!(comm(engine.child.id()), engine_name);
    assert_eq!(cmdline(engine.child.id()), engine_args);
}

#[test]
fn managed_engine_has_its_own_role_and_stops_with_the_named_compose() {
    let dir = tempfile::Builder::new()
        .prefix("managed names ")
        .tempdir()
        .unwrap();
    let port = unused_port();
    fs::write(
        dir.path().join("worker-compose.yaml"),
        format!(
            "namespace: ignored\nengine:\n  url: ws://127.0.0.1:{port}\n  workers:\n    iii-worker-manager:\n      host: 127.0.0.1\n      port: {port}\ncontainers: {{}}\n"
        ),
    )
    .unwrap();
    let mut daemon = Process::start(dir.path(), &["compose", "--namespace", "orders", "--up"]);
    let pid = daemon.child.id();
    daemon.wait_for_compose();
    assert_eq!(comm(pid), "iii:c:orders");
    assert_eq!(cmdline(pid)[0], "iii:c:orders");

    let children = fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap();
    let engine_pid: u32 = children.split_whitespace().next().unwrap().parse().unwrap();
    assert_eq!(comm(engine_pid), "iii:e:orders");
    assert_eq!(
        cmdline(engine_pid),
        [
            "iii:e:orders".to_string(),
            "--config".to_string(),
            dir.path()
                .join("state/orders/engine-config.yaml")
                .display()
                .to_string(),
        ]
    );

    daemon.stop(Signal::SIGINT);
    assert!(!Path::new(&format!("/proc/{engine_pid}")).exists());
    TcpListener::bind(("127.0.0.1", port)).expect("managed engine released its port");
}
