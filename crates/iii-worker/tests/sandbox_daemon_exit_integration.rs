// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! The sandbox daemon must follow the engine lifecycle even though the legacy
//! worker-manager daemon no longer exists.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

const EXIT_DEADLINE: Duration = Duration::from_secs(15);
const ARMED_SURVIVAL_WINDOW: Duration = Duration::from_millis(2500);

struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn pid_alive(pid: i32) -> bool {
    kill(Pid::from_raw(pid), None).is_ok()
}

fn pid_is_iii_worker(pid: i32) -> bool {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains("iii-worker"))
        .unwrap_or(false)
}

struct OrphanGuard {
    shell: std::process::Child,
    daemon_pid: Option<i32>,
}

impl Drop for OrphanGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.daemon_pid
            && pid > 1
            && pid_is_iii_worker(pid)
        {
            let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
        }
        let _ = self.shell.kill();
        let _ = self.shell.wait();
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

fn sandbox_command(tmp: &Path, logfile: &Path) -> Command {
    let config = tmp.join("sandbox-config.yaml");
    std::fs::write(&config, "image_allowlist: []\n").unwrap();

    let log = std::fs::File::create(logfile).unwrap();
    let log_err = log.try_clone().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_iii-worker"));
    cmd.arg("sandbox-daemon")
        .arg("--config")
        .arg(config)
        .args(["--engine", "ws://127.0.0.1:1"])
        .current_dir(tmp)
        .env("RUST_LOG", "info")
        .env("HOME", tmp)
        .env_remove("III_ENGINE_PID")
        .env_remove("III_LIFELINE_FD")
        .env_remove("III_LIFELINE_SPAWNER_PID")
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));
    cmd
}

fn breadcrumb_path(home: &Path) -> PathBuf {
    home.join(".iii/logs/sandbox-daemon.log")
}

fn wait_for_pidfile(path: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(contents) = std::fs::read_to_string(path)
            && let Ok(pid) = contents.trim().parse::<i32>()
            && pid > 1
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "daemon never wrote its pid");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn sandbox_daemon_exits_when_engine_dies() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("sandbox-daemon.out");

    let mut fake_engine = KillOnDrop(spawn_fake_engine());
    let engine_pid = fake_engine.0.id() as i32;

    let mut cmd = sandbox_command(tmp.path(), &logfile);
    cmd.env("III_ENGINE_PID", engine_pid.to_string());
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

    let crumb = std::fs::read_to_string(breadcrumb_path(tmp.path()))
        .expect("sandbox-daemon engine-gone exit must write a breadcrumb");
    assert!(
        crumb.contains("daemon=sandbox-daemon") && crumb.contains("reason=engine-gone"),
        "breadcrumb must identify the sandbox daemon and reason: {crumb}"
    );
    assert!(
        crumb.contains(&format!("engine_pid={engine_pid} ")),
        "breadcrumb must record the watched engine pid: {crumb}"
    );
    assert!(
        crumb.contains(&format!("spawn_parent={}\n", std::process::id())),
        "breadcrumb must record the spawn-time parent: {crumb}"
    );
}

#[test]
fn sandbox_signal_exit_writes_no_engine_gone_breadcrumb() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("sandbox-daemon.out");
    let mut daemon = KillOnDrop(
        sandbox_command(tmp.path(), &logfile)
            .spawn()
            .expect("spawn sandbox-daemon"),
    );

    let (armed, log) =
        wait_for_log_line(&logfile, "parent exit-watch armed", Duration::from_secs(10));
    assert!(
        armed,
        "sandbox-daemon never armed its exit watch; log:\n{log}"
    );

    kill(Pid::from_raw(daemon.0.id() as i32), Signal::SIGTERM).expect("send SIGTERM");
    let status = wait_for_exit(&mut daemon, EXIT_DEADLINE, &logfile);
    assert_eq!(status.code(), Some(0), "SIGTERM exit must be graceful");

    let breadcrumb = breadcrumb_path(tmp.path());
    assert!(
        !breadcrumb.exists(),
        "signal shutdown must not write an engine-gone breadcrumb: {}",
        std::fs::read_to_string(&breadcrumb).unwrap_or_default()
    );
}

#[test]
fn sandbox_lifeline_eof_wins_while_the_declared_engine_is_alive() {
    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("sandbox-daemon.out");
    let fake_engine = KillOnDrop(spawn_fake_engine());
    let engine_pid = fake_engine.0.id() as i32;

    let mut cmd = sandbox_command(tmp.path(), &logfile);
    cmd.env("III_ENGINE_PID", engine_pid.to_string());
    let lifeline = iii_worker::daemon_exit::attach_lifeline_std(&mut cmd).expect("attach lifeline");
    let mut daemon = KillOnDrop(cmd.spawn().expect("spawn sandbox-daemon"));

    let (armed, log) = wait_for_log_line(
        &logfile,
        "lifeline exit-watch armed",
        Duration::from_secs(10),
    );
    assert!(
        armed,
        "sandbox-daemon never armed the lifeline; log:\n{log}"
    );

    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        daemon.0.try_wait().expect("try_wait").is_none(),
        "sandbox-daemon exited while both its lifeline and engine were alive"
    );

    drop(lifeline);
    let status = wait_for_exit(&mut daemon, Duration::from_secs(5), &logfile);
    assert_eq!(status.code(), Some(0), "lifeline exit must be graceful");

    let crumb = std::fs::read_to_string(breadcrumb_path(tmp.path()))
        .expect("lifeline exit must write an engine-gone breadcrumb");
    assert!(
        crumb.contains(&format!("engine_pid={engine_pid} ")),
        "breadcrumb must preserve the still-live declared engine pid: {crumb}"
    );
}

