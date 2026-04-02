use std::process::Command;

fn iii_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iii"))
}

#[test]
fn test_conflicting_flags_exits_with_error() {
    let output = iii_bin()
        .args(["--use-default-config", "-c", "some.yaml"])
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "Should fail with conflicting flags"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "Should mention conflict, got: {}",
        stderr
    );
}

#[test]
fn test_missing_default_config_exits_with_error() {
    // Run in a temp dir where config.yaml doesn't exist
    let dir = tempfile::tempdir().unwrap();
    let output = iii_bin()
        .current_dir(dir.path())
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "Should fail when config.yaml is missing"
    );
    // Check stderr or stdout for the helpful message
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stderr, stdout);
    assert!(
        combined.contains("Config file not found") || combined.contains("config.yaml"),
        "Should mention missing config file, got: {}",
        combined
    );
}

#[test]
fn test_use_default_config_flag_is_accepted() {
    // Run with --use-default-config and --version to verify clap accepts the flag
    let output = iii_bin()
        .args(["--use-default-config", "--version"])
        .output()
        .expect("failed to execute");
    assert!(
        output.status.success(),
        "Should accept --use-default-config flag"
    );
}
