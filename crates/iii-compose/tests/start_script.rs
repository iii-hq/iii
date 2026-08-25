//! Failure cleanup for the CI engine launcher.

#![cfg(unix)]

use std::{path::Path, process::Command};

fn write_executable(path: &Path, contents: &str) {
    use std::{io::Write as _, os::unix::fs::PermissionsExt};

    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    drop(file);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn timeout_stops_the_started_process_and_removes_its_pid_file() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let start_script = root.join("scripts/start-iii.sh");
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("fake-iii");
    let config = tmp.path().join("worker-compose.yaml");
    let pid_file = tmp.path().join("launcher.pid");
    let child_pid_file = tmp.path().join("child.pid");
    let stopped_file = tmp.path().join("stopped");
    let log_file = tmp.path().join("engine.log");

    write_executable(
        &binary,
        "#!/bin/sh\ntrap 'printf stopped > \"$FAKE_STOP_FILE\"; exit 0' TERM INT\nprintf '%s' \"$$\" > \"$FAKE_PID_FILE\"\nwhile :; do sleep 1; done\n",
    );
    std::fs::write(&config, "engine: { workers: {} }\ncontainers: {}\n").unwrap();

    let status = Command::new("bash")
        .arg(start_script)
        .args(["--binary", binary.to_str().unwrap()])
        .args(["--config", config.to_str().unwrap()])
        .args(["--port", "65500"])
        .args(["--pid-file", pid_file.to_str().unwrap()])
        .args(["--log-file", log_file.to_str().unwrap()])
        .args(["--timeout", "1"])
        .env("FAKE_PID_FILE", &child_pid_file)
        .env("FAKE_STOP_FILE", &stopped_file)
        .status()
        .unwrap();

    assert!(!status.success(), "the fake engine never becomes ready");
    assert!(
        stopped_file.exists(),
        "the timeout did not terminate the started process"
    );
    assert!(
        !pid_file.exists(),
        "the timeout left a stale launcher pid file"
    );

    let child_pid = std::fs::read_to_string(child_pid_file)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    assert!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(child_pid), None).is_err(),
        "started process {child_pid} survived the timeout"
    );
}

#[test]
fn timeout_force_kills_a_process_that_ignores_sigterm() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let start_script = root.join("scripts/start-iii.sh");
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("stubborn-iii");
    let config = tmp.path().join("worker-compose.yaml");
    let pid_file = tmp.path().join("launcher.pid");
    let child_pid_file = tmp.path().join("child.pid");
    let log_file = tmp.path().join("engine.log");

    write_executable(
        &binary,
        "#!/usr/bin/env bash\ntrap '' TERM\nprintf '%s' \"$$\" > \"$FAKE_PID_FILE\"\nwhile :; do :; done\n",
    );
    std::fs::write(&config, "engine: { workers: {} }\ncontainers: {}\n").unwrap();

    let status = Command::new("bash")
        .arg(start_script)
        .args(["--binary", binary.to_str().unwrap()])
        .args(["--config", config.to_str().unwrap()])
        .args(["--port", "65500"])
        .args(["--pid-file", pid_file.to_str().unwrap()])
        .args(["--log-file", log_file.to_str().unwrap()])
        .args(["--timeout", "1"])
        .env("FAKE_PID_FILE", &child_pid_file)
        .env("III_START_CLEANUP_GRACE_SECONDS", "1")
        .status()
        .unwrap();

    assert!(!status.success(), "the fake engine never becomes ready");
    assert!(
        !pid_file.exists(),
        "forced cleanup left a stale launcher pid file"
    );

    let child_pid = std::fs::read_to_string(child_pid_file)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    assert!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(child_pid), None).is_err(),
        "SIGTERM-ignoring process {child_pid} survived SIGKILL escalation"
    );
}
