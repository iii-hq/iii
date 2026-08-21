//! Managed-engine lifecycle exercised through the installed CLI shape.

use std::{net::TcpListener, process::Command};

#[cfg(unix)]
use std::{process::Stdio, time::Duration, time::Instant};

fn iii_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iii"))
}

#[cfg(unix)]
fn shell_quote(value: &std::path::Path) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "'\"'\"'"))
}

#[cfg(unix)]
fn wait_for_file(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[cfg(unix)]
#[test]
#[ignore]
fn managed_worker_fixture() {
    let ready = std::env::var_os("READY_MARKER").expect("READY_MARKER");
    let client = iii_sdk::register_worker_from_env(iii_sdk::InitOptions::default());
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if matches!(
            client.get_connection_state(),
            iii_sdk::runtime::IIIConnectionState::Connected
        ) {
            std::fs::write(ready, "ready").unwrap();
            loop {
                std::thread::park_timeout(Duration::from_secs(1));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("fixture worker never connected");
}

#[test]
fn compose_up_starts_logs_and_stops_the_engine_it_owns() {
    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let config = project.path().join("custom-engine.yaml");
    std::fs::write(
        &config,
        format!(
            "workers:\n  - name: iii-worker-manager\n    config:\n      host: 127.0.0.1\n      port: {port}\nmodules: []\n"
        ),
    )
    .unwrap();
    // The project is deliberately invalid after the engine becomes ready. It
    // makes the command leave through its error cleanup path without needing
    // a language SDK fixture, and that path must still stop the owned engine.
    std::fs::write(
        project.path().join("worker-compose.yaml"),
        "namespace: managed-test\ncontainers: {}\n",
    )
    .unwrap();

    let output = iii_bin()
        .current_dir(project.path())
        .env("III_COMPOSE_STATE_DIR", state.path())
        .args([
            "--config",
            config.to_str().unwrap(),
            "compose",
            "--engine",
            &format!("ws://127.0.0.1:{port}"),
            "--namespace",
            "managed-e2e",
            "up",
        ])
        .output()
        .expect("run iii compose up");

    assert!(!output.status.success(), "invalid project must fail");
    let terminal = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        terminal.contains("engine started"),
        "unexpected output:\n{terminal}"
    );
    assert!(
        terminal.contains(config.to_str().unwrap()),
        "config not announced:\n{terminal}"
    );

    let engine_log = state.path().join("managed-e2e/engine.log");
    assert!(
        engine_log.exists(),
        "no engine log at {}",
        engine_log.display()
    );
    #[cfg(unix)]
    assert!(
        terminal.contains(&format!("tail -f '{}'", engine_log.display())),
        "copyable log command missing:\n{terminal}"
    );
    #[cfg(windows)]
    assert!(
        terminal.contains("Get-Content -LiteralPath") && terminal.contains("-Wait"),
        "copyable log command missing:\n{terminal}"
    );

    // The child had to bind this custom port for compose to reach the invalid
    // project, and cleanup must release it before the foreground CLI returns.
    TcpListener::bind(("127.0.0.1", port)).expect("managed engine should be stopped");
}

#[cfg(unix)]
#[test]
fn ctrl_c_stops_the_worker_before_the_managed_engine() {
    use std::os::unix::fs::PermissionsExt;

    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let config = project.path().join("config.yaml");
    std::fs::write(
        &config,
        format!(
            "workers:\n  - name: iii-worker-manager\n    config:\n      host: 127.0.0.1\n      port: {port}\nmodules: []\n"
        ),
    )
    .unwrap();

    let ready = project.path().join("worker.ready");
    let stopped = project.path().join("worker.stopped");
    let worker_script = project.path().join("worker.sh");
    let test_binary = std::env::current_exe().unwrap();
    std::fs::write(
        &worker_script,
        format!(
            "#!/bin/sh\nREADY_MARKER={}\nSTOPPED_MARKER={}\nTEST_BINARY={}\nexport READY_MARKER\non_stop() {{\n  kill \"$worker\" 2>/dev/null || true\n  wait \"$worker\" 2>/dev/null || true\n  printf stopped > \"$STOPPED_MARKER\"\n  exit 0\n}}\ntrap on_stop TERM INT\n\"$TEST_BINARY\" --ignored --exact managed_worker_fixture --nocapture &\nworker=$!\nwait \"$worker\"\n",
            shell_quote(&ready),
            shell_quote(&stopped),
            shell_quote(&test_binary),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&worker_script, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        project.path().join("worker-compose.yaml"),
        "namespace: managed-test\nstartup_timeout: 20s\nstop_timeout: 5s\ncontainers:\n  probe:\n    worker: path://.\n    scripts:\n      run: ./worker.sh\n",
    )
    .unwrap();

    let mut child = iii_bin()
        .current_dir(project.path())
        .env("III_COMPOSE_STATE_DIR", state.path())
        .args([
            "--config",
            config.to_str().unwrap(),
            "compose",
            "--engine",
            &format!("ws://127.0.0.1:{port}"),
            "--namespace",
            "managed-signal-e2e",
            "up",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run iii compose up");

    if !wait_for_file(&ready, Duration::from_secs(30)) {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(child.id() as i32),
            nix::sys::signal::Signal::SIGTERM,
        );
        let _ = child.wait();
        panic!("worker never became ready");
    }
    std::thread::sleep(Duration::from_millis(500));
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && child.try_wait().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(50));
    }
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("compose did not exit after SIGINT");
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "compose exited with {output:?}");
    assert!(stopped.exists(), "worker shutdown trap did not run");

    let terminal = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let workers = terminal
        .find("stopping every project...")
        .unwrap_or_else(|| panic!("worker shutdown missing:\n{terminal}"));
    let engine = terminal
        .find("stopping engine...")
        .unwrap_or_else(|| panic!("engine shutdown missing:\n{terminal}"));
    assert!(workers < engine, "shutdown order was reversed:\n{terminal}");
    TcpListener::bind(("127.0.0.1", port)).expect("managed engine should be stopped");
}
