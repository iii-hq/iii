use std::process::{Command, Stdio};

fn iii_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iii"))
}

/// Spawn the engine in `dir`, wait for it to create `file_name`, then kill it.
/// Returns whether the file appeared within the deadline. The engine may fail
/// later (e.g. port already bound by a concurrent test) — irrelevant here, the
/// config file is written before any listener binds.
fn spawn_and_wait_for_config(dir: &std::path::Path, args: &[&str], file_name: &str) -> bool {
    let mut child = iii_bin()
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iii");

    let config_path = dir.join(file_name);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut created = false;
    while std::time::Instant::now() < deadline {
        if config_path.exists() {
            created = true;
            break;
        }
        // A crashed child will never create the file; stop early.
        if let Ok(Some(_)) = child.try_wait() {
            created = config_path.exists();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    created
}

#[test]
fn test_use_default_config_flag_is_rejected() {
    // `--use-default-config` was removed: `iii` now creates config.yaml when
    // it's missing instead of offering a file-less mode that broke the
    // config.yaml setup (worker add, reload watcher).
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
    // Deliberately empty: no workers are pre-listed; users opt in via
    // `iii worker add <name>`.
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
