//! Managed-engine lifecycle exercised through the installed CLI shape.

use std::{net::TcpListener, process::Command};

#[cfg(unix)]
use std::{process::Stdio, time::Duration, time::Instant};

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

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct ProcessIdentity {
    pid: i32,
    start_time: u64,
}

#[cfg(target_os = "linux")]
fn process_status(pid: i32) -> Option<(char, u64)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut fields = stat[stat.rfind(')')? + 1..].split_whitespace();
    let state = fields.next()?.chars().next()?;
    let start_time = fields.nth(18)?.parse().ok()?;
    Some((state, start_time))
}

#[cfg(target_os = "linux")]
fn process_identity(pid: i32) -> Option<ProcessIdentity> {
    process_status(pid).map(|(_, start_time)| ProcessIdentity { pid, start_time })
}

#[cfg(target_os = "linux")]
fn process_has_terminated(identity: ProcessIdentity) -> bool {
    match process_status(identity.pid) {
        None => true,
        Some((state, start_time)) => {
            start_time != identity.start_time || matches!(state, 'Z' | 'X')
        }
    }
}

#[cfg(target_os = "linux")]
fn wait_for_process_exit(identity: ProcessIdentity, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if process_has_terminated(identity) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    process_has_terminated(identity)
}

#[cfg(target_os = "linux")]
fn wait_for_child_identity(parent_pid: u32, timeout: Duration) -> Option<ProcessIdentity> {
    let children = format!("/proc/{parent_pid}/task/{parent_pid}/children");
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(contents) = std::fs::read_to_string(&children) {
            for pid in contents
                .split_whitespace()
                .filter_map(|pid| pid.parse().ok())
            {
                if let Some(identity) = process_identity(pid) {
                    return Some(identity);
                }
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
fn signal_same_process(identity: ProcessIdentity, signal: nix::sys::signal::Signal) {
    if process_identity(identity.pid)
        .is_some_and(|current| current.start_time == identity.start_time)
    {
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(identity.pid), signal);
    }
}

#[cfg(target_os = "linux")]
fn ptrace_call(request: libc::c_uint, pid: i32, data: usize) -> std::io::Result<()> {
    let result = unsafe {
        libc::ptrace(
            request,
            pid,
            std::ptr::null_mut::<libc::c_void>(),
            data as *mut libc::c_void,
        )
    };
    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn seize_fork_events(pid: i32) -> std::io::Result<()> {
    ptrace_call(
        libc::PTRACE_SEIZE,
        pid,
        (libc::PTRACE_O_TRACEFORK | libc::PTRACE_O_TRACEVFORK) as usize,
    )
}

#[cfg(target_os = "linux")]
fn wait_for_fork_event(pid: i32, timeout: Duration) -> std::io::Result<i32> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let mut status = 0;
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG | libc::__WALL) };
        if waited == 0 {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        if waited == -1 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
            return Err(std::io::Error::other(
                "Compose exited before spawning its managed engine",
            ));
        }
        if !libc::WIFSTOPPED(status) {
            continue;
        }

        let event = status >> 16;
        if event == libc::PTRACE_EVENT_FORK || event == libc::PTRACE_EVENT_VFORK {
            let mut child_pid = 0_usize;
            let result = unsafe {
                libc::ptrace(
                    libc::PTRACE_GETEVENTMSG,
                    pid,
                    std::ptr::null_mut::<libc::c_void>(),
                    &mut child_pid as *mut usize as *mut libc::c_void,
                )
            };
            if result == -1 {
                return Err(std::io::Error::last_os_error());
            }
            return Ok(child_pid as i32);
        }
        ptrace_call(libc::PTRACE_CONT, pid, 0)?;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "Compose did not spawn its managed engine",
    ))
}

#[cfg(target_os = "linux")]
fn wait_for_ptrace_stop(pid: i32, timeout: Duration) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let mut status = 0;
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG | libc::__WALL) };
        if waited == 0 {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        if waited == -1 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::WIFSTOPPED(status) {
            return Ok(());
        }
        return Err(std::io::Error::other(
            "managed engine exited before its startup gate",
        ));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "managed engine did not enter its ptrace stop",
    ))
}

