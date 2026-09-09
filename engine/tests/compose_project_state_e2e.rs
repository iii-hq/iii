//! Project-local Compose state exercised through the real CLI.

#![cfg(unix)]

use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use nix::{sys::signal::Signal, unistd::Pid};

fn compose_command(cwd: &Path, file: &Path, state_root: Option<&Path>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_iii"));
    command
        .current_dir(cwd)
        .args(["compose", "--up", "--file"])
        .arg(file)
        .env_remove("III_COMPOSE_STATE_DIR")
        .env_remove("III_URL")
        .env("III_TELEMETRY_ENABLED", "false")
        .env("TOKIO_WORKER_THREADS", "2")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null());
    if let Some(root) = state_root {
        command.env("III_COMPOSE_STATE_DIR", root);
    }
    command
}

struct ComposeProcess {
    child: Child,
    output: PathBuf,
}

impl ComposeProcess {
    fn start(mut command: Command, output: PathBuf, state_dir: &Path) -> Self {
        let log = fs::File::create(&output).unwrap();
        let child = command
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let mut process = Self { child, output };
        let deadline = Instant::now() + Duration::from_secs(30);
        while !state_dir.join("state.json").exists() {
            assert!(
                process.child.try_wait().unwrap().is_none() && Instant::now() < deadline,
                "compose did not start:\n{}",
                fs::read_to_string(&process.output).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        process
    }

    fn stop(&mut self) {
        nix::sys::signal::kill(Pid::from_raw(self.child.id() as i32), Signal::SIGTERM).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "compose failed:\n{}",
                    fs::read_to_string(&self.output).unwrap_or_default()
                );
                return;
            }
            assert!(Instant::now() < deadline, "compose did not stop");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for ComposeProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = nix::sys::signal::kill(Pid::from_raw(self.child.id() as i32), Signal::SIGTERM);
            let deadline = Instant::now() + Duration::from_secs(10);
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

fn write_compose(file: &Path, port: u16) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(
        file,
        format!("engine:\n  url: ws://127.0.0.1:{port}\n  workers: {{}}\ncontainers: {{}}\n"),
    )
    .unwrap();
}

#[test]
fn default_namespace_is_isolated_beside_each_compose_file() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("one/shop/worker-compose.yaml");
    let b = root.path().join("two/shop/worker-compose.yaml");
    let listener_a = TcpListener::bind("127.0.0.1:0").unwrap();
    let listener_b = TcpListener::bind("127.0.0.1:0").unwrap();
    let port_a = listener_a.local_addr().unwrap().port();
    let port_b = listener_b.local_addr().unwrap().port();
    write_compose(&a, port_a);
    write_compose(&b, port_b);
    let state_a = a.parent().unwrap().join(".iii/compose/default");
    let state_b = b.parent().unwrap().join(".iii/compose/default");
    drop((listener_a, listener_b));

    // Both invocations start outside their projects, using relative --file paths.
    let mut first = ComposeProcess::start(
        compose_command(root.path(), a.strip_prefix(root.path()).unwrap(), None),
        root.path().join("first.log"),
        &state_a,
    );
    let mut second = ComposeProcess::start(
        compose_command(root.path(), b.strip_prefix(root.path()).unwrap(), None),
        root.path().join("second.log"),
        &state_b,
    );

    for (file, state_dir, port) in [(&a, &state_a, port_a), (&b, &state_b, port_b)] {
        let state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(state_dir.join("state.json")).unwrap())
                .unwrap();
        assert_eq!(state["namespace"], "default");
        assert_eq!(
            state["compose_path"],
            file.canonicalize().unwrap().to_str().unwrap()
        );
        for artifact in ["engine.lock", "engine-config.yaml", "engine.log", "logs"] {
            assert!(state_dir.join(artifact).exists(), "missing {artifact}");
        }
        let config = fs::read_to_string(state_dir.join("engine-config.yaml")).unwrap();
        assert!(config.contains(&format!("port: {port}")), "{config}");
    }
    assert!(!root.path().join(".iii/compose").exists());

    first.stop();
    assert!(!state_a.join("engine-config.yaml").exists());
    assert!(!state_a.join("state.json").exists());
    assert!(second.child.try_wait().unwrap().is_none());
    assert!(state_b.join("engine-config.yaml").exists());
    TcpStream::connect(("127.0.0.1", port_b)).expect("second project's engine still serves");
    second.stop();
    assert!(!state_b.join("state.json").exists());
}

#[test]
fn relocated_state_groups_namespaces_under_each_project() {
    let root = tempfile::tempdir().unwrap();
    let state_root = root.path().join("shared-state");
    let mut processes = Vec::new();
    for (project, namespace) in [
        ("one/shop", "default"),
        ("two/shop", "default"),
        ("one/shop", "dev"),
    ] {
        let file = root.path().join(project).join("worker-compose.yaml");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        write_compose(&file, port);
        let slug = iii_compose::state::project_slug(&file.canonicalize().unwrap());
        let state_dir = state_root.join(&slug).join(namespace);
        let mut command = compose_command(root.path(), &file, Some(&state_root));
        command.args(["--namespace", namespace]);
        drop(listener);
        let process = ComposeProcess::start(
            command,
            root.path().join(format!("{slug}-{namespace}.log")),
            &state_dir,
        );
        assert!(state_dir.join("engine-config.yaml").exists());
        assert!(state_dir.join("engine.log").exists());
        assert!(!file.parent().unwrap().join(".iii/compose").exists());
        processes.push(process);
    }
    assert!(!state_root.join("default").exists());
    for process in &mut processes {
        process.stop();
    }
}

#[test]
fn the_same_project_namespace_is_refused_even_with_another_engine_port() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("worker-compose.yaml");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    write_compose(&file, port);
    let state_dir = root.path().join(".iii/compose/default");
    drop(listener);
    let mut first = ComposeProcess::start(
        compose_command(root.path(), &file, None),
        root.path().join("first.log"),
        &state_dir,
    );
    let original_config = fs::read(state_dir.join("engine-config.yaml")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    write_compose(&file, listener.local_addr().unwrap().port());
    drop(listener);
    fs::create_dir(root.path().join("links")).unwrap();
    std::os::unix::fs::symlink(&file, root.path().join("links/worker-compose.yaml")).unwrap();
    let output = compose_command(root.path(), Path::new("links/worker-compose.yaml"), None)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("DAEMON_NAMESPACE_TAKEN"), "{stderr}");
    assert_eq!(
        fs::read(state_dir.join("engine-config.yaml")).unwrap(),
        original_config
    );
    assert!(first.child.try_wait().unwrap().is_none());
    TcpStream::connect(("127.0.0.1", port)).expect("original engine still serves");

    write_compose(&file, port);
    first.stop();
    let mut restarted = ComposeProcess::start(
        compose_command(root.path(), &file, None),
        root.path().join("restarted.log"),
        &state_dir,
    );
    restarted.stop();
}