#[test]
fn sandbox_survives_orphaning_while_its_declared_engine_is_alive() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("sandbox-config.yaml");
    let pidfile = tmp.path().join("sandbox-daemon.pid");
    let logfile = tmp.path().join("sandbox-daemon.out");
    std::fs::write(&config, "image_allowlist: []\n").unwrap();

    let shell = Command::new("sh")
        .arg("-c")
        .arg(
            r#""$DAEMON_BIN" sandbox-daemon --config "$SANDBOX_CONFIG" --engine ws://127.0.0.1:1 >>"$DAEMON_LOG" 2>&1 & echo $! > "$DAEMON_PIDFILE"; wait"#,
        )
        .current_dir(tmp.path())
        .env("DAEMON_BIN", env!("CARGO_BIN_EXE_iii-worker"))
        .env("SANDBOX_CONFIG", &config)
        .env("DAEMON_PIDFILE", &pidfile)
        .env("DAEMON_LOG", &logfile)
        .env("RUST_LOG", "info")
        .env("HOME", tmp.path())
        .env("III_ENGINE_PID", std::process::id().to_string())
        .env_remove("III_LIFELINE_FD")
        .env_remove("III_LIFELINE_SPAWNER_PID")
        .spawn()
        .expect("spawn intermediate shell");
    let mut guard = OrphanGuard {
        shell,
        daemon_pid: None,
    };
    let daemon_pid = wait_for_pidfile(&pidfile);
    guard.daemon_pid = Some(daemon_pid);

    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    assert!(
        armed,
        "sandbox-daemon never armed the engine watch; log:\n{log}"
    );

    let _ = guard.shell.kill();
    let _ = guard.shell.wait();
    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        pid_alive(daemon_pid),
        "sandbox-daemon exited on parent loss while its declared engine was alive"
    );
}

#[test]
fn sandbox_spawner_pid_backstop_covers_a_leaked_lifeline_writer() {
    use std::os::fd::AsRawFd;

    let tmp = tempfile::tempdir().unwrap();
    let logfile = tmp.path().join("sandbox-daemon.out");
    let mut fake_spawner = KillOnDrop(spawn_fake_engine());
    let spawner_pid = fake_spawner.0.id() as i32;
    let (read_end, _write_end_leak) = nix::unistd::pipe().expect("pipe");

    let mut cmd = sandbox_command(tmp.path(), &logfile);
    cmd.env("III_LIFELINE_FD", read_end.as_raw_fd().to_string())
        .env("III_LIFELINE_SPAWNER_PID", spawner_pid.to_string());
    let mut daemon = KillOnDrop(cmd.spawn().expect("spawn sandbox-daemon"));

    let (armed, log) = wait_for_log_line(
        &logfile,
        "lifeline exit-watch armed",
        Duration::from_secs(10),
    );
    assert!(
        armed,
        "sandbox-daemon never armed the lifeline; log:\n{log}"
    );
    let (armed, log) =
        wait_for_log_line(&logfile, "engine exit-watch armed", Duration::from_secs(10));
    assert!(
        armed,
        "sandbox-daemon never armed the pid backstop; log:\n{log}"
    );

    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        daemon.0.try_wait().expect("try_wait").is_none(),
        "sandbox-daemon exited while its declared spawner was alive"
    );

    let _ = fake_spawner.0.kill();
    let _ = fake_spawner.0.wait();
    let status = wait_for_exit(&mut daemon, EXIT_DEADLINE, &logfile);
    assert_eq!(status.code(), Some(0), "backstop exit must be graceful");

    let crumb = std::fs::read_to_string(breadcrumb_path(tmp.path()))
        .expect("backstop exit must write an engine-gone breadcrumb");
    assert!(
        crumb.contains("engine_pid=none "),
        "breadcrumb must show that no engine pid was declared: {crumb}"
    );
}

#[test]
fn watch_source_exits_when_its_declared_engine_dies() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let mut fake_engine = KillOnDrop(spawn_fake_engine());
    let engine_pid = fake_engine.0.id() as i32;

    let logfile = tmp.path().join("watch-source.out");
    let log = std::fs::File::create(&logfile).unwrap();
    let log_err = log.try_clone().unwrap();
    let mut watcher = KillOnDrop(
        Command::new(env!("CARGO_BIN_EXE_iii-worker"))
            .args(["__watch-source", "--worker", "test-worker", "--project"])
            .arg(&project)
            .env("RUST_LOG", "info")
            .env("HOME", tmp.path())
            .env("III_ENGINE_PID", engine_pid.to_string())
            .stdout(std::process::Stdio::from(log))
            .stderr(std::process::Stdio::from(log_err))
            .spawn()
            .expect("spawn watch-source"),
    );

    std::thread::sleep(ARMED_SURVIVAL_WINDOW);
    assert!(
        watcher.0.try_wait().expect("try_wait").is_none(),
        "watch-source exited while its engine was alive: {}",
        std::fs::read_to_string(&logfile).unwrap_or_default()
    );

    let _ = fake_engine.0.kill();
    let _ = fake_engine.0.wait();
    let end = Instant::now() + EXIT_DEADLINE;
    let status = loop {
        if let Some(status) = watcher.0.try_wait().expect("try_wait") {
            break status;
        }
        if Instant::now() >= end {
            panic!(
                "watch-source ignored engine death; log:\n{}",
                std::fs::read_to_string(&logfile).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(status.code(), Some(0), "watch-source exit must be graceful");
}