#[cfg(target_os = "linux")]
struct ComposeFixture {
    child: std::process::Child,
    engine: Option<ProcessIdentity>,
    engine_traced: bool,
}

#[cfg(target_os = "linux")]
impl ComposeFixture {
    fn new(child: std::process::Child) -> Self {
        Self {
            child,
            engine: None,
            engine_traced: false,
        }
    }

    fn child(&self) -> &std::process::Child {
        &self.child
    }

    fn child_mut(&mut self) -> &mut std::process::Child {
        &mut self.child
    }

    fn set_engine(&mut self, engine: ProcessIdentity, traced: bool) {
        self.engine = Some(engine);
        self.engine_traced = traced;
    }

    fn kill_compose(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn detach_engine(&mut self) -> std::io::Result<()> {
        let Some(engine) = self.engine else {
            return Ok(());
        };
        if self.engine_traced && !process_has_terminated(engine) {
            ptrace_call(libc::PTRACE_DETACH, engine.pid, 0)?;
        }
        self.engine_traced = false;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for ComposeFixture {
    fn drop(&mut self) {
        if self.engine.is_none() {
            self.engine = wait_for_child_identity(self.child.id(), Duration::from_millis(100));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(engine) = self.engine {
            if self.engine_traced && !process_has_terminated(engine) {
                let _ = ptrace_call(libc::PTRACE_DETACH, engine.pid, 0);
            }
            signal_same_process(engine, nix::sys::signal::Signal::SIGKILL);
        }
    }
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
fn sighup_during_managed_engine_startup_stops_the_engine() {
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
    send_signal(&child, nix::sys::signal::Signal::SIGHUP);
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

#[cfg(target_os = "linux")]
#[test]
#[serial_test::serial(managed_engine_port)]
fn sigkill_of_compose_stops_its_engine_and_allows_a_clean_restart() {
    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    std::fs::write(
        project.path().join("worker-compose.yaml"),
        format!(
            "namespace: abrupt-test\nengine:\n  url: ws://127.0.0.1:{port}\n  workers: {{}}\ncontainers: {{}}\n"
        ),
    )
    .unwrap();

    let spawn = || {
        iii_bin()
            .current_dir(project.path())
            .env("III_COMPOSE_STATE_DIR", state.path())
            .args(["compose", "--namespace", "abrupt-e2e", "--up"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("run iii compose --up")
    };

    let mut first = ComposeFixture::new(spawn());
    let engine = wait_for_child_identity(first.child().id(), Duration::from_secs(20))
        .expect("Compose never spawned its managed engine");
    first.set_engine(engine, false);
    assert!(
        wait_for_port(port, Duration::from_secs(20)),
        "managed engine never became ready"
    );
    first.kill_compose();

    assert!(
        wait_for_process_exit(engine, Duration::from_secs(10)),
        "managed engine survived SIGKILL of its Compose owner"
    );
    TcpListener::bind(("127.0.0.1", port))
        .expect("managed engine kept its listener after owner death");

    let mut restarted = ComposeFixture::new(spawn());
    let restarted_engine = wait_for_child_identity(restarted.child().id(), Duration::from_secs(20))
        .expect("restarted Compose never spawned its managed engine");
    restarted.set_engine(restarted_engine, false);
    assert!(
        wait_for_port(port, Duration::from_secs(20)),
        "a new Compose could not start a fresh managed engine"
    );
    send_signal(restarted.child(), nix::sys::signal::Signal::SIGTERM);
    wait_for_exit(restarted.child_mut(), Duration::from_secs(20));
    let status = restarted.child_mut().wait().unwrap();
    assert!(status.success(), "restarted Compose exited with {status}");

    assert!(
        wait_for_process_exit(restarted_engine, Duration::from_secs(10)),
        "fresh managed engine survived graceful Compose shutdown"
    );
    TcpListener::bind(("127.0.0.1", port))
        .expect("fresh managed engine kept its listener after graceful shutdown");
}

#[cfg(target_os = "linux")]
#[test]
#[serial_test::serial(managed_engine_port)]
fn sigkill_before_engine_readiness_does_not_orphan_the_engine() {
    use std::{ffi::CString, io::Write as _, os::unix::ffi::OsStrExt};

    let project = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    std::fs::write(
        project.path().join("worker-compose.yaml"),
        format!(
            "namespace: abrupt-startup-test\nengine:\n  url: ws://127.0.0.1:{port}\n  workers: {{}}\ncontainers: {{}}\n"
        ),
    )
    .unwrap();

    let gate = project.path().join("startup.gate");
    let gate_ready = project.path().join("startup-gate.ready");
    let gate_name = CString::new(gate.as_os_str().as_bytes()).unwrap();
    assert_eq!(
        unsafe { libc::mkfifo(gate_name.as_ptr(), 0o600) },
        0,
        "could not create startup gate: {}",
        std::io::Error::last_os_error()
    );

    let mut command = Command::new("/bin/sh");
    command
        .current_dir(project.path())
        .env_remove("III_URL")
        .env_remove("III_COMPOSE_NAMESPACE")
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1")
        .env("III_COMPOSE_STATE_DIR", state.path())
        .arg("-c")
        .arg("printf ready > \"$1\"; read _ < \"$2\"; shift 2; exec \"$@\"")
        .arg("managed-engine-startup-gate")
        .arg(&gate_ready)
        .arg(&gate)
        .arg(env!("CARGO_BIN_EXE_iii"))
        .args(["compose", "--namespace", "abrupt-startup-e2e", "--up"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut compose = ComposeFixture::new(command.spawn().expect("run gated iii compose --up"));
    assert!(
        wait_for_file(&gate_ready, Duration::from_secs(10)),
        "Compose wrapper never reached the startup gate"
    );
    seize_fork_events(compose.child().id() as i32).expect("attach startup tracer to Compose");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&gate)
        .and_then(|mut release| release.write_all(b"\n"))
        .expect("release Compose startup gate");

    let engine_pid = wait_for_fork_event(compose.child().id() as i32, Duration::from_secs(20))
        .expect("observe the managed engine spawn");
    let engine = process_identity(engine_pid).expect("record managed engine identity");
    compose.set_engine(engine, true);
    wait_for_ptrace_stop(engine_pid, Duration::from_secs(5))
        .expect("hold the managed engine before it can execute");

    assert!(
        !wait_for_port(port, Duration::from_millis(250)),
        "startup gate allowed the managed engine to become ready"
    );
    compose.kill_compose();
    compose
        .detach_engine()
        .expect("release managed engine after owner death");

    assert!(
        wait_for_process_exit(engine, Duration::from_secs(10)),
        "managed engine survived SIGKILL of Compose before readiness"
    );
    TcpListener::bind(("127.0.0.1", port))
        .expect("early managed engine kept its listener after owner death");

    let spawn = || {
        iii_bin()
            .current_dir(project.path())
            .env("III_COMPOSE_STATE_DIR", state.path())
            .args(["compose", "--namespace", "abrupt-startup-e2e", "--up"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("restart iii compose --up")
    };
    let mut restarted = ComposeFixture::new(spawn());
    let restarted_engine = wait_for_child_identity(restarted.child().id(), Duration::from_secs(20))
        .expect("restarted Compose never spawned its managed engine");
    restarted.set_engine(restarted_engine, false);
    assert!(
        wait_for_port(port, Duration::from_secs(20)),
        "a new Compose could not start after early owner death"
    );
    send_signal(restarted.child(), nix::sys::signal::Signal::SIGTERM);
    wait_for_exit(restarted.child_mut(), Duration::from_secs(20));
    let status = restarted.child_mut().wait().unwrap();
    assert!(status.success(), "restarted Compose exited with {status}");

    assert!(
        wait_for_process_exit(restarted_engine, Duration::from_secs(10)),
        "restarted managed engine survived graceful Compose shutdown"
    );
    TcpListener::bind(("127.0.0.1", port))
        .expect("restarted managed engine kept its listener after graceful shutdown");
}
