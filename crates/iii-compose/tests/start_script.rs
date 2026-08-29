//! Failure cleanup for the CI engine launcher.

#![cfg(unix)]

use std::{
    path::Path,
    process::Command,
    sync::{Mutex, MutexGuard},
};

static LAUNCHER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn launcher_test_guard() -> MutexGuard<'static, ()> {
    LAUNCHER_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write_executable(path: &Path, contents: &str) {
    use std::{io::Write as _, os::unix::fs::PermissionsExt};

    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    drop(file);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn repository_compose_fixtures_use_the_strict_stack_contract() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for relative in [
        "sdk/fixtures/config-test.worker-compose.yaml",
        "sdk/fixtures/config-bridge.worker-compose.yaml",
        "sdk/fixtures/config-bridge-backend.worker-compose.yaml",
        "engine/config.prod.worker-compose.yaml",
        "engine/worker-compose.remote-kv.yaml",
    ] {
        iii_compose::config::ComposeFile::load(root.join(relative))
            .unwrap_or_else(|error| panic!("{relative}: {error}"));
    }
}

#[test]
fn timeout_stops_the_started_process_and_removes_its_pid_file() {
    let _guard = launcher_test_guard();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let start_script = root.join("scripts/start-iii.sh");
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("fake-iii");
    let config = tmp.path().join("config.yaml");
    let pid_file = tmp.path().join("launcher.pid");
    let child_pid_file = tmp.path().join("child.pid");
    let stopped_file = tmp.path().join("stopped");
    let log_file = tmp.path().join("engine.log");

    write_executable(
        &binary,
        "#!/bin/sh\ntrap 'printf stopped > \"$FAKE_STOP_FILE\"; exit 0' TERM INT\nprintf '%s' \"$$\" > \"$FAKE_PID_FILE\"\nwhile :; do sleep 1; done\n",
    );
    std::fs::write(&config, "workers: []\n").unwrap();

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
    let _guard = launcher_test_guard();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let start_script = root.join("scripts/start-iii.sh");
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("stubborn-iii");
    let config = tmp.path().join("config.yaml");
    let pid_file = tmp.path().join("launcher.pid");
    let child_pid_file = tmp.path().join("child.pid");
    let log_file = tmp.path().join("engine.log");

    write_executable(
        &binary,
        "#!/usr/bin/env bash\ntrap '' TERM\nprintf '%s' \"$$\" > \"$FAKE_PID_FILE\"\nwhile :; do :; done\n",
    );
    std::fs::write(&config, "workers: []\n").unwrap();

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

#[test]
fn explicit_engine_is_forwarded_to_compose() {
    let _guard = launcher_test_guard();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let start_script = root.join("scripts/start-iii.sh");
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("fake-iii");
    let config = tmp.path().join("config.yaml");
    let compose = tmp.path().join("worker-compose.yaml");
    let pid_file = tmp.path().join("launcher.pid");
    let args_file = tmp.path().join("args");
    let log_file = tmp.path().join("engine.log");

    write_executable(
        &binary,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$FAKE_ARGS_FILE\"\nprintf 'up: 1 of 1 changed\\n'\nwhile :; do sleep 1; done\n",
    );
    std::fs::write(&config, "workers: []\n").unwrap();
    std::fs::write(
        &compose,
        "workers: {}\nstacks:\n  default:\n    namespace: test\n    containers: {}\n",
    )
    .unwrap();

    let status = Command::new("bash")
        .arg(start_script)
        .args(["--binary", binary.to_str().unwrap()])
        .args(["--config", config.to_str().unwrap()])
        .args(["--compose-file", compose.to_str().unwrap()])
        .args(["--port", "49134"])
        .args(["--pid-file", pid_file.to_str().unwrap()])
        .args(["--log-file", log_file.to_str().unwrap()])
        .args(["--timeout", "3"])
        .args(["--engine", "ws://127.0.0.1:49200"])
        .env("FAKE_ARGS_FILE", &args_file)
        .status()
        .unwrap();

    assert!(status.success());
    let args = std::fs::read_to_string(args_file).unwrap();
    assert!(args.contains("--engine\nws://127.0.0.1:49200\n"));

    let child_pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child_pid),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    std::fs::remove_file(pid_file).unwrap();
}

#[test]
fn engine_config_and_compose_are_started_as_separate_processes() {
    let _guard = launcher_test_guard();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let start_script = root.join("scripts/start-iii.sh");
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("fake-iii");
    let config = tmp.path().join("config.yaml");
    let compose = tmp.path().join("worker-compose.yaml");
    let pid_file = tmp.path().join("launcher.pid");
    let engine_args_file = tmp.path().join("engine-args");
    let compose_args_file = tmp.path().join("compose-args");
    let log_file = tmp.path().join("engine.log");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();

    write_executable(
        &binary,
        "#!/bin/sh\nif [ \"${1:-}\" = compose ]; then\n  printf '%s\\n' \"$@\" > \"$FAKE_COMPOSE_ARGS_FILE\"\n  printf 'up: 1 of 1 changed\\n'\nelse\n  printf '%s\\n' \"$@\" > \"$FAKE_ENGINE_ARGS_FILE\"\nfi\nwhile :; do sleep 1; done\n",
    );
    std::fs::write(&config, "workers: []\n").unwrap();
    std::fs::write(
        &compose,
        "workers: {}\nstacks:\n  default:\n    namespace: test\n    containers: {}\n",
    )
    .unwrap();

    let status = Command::new("bash")
        .arg(start_script)
        .args(["--binary", binary.to_str().unwrap()])
        .args(["--config", config.to_str().unwrap()])
        .args(["--compose-file", compose.to_str().unwrap()])
        .args(["--port", &port])
        .args(["--pid-file", pid_file.to_str().unwrap()])
        .args(["--log-file", log_file.to_str().unwrap()])
        .args(["--timeout", "3"])
        .env("FAKE_ENGINE_ARGS_FILE", &engine_args_file)
        .env("FAKE_COMPOSE_ARGS_FILE", &compose_args_file)
        .status()
        .unwrap();

    assert!(status.success());
    let engine_args = std::fs::read_to_string(engine_args_file).unwrap();
    assert!(engine_args.contains("--config\n"));
    assert!(engine_args.contains(config.to_str().unwrap()));
    let compose_args = std::fs::read_to_string(compose_args_file).unwrap();
    assert!(compose_args.contains(&format!("--engine\nws://127.0.0.1:{port}\n")));
    assert!(compose_args.contains("--up\n--file\n"));

    let engine_pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let compose_pid_file = std::path::PathBuf::from(format!("{}.compose.pid", pid_file.display()));
    let compose_pid = std::fs::read_to_string(&compose_pid_file)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let stop_script = root.join("scripts/stop-iii.sh");
    assert!(
        Command::new("bash")
            .arg(stop_script)
            .arg(&pid_file)
            .status()
            .unwrap()
            .success()
    );
    assert!(!pid_file.exists());
    assert!(!compose_pid_file.exists());
    for pid in [engine_pid, compose_pid] {
        for _ in 0..20 {
            if nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err(),
            "launcher process {pid} survived stop-iii.sh"
        );
    }
}
