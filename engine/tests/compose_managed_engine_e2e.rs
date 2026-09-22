//! Managed-engine lifecycle exercised through the installed CLI shape.

use std::{net::TcpListener, process::Command};

#[cfg(unix)]
use std::{
    io::Read,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
    time::Instant,
};

fn iii_bin() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_iii"));
    // These fixtures own their engine endpoint. Never inherit the operator's
    // engine URL: it would override a managed engine and invalidate the test.
    command
        .env_remove("III_URL")
        .env_remove("III_COMPOSE_NAMESPACE")
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1");
    command
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

fn wait_for_port(port: u16, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

#[cfg(unix)]
fn wait_for_exit(child: &mut std::process::Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("compose did not exit after the shutdown signal");
}

#[cfg(unix)]
fn send_signal(child: &std::process::Child, signal: nix::sys::signal::Signal) {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(child.id() as i32), signal).unwrap();
}

#[cfg(unix)]
fn write_fixture_worker(
    script: &std::path::Path,
    ready: &std::path::Path,
    stopped: &std::path::Path,
    test_binary: &std::path::Path,
) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(
        script,
        format!(
            "#!/bin/sh\nREADY_MARKER={}\nSTOPPED_MARKER={}\nTEST_BINARY={}\nexport READY_MARKER\non_stop() {{\n  kill \"$worker\" 2>/dev/null || true\n  wait \"$worker\" 2>/dev/null || true\n  printf stopped > \"$STOPPED_MARKER\"\n  exit 0\n}}\ntrap on_stop TERM INT\n\"$TEST_BINARY\" --ignored --exact managed_worker_fixture --nocapture &\nworker=$!\nwait \"$worker\"\n",
            shell_quote(ready),
            shell_quote(stopped),
            shell_quote(test_binary),
        ),
    )
    .unwrap();
    std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o700)).unwrap();
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
#[serial_test::serial(managed_engine_port)]
fn compose_up_starts_logs_and_stops_the_engine_it_owns() {
    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let compose = project.path().join("worker-compose.yaml");
    // The missing worker directory is discovered only while bringing the
    // project up, after the managed engine is ready. That takes the command
    // through its error cleanup path without a language SDK fixture.
    std::fs::write(
        &compose,
        format!(
            "namespace: managed-test\nengine:\n  url: ws://127.0.0.1:{port}\n  workers:\n    iii-worker-manager:\n      host: 127.0.0.1\n      port: {port}\ncontainers:\n  missing:\n    worker: path://./does-not-exist\n"
        ),
    )
    .unwrap();

    let output = iii_bin()
        .current_dir(project.path())
        .env("III_COMPOSE_STATE_DIR", state.path())
        .args(["compose", "--namespace", "managed-e2e", "--up"])
        .output()
        .expect("run iii compose --up");

    assert!(!output.status.success(), "invalid project must fail");
    let terminal = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        terminal.contains("Engine Ready"),
        "unexpected output:\n{terminal}"
    );
    let progress = String::from_utf8_lossy(&output.stderr);
    let waiting = progress.find("Engine Waiting for connection").unwrap();
    let ready = progress.find("Engine Ready").unwrap();
    let downloads = progress.find("Downloads Checking").unwrap();
    let failed = progress.find("Downloads Failed").unwrap();
    assert!(
        waiting < ready && ready < downloads && downloads < failed,
        "{progress}"
    );
    assert!(progress.contains("Containers Not started"), "{progress}");
    assert!(!progress.contains("Containers Starting"), "{progress}");
    assert!(!progress.contains("Containers Ready"), "{progress}");
    assert!(
        !progress.contains('\x1b'),
        "redirected output must not animate: {progress}"
    );
    assert!(
        terminal.contains(compose.to_str().unwrap()),
        "owner file not announced:\n{terminal}"
    );

    let project_state = state
        .path()
        .join(iii_compose::state::project_slug(
            &compose.canonicalize().unwrap(),
        ))
        .join("managed-e2e");
    let generated_config = project_state.join("engine-config.yaml");
    assert!(
        terminal.contains(generated_config.to_str().unwrap()),
        "generated config not announced:\n{terminal}"
    );
    assert!(
        !generated_config.exists(),
        "clean error teardown must remove generated config"
    );

    let engine_log = project_state.join("engine.log");
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
    // project. On Unix, cleanup must release it before the foreground CLI
    // returns. Windows can keep the address unavailable in TIME_WAIT after the
    // process has exited, so an immediate rebind is not a reliable lifecycle
    // probe there; that path still exercises startup, logging, and cleanup.
    #[cfg(unix)]
    TcpListener::bind(("127.0.0.1", port)).expect("managed engine should be stopped");
}

