// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! The sandbox daemon must follow the engine lifecycle even though the legacy
//! worker-manager daemon no longer exists.

#![cfg(unix)]

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const EXIT_DEADLINE: Duration = Duration::from_secs(15);
const ARMED_SURVIVAL_WINDOW: Duration = Duration::from_millis(5000);

struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_log_line(logfile: &Path, needle: &str, deadline: Duration) -> (bool, String) {
    let end = Instant::now() + deadline;
    loop {
        let contents = std::fs::read_to_string(logfile).unwrap_or_default();
        if contents.contains(needle) {
            return (true, contents);
        }
        if Instant::now() >= end {
            return (false, contents);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_exit(
    child: &mut KillOnDrop,
    deadline: Duration,
    logfile: &Path,
) -> std::process::ExitStatus {
    let end = Instant::now() + deadline;
    loop {
        if let Some(status) = child.0.try_wait().expect("try_wait") {
            return status;
        }
        if Instant::now() >= end {
            let log = std::fs::read_to_string(logfile).unwrap_or_default();
            panic!("sandbox-daemon did not exit within {deadline:?}; log:\n{log}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn spawn_fake_engine() -> std::process::Child {
    Command::new("sleep")
        .arg("300")
        .spawn()
        .expect("spawn fake engine")
}

#[test]
fn sandbox_daemon_exits_when_engine_dies() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("sandbox-daemon.out");
    let config = tmp.path().join("sandbox-config.yaml");
    std::fs::write(&config, "image_allowlist: []\n").unwrap();

    let mut fake_engine = KillOnDrop(spawn_fake_engine());
    let engine_pid = fake_engine.0.id() as i32;

    let log = std::fs::File::create(&logfile).unwrap();
    let log_err = log.try_clone().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_iii-worker"));
    cmd.arg("sandbox-daemon")
        .arg("--config")
        .arg(&config)
        .args(["--engine", "ws://127.0.0.1:1"])
        .current_dir(tmp.path())
        .env("RUST_LOG", "info")
        .env("HOME", tmp.path())
        .env("III_ENGINE_PID", engine_pid.to_string())
        .env_remove("III_LIFELINE_FD")
        .env_remove("III_LIFELINE_SPAWNER_PID")
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));
    let mut daemon = KillOnDrop(cmd.spawn().expect("spawn sandbox-daemon"));

    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    assert!(
        armed,
        "sandbox-daemon never armed the engine watch; log:\n{log}"
    );

    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        daemon.0.try_wait().expect("try_wait").is_none(),
        "sandbox-daemon exited while its engine was alive"
    );

    let _ = fake_engine.0.kill();
    let _ = fake_engine.0.wait();

    let status = wait_for_exit(&mut daemon, EXIT_DEADLINE, &logfile);
    assert_eq!(status.code(), Some(0), "engine-gone exit must be graceful");

    let breadcrumb = tmp.path().join(".iii/logs/sandbox-daemon.log");
    let crumb = std::fs::read_to_string(breadcrumb)
        .expect("sandbox-daemon engine-gone exit must write a breadcrumb");
    assert!(
        crumb.contains("daemon=sandbox-daemon") && crumb.contains("reason=engine-gone"),
        "breadcrumb must identify the sandbox daemon and reason: {crumb}"
    );
}
