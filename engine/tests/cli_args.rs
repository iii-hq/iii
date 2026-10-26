use std::process::{Command, Stdio};

fn iii_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iii"))
}

/// Spawn the engine in `dir`, wait for it to exit, and return whether it
/// created `file_name`.
///
/// The engine must never reach its listeners here: the mandatory
/// `iii-worker-manager` would bind the default `0.0.0.0:49134`, racing the
/// other test in this file and any engine already running on this machine.
/// A regular file named `config` blocks the configuration worker's default
/// cwd-relative `./config` store, so `build()` fails right after the config
/// file is written and the engine exits before anything binds.
fn spawn_and_wait_for_config(dir: &std::path::Path, args: &[&str], file_name: &str) -> bool {
    std::fs::write(dir.join("config"), "").expect("block the default configuration store");
    let stderr = tempfile::NamedTempFile::new().expect("create stderr capture");
    let mut child = iii_bin()
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr.reopen().expect("reopen stderr capture"))
        .spawn()
        .expect("failed to spawn iii");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll iii") {
            break Some(status);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let stderr = std::fs::read_to_string(stderr.path()).unwrap_or_default();
    assert!(
        status.is_some_and(|status| !status.success())
            && stderr.contains("configuration directory"),
        "engine should stop at the blocked configuration store, before binding \
         any port; status: {status:?}, stderr:\n{stderr}"
    );
    dir.join(file_name).exists()
}

#[test]
fn test_use_default_config_flag_is_rejected() {
    // `--use-default-config` was removed: `iii` now creates config.yaml when
    // it's missing instead of offering a file-less mode that bypassed the
    // config reload watcher.
    let output = iii_bin()
        .args(["--use-default-config", "--version"])
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "--use-default-config should no longer be accepted"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--use-default-config"),
        "Should reject the removed flag, got: {}",
        stderr
    );
}

#[test]
fn test_missing_default_config_is_created_headless() {
    // Non-interactive session (stdin is not a TTY): a missing config.yaml is
    // created instead of aborting the boot.
    let dir = tempfile::tempdir().unwrap();
    assert!(
        spawn_and_wait_for_config(dir.path(), &[], "config.yaml"),
        "engine should create config.yaml when it is missing"
    );
    let content = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
    assert!(
        content.contains("workers: []"),
        "created config should declare an empty workers list, got: {}",
        content
    );
    // Deliberately empty: project workers belong in worker-compose.yaml.
    assert!(
        !content.contains("- name:"),
        "created config must not pre-list any workers, got: {}",
        content
    );
}

#[test]
fn test_missing_explicit_config_is_created_headless() {
    // The same creation flow applies to a custom --config name, so setups
    // using e.g. config.yml work end to end.
    let dir = tempfile::tempdir().unwrap();
    assert!(
        spawn_and_wait_for_config(dir.path(), &["--config", "config.yml"], "config.yml"),
        "engine should create the file named by --config when it is missing"
    );
}