#[test]
#[serial_test::serial(managed_engine_port)]
fn compose_without_engine_section_uses_and_preserves_an_external_engine() {
    let project = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let config = project.path().join("config.yaml");
    std::fs::write(
        &config,
        format!(
            "workers:\n  - name: iii-worker-manager\n    config:\n      host: 127.0.0.1\n      port: {port}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        project.path().join("worker-compose.yaml"),
        "namespace: external-test\ncontainers:\n  missing:\n    worker: path://./does-not-exist\n",
    )
    .unwrap();

    let mut engine = iii_bin()
        .current_dir(project.path())
        .args(["--config", config.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("start directly supervised engine");
    if !wait_for_port(port, std::time::Duration::from_secs(20)) {
        let _ = engine.kill();
        let _ = engine.wait();
        panic!("external engine never became ready");
    }

    let output = iii_bin()
        .current_dir(project.path())
        .args([
            "compose",
            "--engine",
            &format!("ws://127.0.0.1:{port}"),
            "--namespace",
            "external-e2e",
            "--up",
        ])
        .output()
        .expect("run external compose --up");
    let terminal = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let engine_survived = engine.try_wait().unwrap().is_none();
    let engine_reachable = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
    let _ = engine.kill();
    let _ = engine.wait();

    assert!(!output.status.success(), "missing project worker must fail");
    assert!(terminal.contains("compose serving"), "{terminal}");
    assert!(terminal.contains("Engine Connecting"), "{terminal}");
    assert!(!terminal.contains("Engine Starting"), "{terminal}");
    assert!(engine_survived, "Compose stopped the external engine");
    assert!(
        engine_reachable,
        "external engine stopped accepting connections"
    );
}

#[test]
#[serial_test::serial(managed_engine_port)]
fn cli_engine_overrides_file_engine_and_preserves_the_external_engine() {
    let project = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let ignored_probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let ignored_port = ignored_probe.local_addr().unwrap().port();
    drop(ignored_probe);

    let config = project.path().join("config.yaml");
    std::fs::write(
        &config,
        format!(
            "workers:\n  - name: iii-worker-manager\n    config:\n      host: 127.0.0.1\n      port: {port}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        project.path().join("worker-compose.yaml"),
        format!(
            "namespace: file-namespace\nengine:\n  url: ws://127.0.0.1:{ignored_port}\n  workers: {{}}\ncontainers:\n  missing:\n    worker: path://./does-not-exist\n"
        ),
    )
    .unwrap();

    let mut engine = iii_bin()
        .current_dir(project.path())
        .args(["--config", config.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("start directly supervised engine");
    if !wait_for_port(port, std::time::Duration::from_secs(20)) {
        let _ = engine.kill();
        let _ = engine.wait();
        panic!("external engine never became ready");
    }

    let output = iii_bin()
        .current_dir(project.path())
        .args([
            "compose",
            "--engine",
            &format!("ws://127.0.0.1:{port}"),
            "--namespace",
            "cli-namespace",
            "--up",
        ])
        .output()
        .expect("run external compose --up");
    let terminal = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let engine_survived = engine.try_wait().unwrap().is_none();
    let engine_reachable = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
    let _ = engine.kill();
    let _ = engine.wait();

    assert!(!output.status.success(), "missing project worker must fail");
    assert!(terminal.contains("compose serving"), "{terminal}");
    assert!(terminal.contains("namespace: cli-namespace"), "{terminal}");
    assert!(terminal.contains("Engine Connecting"), "{terminal}");
    assert!(!terminal.contains("Engine Starting"), "{terminal}");
    assert!(engine_survived, "Compose stopped the external engine");
    assert!(
        engine_reachable,
        "external engine stopped accepting connections"
    );
}

#[test]
fn compose_up_rejects_an_occupied_managed_engine_listener() {
    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::fs::write(
        project.path().join("worker-compose.yaml"),
        format!("engine:\n  url: ws://127.0.0.1:{port}\n  workers: {{}}\ncontainers: {{}}\n"),
    )
    .unwrap();

    let output = iii_bin()
        .current_dir(project.path())
        .env("III_COMPOSE_STATE_DIR", state.path())
        .args(["compose", "--namespace", "occupied-listener", "--up"])
        .output()
        .expect("run iii compose --up");
    let terminal = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(!output.status.success(), "an occupied listener must fail");
    assert!(
        terminal.contains("MANAGED_ENGINE_LISTENER_UNAVAILABLE"),
        "{terminal}"
    );
    assert!(terminal.contains("Engine Failed"), "{terminal}");
    assert!(terminal.contains("Containers Not started"), "{terminal}");
    assert!(!terminal.contains("Engine Ready"), "{terminal}");
}

#[cfg(unix)]
#[test]
#[serial_test::serial(managed_engine_port)]
fn signal_during_managed_engine_startup_stops_the_engine() {
    use std::os::unix::fs::PermissionsExt;

    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let worker = project.path().join("worker.sh");
    std::fs::write(&worker, "#!/bin/sh\nwhile :; do sleep 1; done\n").unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        project.path().join("worker-compose.yaml"),
        format!(
            "namespace: managed-test\nengine:\n  url: ws://127.0.0.1:{port}\n  workers:\n    iii-worker-manager:\n      host: 127.0.0.1\n      port: {port}\ncontainers:\n  probe:\n    worker: path://.\n    scripts:\n      run: ./worker.sh\n"
        ),
    )
    .unwrap();

    let generated_config = state
        .path()
        .join(iii_compose::state::project_slug(
            &project
                .path()
                .join("worker-compose.yaml")
                .canonicalize()
                .unwrap(),
        ))
        .join("managed-early-signal/engine-config.yaml");
    let mut child = iii_bin()
        .current_dir(project.path())
        .env("III_COMPOSE_STATE_DIR", state.path())
        .args(["compose", "--namespace", "managed-early-signal", "--up"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run iii compose --up");

    if !wait_for_file(&generated_config, Duration::from_secs(20)) {
        send_signal(&child, nix::sys::signal::Signal::SIGTERM);
        let _ = child.wait();
        panic!("managed engine config was never created");
    }
    send_signal(&child, nix::sys::signal::Signal::SIGINT);
    wait_for_exit(&mut child, Duration::from_secs(20));
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success(), "compose exited with {output:?}");
    let progress = String::from_utf8_lossy(&output.stderr);
    assert!(progress.contains("Cancelled"), "{progress}");
    assert!(!progress.contains("Containers Ready"), "{progress}");
    assert!(
        !generated_config.exists(),
        "managed engine config survived shutdown"
    );
    TcpListener::bind(("127.0.0.1", port)).expect("managed engine should be stopped");
}

#[cfg(unix)]
#[test]
#[serial_test::serial(managed_engine_port)]
fn signal_during_dependent_startup_rolls_back_every_started_process() {
    use std::os::unix::fs::PermissionsExt;

    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let test_binary = std::env::current_exe().unwrap();
    let root_ready = project.path().join("root.ready");
    let root_stopped = project.path().join("root.stopped");
    let root_script = project.path().join("root.sh");
    write_fixture_worker(&root_script, &root_ready, &root_stopped, &test_binary);

    let dependent_started = project.path().join("dependent.started");
    let dependent_stopped = project.path().join("dependent.stopped");
    let dependent_pid = project.path().join("dependent.pid");
    let dependent_script = project.path().join("dependent.sh");
    std::fs::write(
        &dependent_script,
        format!(
            "#!/bin/sh\nprintf %s \"$$\" > {}\nprintf started > {}\non_stop() {{\n  printf stopped > {}\n  exit 0\n}}\ntrap on_stop TERM INT\nwhile :; do sleep 1; done\n",
            shell_quote(&dependent_pid),
            shell_quote(&dependent_started),
            shell_quote(&dependent_stopped),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&dependent_script, std::fs::Permissions::from_mode(0o700)).unwrap();

    std::fs::write(
        project.path().join("worker-compose.yaml"),
        format!(
            "namespace: managed-test\nstartup_timeout: 60s\nstop_timeout: 5s\nengine:\n  url: ws://127.0.0.1:{port}\n  workers:\n    iii-worker-manager:\n      host: 127.0.0.1\n      port: {port}\ncontainers:\n  root:\n    worker: path://.\n    scripts:\n      run: ./root.sh\n  dependent:\n    worker: path://.\n    start_after: [root]\n    scripts:\n      run: ./dependent.sh\n"
        ),
    )
    .unwrap();

    let generated_config = state
        .path()
        .join(iii_compose::state::project_slug(
            &project
                .path()
                .join("worker-compose.yaml")
                .canonicalize()
                .unwrap(),
        ))
        .join("managed-dependent-signal/engine-config.yaml");
    let mut child = iii_bin()
        .current_dir(project.path())
        .env("III_COMPOSE_STATE_DIR", state.path())
        .args(["compose", "--namespace", "managed-dependent-signal", "--up"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run iii compose --up");

    if !wait_for_file(&dependent_started, Duration::from_secs(30)) {
        send_signal(&child, nix::sys::signal::Signal::SIGTERM);
        let _ = child.wait();
        panic!("dependent worker never entered startup");
    }
    let pid: i32 = std::fs::read_to_string(&dependent_pid)
        .unwrap()
        .parse()
        .unwrap();
    send_signal(&child, nix::sys::signal::Signal::SIGINT);
    wait_for_exit(&mut child, Duration::from_secs(20));
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success(), "compose exited with {output:?}");
    assert!(
        root_stopped.exists(),
        "ready root worker was not rolled back"
    );
    assert!(
        dependent_stopped.exists(),
        "starting dependent worker was not stopped"
    );
    assert!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err(),
        "dependent worker process {pid} survived"
    );
    assert!(
        !generated_config.exists(),
        "managed engine config survived shutdown"
    );
    TcpListener::bind(("127.0.0.1", port)).expect("managed engine should be stopped");

    // A clean restart on the same engine address and namespace proves that no
    // old worker registration or process survived the interrupted attempt.
    for marker in [
        &root_ready,
        &root_stopped,
        &dependent_started,
        &dependent_stopped,
        &dependent_pid,
    ] {
        let _ = std::fs::remove_file(marker);
    }
    write_fixture_worker(
        &dependent_script,
        &dependent_started,
        &dependent_stopped,
        &test_binary,
    );

    let mut restarted = iii_bin()
        .current_dir(project.path())
        .env("III_COMPOSE_STATE_DIR", state.path())
        .args(["compose", "--namespace", "managed-dependent-signal", "--up"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("restart iii compose --up");
    if !wait_for_file(&dependent_started, Duration::from_secs(30)) {
        send_signal(&restarted, nix::sys::signal::Signal::SIGTERM);
        let output = restarted.wait_with_output().unwrap();
        panic!("dependent worker was not ready after restart: {output:?}");
    }
    send_signal(&restarted, nix::sys::signal::Signal::SIGINT);
    wait_for_exit(&mut restarted, Duration::from_secs(20));
    let output = restarted.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "compose restart exited with {output:?}"
    );
    assert!(root_stopped.exists(), "root worker survived the restart");
    assert!(
        dependent_stopped.exists(),
        "dependent worker survived the restart"
    );
    assert!(
        !generated_config.exists(),
        "managed engine config survived restart shutdown"
    );
    TcpListener::bind(("127.0.0.1", port)).expect("managed engine should be stopped after restart");
}

#[cfg(unix)]
#[test]
#[serial_test::serial(managed_engine_port)]
fn ctrl_c_stops_the_worker_before_the_managed_engine() {
    use std::os::unix::fs::PermissionsExt;

    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

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
        format!(
            "namespace: managed-test\nstartup_timeout: 20s\nstop_timeout: 5s\nengine:\n  url: ws://127.0.0.1:{port}\n  workers:\n    iii-worker-manager:\n      host: 127.0.0.1\n      port: {port}\ncontainers:\n  probe:\n    worker: path://.\n    scripts:\n      run: ./worker.sh\n"
        ),
    )
    .unwrap();

    let mut child = iii_bin()
        .current_dir(project.path())
        .env("III_COMPOSE_STATE_DIR", state.path())
        .args(["compose", "--namespace", "managed-signal-e2e", "--up"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run iii compose --up");

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
    let progress = String::from_utf8_lossy(&output.stderr);
    assert!(progress.contains("Engine Ready"), "{progress}");
    assert!(progress.contains("Containers Ready"), "{progress}");

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

/// Collects a child's stream on a thread, so the child can be signalled and
/// waited for while its output keeps flowing.
#[cfg(unix)]
fn pump(mut reader: impl Read + Send + 'static) -> Arc<Mutex<String>> {
    let buffer = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&buffer);
    std::thread::spawn(move || {
        let mut bytes = [0_u8; 4096];
        while let Ok(read) = reader.read(&mut bytes) {
            if read == 0 {
                break;
            }
            sink.lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&bytes[..read]));
        }
    });
    buffer
}

/// Polls a pumped buffer until it contains `needle` or `timeout` elapses.
#[cfg(unix)]
fn wait_for_text(buffer: &Arc<Mutex<String>>, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if buffer.lock().unwrap().contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Ends whatever a test started and did not get to stop, so a failed
/// assertion never leaves a compose or an engine behind. Direct children are
/// killed and reaped through their handles. An engine is not the test's
/// child: it is remembered with the birth identity compose itself uses, and
/// signalled only while the pid still is that process.
#[cfg(unix)]
#[derive(Default)]
struct Leftovers {
    children: Vec<std::process::Child>,
    engines: Vec<(u32, iii_compose::process::BirthIdentity)>,
    /// The engine record a daemon under test writes: read on drop, so an
    /// engine spawned after the pids above were noted is not left behind.
    record: Option<std::path::PathBuf>,
}

#[cfg(unix)]
impl Leftovers {
    /// Takes ownership of a direct child; the index addresses it from then on.
    fn child(&mut self, child: std::process::Child) -> usize {
        self.children.push(child);
        self.children.len() - 1
    }

    /// Remembers an engine pid together with the birth identity it has now.
    fn engine(&mut self, pid: u32) {
        if !self.engines.iter().any(|(watched, _)| *watched == pid) {
            self.engines
                .push((pid, iii_compose::process::birth_identity(pid)));
        }
    }

    /// The pid and recorded birth identity in an engine record, if readable.
    fn recorded_engine(
        path: &std::path::Path,
    ) -> Option<(u32, iii_compose::process::BirthIdentity)> {
        let record: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        let pid = record["process"]["pid"].as_u64()? as u32;
        let born = serde_json::from_value(record["process"]["birth"].clone()).ok()?;
        Some((pid, born))
    }
}

#[cfg(unix)]
impl Drop for Leftovers {
    /// Kills and reaps the children, then signals every engine that is still
    /// the process it was when noted, the recorded one included.
    fn drop(&mut self) {
        for child in &mut self.children {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if let Some(recorded) = self.record.as_deref().and_then(Self::recorded_engine) {
            self.engines.push(recorded);
        }
        for (pid, born) in self.engines.drain(..) {
            if born.matches(&iii_compose::process::birth_identity(pid)) {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
#[serial_test::serial(managed_engine_port)]
fn compose_up_adopts_the_engine_a_killed_daemon_left_behind() {
    use std::net::TcpStream;

    /// Whether `pid` is a live process that is not a zombie.
    fn is_alive(pid: u32) -> bool {
        if nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_err() {
            return false;
        }
        // A zombie still answers signal 0. On Linux its state says so.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => !stat
                .rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z'),
            Err(_) => true,
        }
    }

    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let compose = project.path().join("worker-compose.yaml");
    std::fs::write(
        &compose,
        format!(
            "namespace: adopt-test\nengine:\n  url: ws://127.0.0.1:{port}\n  workers:\n    iii-worker-manager:\n      host: 127.0.0.1\n      port: {port}\ncontainers: {{}}\n"
        ),
    )
    .unwrap();
    let record_path = state
        .path()
        .join(iii_compose::state::project_slug(
            &compose.canonicalize().unwrap(),
        ))
        .join("adopt-test/engine.json");
    let record = || -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(&record_path).unwrap()).unwrap()
    };
    let spawn_compose = || {
        iii_bin()
            .current_dir(project.path())
            .env("III_COMPOSE_STATE_DIR", state.path())
            .args(["compose", "--namespace", "adopt-test", "--up"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run iii compose --up")
    };

    let mut leftovers = Leftovers::default();
    leftovers.record = Some(record_path.clone());
    let first = leftovers.child(spawn_compose());
    let first_out = pump(leftovers.children[first].stdout.take().unwrap());
    let first_err = pump(leftovers.children[first].stderr.take().unwrap());
    if !wait_for_text(&first_err, "Containers Ready", Duration::from_secs(30)) {
        if let Some(pid) = std::fs::read_to_string(&record_path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|record| record["process"]["pid"].as_u64())
        {
            leftovers.engine(pid as u32);
        }
        panic!(
            "first compose never came up:\n{}{}",
            first_out.lock().unwrap(),
            first_err.lock().unwrap()
        );
    }
    let engine_pid = record()["process"]["pid"]
        .as_u64()
        .expect("engine pid recorded") as u32;
    leftovers.engine(engine_pid);
    assert!(is_alive(engine_pid));

    // The daemon dies with no chance to stop anything: kill -9, the OOM
    // killer, an IDE ending the whole process tree. The engine lives in its
    // own process group precisely so that it survives this.
    send_signal(
        &leftovers.children[first],
        nix::sys::signal::Signal::SIGKILL,
    );
    let _ = leftovers.children[first].wait();
    std::thread::sleep(Duration::from_millis(200));
    assert!(is_alive(engine_pid), "the engine must outlive its daemon");
    TcpStream::connect(("127.0.0.1", port)).expect("the surviving engine still serves");

    // The next --up used to lose the port to that engine. Now it recognises
    // the engine as its own and carries on with it.
    let second = leftovers.child(spawn_compose());
    let second_out = pump(leftovers.children[second].stdout.take().unwrap());
    let second_err = pump(leftovers.children[second].stderr.take().unwrap());
    let came_up = wait_for_text(&second_err, "Containers Ready", Duration::from_secs(30));
    let terminal = || {
        format!(
            "{}{}",
            second_out.lock().unwrap(),
            second_err.lock().unwrap()
        )
    };
    // Whatever the second daemon recorded, adopted or freshly spawned, is
    // ours to clean up if an assertion below fails.
    if let Some(pid) = std::fs::read_to_string(&record_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|record| record["process"]["pid"].as_u64())
    {
        leftovers.engine(pid as u32);
    }
    if !came_up {
        panic!("second compose did not come up:\n{}", terminal());
    }
    assert!(
        !terminal().contains("MANAGED_ENGINE_LISTENER_UNAVAILABLE"),
        "{}",
        terminal()
    );
    assert!(terminal().contains("adopted:"), "{}", terminal());
    assert!(
        terminal().contains(&format!("pid: {engine_pid}")),
        "{}",
        terminal()
    );
    assert_eq!(
        record()["process"]["pid"].as_u64().unwrap() as u32,
        engine_pid,
        "adoption must not restart the engine"
    );

    // From here on the adopting daemon owns the engine: its shutdown stops it.
    send_signal(
        &leftovers.children[second],
        nix::sys::signal::Signal::SIGINT,
    );
    wait_for_exit(&mut leftovers.children[second], Duration::from_secs(20));
    assert!(!is_alive(engine_pid), "the adopted engine was not stopped");
    TcpListener::bind(("127.0.0.1", port)).expect("the adopted engine should be stopped");
    assert_eq!(record()["process"]["status"], "stopped");
}

#[cfg(unix)]
#[test]
#[serial_test::serial(managed_engine_port)]
fn a_signal_during_an_engine_replacement_waits_for_the_stop() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    /// A process that must not outlive the test: killed on drop unless it is
    /// known to have exited, and only while the pid is still that process.
    struct Doomed {
        pid: u32,
        birth: iii_compose::process::BirthIdentity,
        exited: Arc<AtomicBool>,
    }

    impl Drop for Doomed {
        /// SIGKILL, unless the process exited or the pid is someone else's.
        fn drop(&mut self) {
            if !self.exited.load(Ordering::Acquire)
                && self
                    .birth
                    .matches(&iii_compose::process::birth_identity(self.pid))
            {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(self.pid as i32),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
        }
    }

    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let compose = project.path().join("worker-compose.yaml");
    std::fs::write(
        &compose,
        format!(
            "namespace: replace-test\nengine:\n  url: ws://127.0.0.1:{port}\n  workers:\n    iii-worker-manager:\n      host: 127.0.0.1\n      port: {port}\ncontainers: {{}}\n"
        ),
    )
    .unwrap();
    let compose = compose.canonicalize().unwrap();
    let namespace_dir = state
        .path()
        .join(iii_compose::state::project_slug(&compose))
        .join("replace-test");
    std::fs::create_dir_all(&namespace_dir).unwrap();

    // The engine an earlier compose of this file left behind: verifiably its
    // own, but started from another engine section, so this start replaces
    // it. It ignores SIGTERM, which makes the replacement take the whole
    // grace, and it says when it was asked. A thread reaps it the moment it
    // dies and notes the time: a zombie would still look alive to compose.
    let armed = project.path().join("armed");
    let signalled = project.path().join("signalled");
    // In its own process group, as compose spawns engines: the stop signals
    // the group, not the pid.
    let mut survivor = std::os::unix::process::CommandExt::process_group(
        Command::new("sh")
            .arg("-c")
            .arg(format!(
                "trap 'touch {}' TERM INT; touch {}; while :; do sleep 1; done",
                shell_quote(&signalled),
                shell_quote(&armed)
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        0,
    )
    .spawn()
    .expect("spawn the surviving engine stand-in");
    let survivor_pid = survivor.id();
    let survivor_birth = iii_compose::process::birth_identity(survivor_pid);
    let exited = Arc::new(AtomicBool::new(false));
    let (survivor_exited, survivor_exit) = mpsc::channel();
    std::thread::spawn({
        let exited = Arc::clone(&exited);
        move || {
            let _ = survivor.wait();
            let at = Instant::now();
            exited.store(true, Ordering::Release);
            let _ = survivor_exited.send(at);
        }
    });
    let _doomed = Doomed {
        pid: survivor_pid,
        birth: survivor_birth,
        exited,
    };
    assert!(
        wait_for_file(&armed, Duration::from_secs(5)),
        "the survivor never armed its trap"
    );
    let record = serde_json::json!({
        "compose_path": compose,
        "launch": {
            "engine_url": "ws://old",
            "listener": "old",
            "cwd": "/old",
            "executable": { "path": "/old", "len": 0, "modified": null, "sha256": "old" },
            "env_fingerprint": "old",
        },
        "process": {
            "pid": survivor_pid,
            "birth": iii_compose::process::birth_identity(survivor_pid),
            "status": "starting",
            "started_at": 0,
        },
    });
    std::fs::write(
        namespace_dir.join("engine.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    std::fs::write(namespace_dir.join("engine-config.yaml"), "workers: []\n").unwrap();

    let mut leftovers = Leftovers::default();
    leftovers.record = Some(namespace_dir.join("engine.json"));
    let daemon = leftovers.child(
        iii_bin()
            .current_dir(project.path())
            .env("III_COMPOSE_STATE_DIR", state.path())
            .args(["compose", "--namespace", "replace-test", "--up"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run iii compose --up"),
    );
    let daemon_out = pump(leftovers.children[daemon].stdout.take().unwrap());
    let daemon_err = pump(leftovers.children[daemon].stderr.take().unwrap());
    let terminal = || {
        format!(
            "{}{}",
            daemon_out.lock().unwrap(),
            daemon_err.lock().unwrap()
        )
    };
    assert!(
        wait_for_file(&signalled, Duration::from_secs(30)),
        "compose never asked the surviving engine to stop:\n{}",
        terminal()
    );

    // The shutdown request lands while the replacement is in flight. Compose
    // may only exit once the engine it was stopping is gone: SIGTERM,
    // grace, SIGKILL, and never earlier.
    send_signal(
        &leftovers.children[daemon],
        nix::sys::signal::Signal::SIGINT,
    );
    wait_for_exit(&mut leftovers.children[daemon], Duration::from_secs(40));
    let compose_exit = Instant::now();
    let status = leftovers.children[daemon]
        .try_wait()
        .unwrap()
        .expect("compose has exited");
    // Whatever the daemon recorded, adopted or freshly spawned, is ours to
    // clean up if an assertion below fails.
    if let Some(pid) = std::fs::read_to_string(namespace_dir.join("engine.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|record| record["process"]["pid"].as_u64())
    {
        leftovers.engine(pid as u32);
    }

    // The reaper thread notes the instant right after `wait()` returns; it
    // may not have been scheduled yet when the poll above saw compose exit.
    // A bounded receive keeps the proof: the instants still compare.
    let survivor_exit = survivor_exit
        .recv_timeout(Duration::from_secs(5))
        .expect("the replaced engine must be gone before compose exits");
    assert!(
        survivor_exit <= compose_exit,
        "compose exited before the engine it was replacing:\n{}",
        terminal()
    );
    assert!(status.success(), "{}", terminal());
    // The pumps may still be copying the last lines the daemon wrote.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !terminal().contains("Cancelled") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(terminal().contains("Cancelled"), "{}", terminal());
    TcpListener::bind(("127.0.0.1", port)).expect("no engine may have been started");
}
