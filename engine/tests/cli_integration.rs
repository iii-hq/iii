//! Integration tests for the merged CLI commands.
//!
//! These tests exercise the actual binary to verify subcommand routing,
//! help output, error messages, and backward compatibility.

use std::process::Command;

fn iii_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iii"))
}

// ── Version & help ──────────────────────────────────────────────────

#[test]
fn version_flag_prints_version() {
    let output = iii_bin()
        .arg("--version")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should be a valid semver
    let trimmed = stdout.trim();
    assert!(
        semver::Version::parse(trimmed).is_ok(),
        "Expected valid semver, got: {:?}",
        trimmed
    );
}

#[test]
fn short_version_flag_prints_version() {
    let output = iii_bin().arg("-v").output().expect("failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        semver::Version::parse(stdout.trim()).is_ok(),
        "Expected valid semver from -v flag"
    );
}

#[test]
fn help_flag_shows_all_subcommands() {
    let output = iii_bin().arg("--help").output().expect("failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // All subcommands should appear in help
    assert!(stdout.contains("trigger"), "help should list trigger");
    assert!(stdout.contains("console"), "help should list console");
    assert!(stdout.contains("worker"), "help should list worker");
    assert!(stdout.contains("project"), "help should list project");
    assert!(stdout.contains("update"), "help should list update");
    assert!(
        !stdout.contains(" create "),
        "help should NOT list create (replaced by `iii project init --template`)"
    );
}

#[test]
fn help_does_not_show_start_command() {
    let output = iii_bin().arg("--help").output().expect("failed to execute");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // "start" was removed — should not appear as a subcommand
    // (it may appear in description text, so check for the subcommand pattern)
    let lines: Vec<&str> = stdout.lines().collect();
    let subcommand_lines: Vec<&&str> = lines
        .iter()
        .filter(|l| l.trim_start().starts_with("start ") || l.trim() == "start")
        .collect();
    assert!(
        subcommand_lines.is_empty(),
        "\"start\" should not be a subcommand, found: {:?}",
        subcommand_lines
    );
}

// ── Invalid subcommand ──────────────────────────────────────────────

#[test]
fn invalid_subcommand_exits_with_error() {
    let output = iii_bin()
        .arg("nonexistent-command")
        .output()
        .expect("failed to execute");
    assert!(!output.status.success());
}

#[test]
fn start_subcommand_is_rejected() {
    let output = iii_bin().arg("start").output().expect("failed to execute");
    assert!(
        !output.status.success(),
        "\"iii start\" should not be a valid subcommand"
    );
}

// ── Worker subcommand group ─────────────────────────────────────────

#[test]
fn worker_help_shows_subcommands() {
    let output = iii_bin()
        .args(["worker", "--help"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("add"), "worker help should list add");
    assert!(stdout.contains("remove"), "worker help should list remove");
    assert!(stdout.contains("list"), "worker help should list list");
    assert!(stdout.contains("logs"), "worker help should list logs");
}

#[test]
fn worker_without_subcommand_shows_help() {
    let output = iii_bin().arg("worker").output().expect("failed to execute");
    // clap shows help/error when subcommand is missing
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("add") || combined.contains("Usage"),
        "worker without subcommand should show help or usage"
    );
}

#[test]
fn worker_list_runs_in_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let output = iii_bin()
        .args(["worker", "list"])
        .current_dir(dir.path())
        .output()
        .expect("failed to execute");
    // Should succeed (empty list) or fail gracefully (no iii.toml)
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stderr, stdout);
    assert!(
        combined.contains("No workers") || combined.contains("iii.toml") || output.status.success(),
        "worker list should handle empty directory gracefully, got: {}",
        combined
    );
}

#[test]
fn worker_add_without_network_fails_gracefully() {
    // worker add with a fake name — should fail with a registry error, not panic
    let dir = tempfile::tempdir().unwrap();
    let output = iii_bin()
        .args(["worker", "add", "nonexistent-worker-xyz-99999"])
        .current_dir(dir.path())
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "adding nonexistent worker should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error") || stderr.contains("failed") || stderr.contains("not found"),
        "should show an error message, got: {}",
        stderr
    );
}

#[test]
fn worker_remove_nonexistent_fails_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let output = iii_bin()
        .args(["worker", "remove", "nonexistent-worker"])
        .current_dir(dir.path())
        .output()
        .expect("failed to execute");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error") || stderr.contains("not installed") || stderr.contains("iii.toml"),
        "should show a helpful error, got: {}",
        stderr
    );
}

#[test]
fn worker_info_is_not_a_valid_subcommand() {
    let output = iii_bin()
        .args(["worker", "info"])
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "worker info should fail (not a valid subcommand)"
    );
}

#[test]
fn worker_remove_requires_worker_name() {
    let output = iii_bin()
        .args(["worker", "remove"])
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "worker remove without name should fail"
    );
}

// ── Trigger subcommand ──────────────────────────────────────────────

#[test]
fn trigger_requires_function_id() {
    let output = iii_bin()
        .args(["trigger", "--payload", "{}"])
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "trigger without --function-id should fail"
    );
}

#[test]
fn trigger_help_shows_options() {
    let output = iii_bin()
        .args(["trigger", "--help"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("function-id"),
        "trigger help should show --function-id"
    );
    assert!(
        stdout.contains("payload"),
        "trigger help should show --payload"
    );
}

// ── Update subcommand ───────────────────────────────────────────────

#[test]
fn update_help_shows_options() {
    let output = iii_bin()
        .args(["update", "--help"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
}

// ── No-update-check flag ────────────────────────────────────────────

#[test]
fn no_update_check_flag_accepted_with_version() {
    let output = iii_bin()
        .args(["--no-update-check", "--version"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
}

#[test]
fn no_update_check_flag_accepted_with_worker_list() {
    let dir = tempfile::tempdir().unwrap();
    let output = iii_bin()
        .args(["--no-update-check", "worker", "list"])
        .current_dir(dir.path())
        .output()
        .expect("failed to execute");
    // May succeed or fail (no iii.toml), but should not reject the flag
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "--no-update-check should be accepted globally"
    );
}

// ── Error message quality ───────────────────────────────────────────

#[test]
fn error_messages_never_reference_iii_cli() {
    // Run several commands that produce errors and check none say "iii-cli"
    let dir = tempfile::tempdir().unwrap();

    let commands: Vec<Vec<&str>> = vec![
        vec!["start"],                           // invalid subcommand
        vec!["worker", "remove", "nonexistent"], // worker not found
        vec!["worker", "info"],                  // invalid subcommand
    ];

    for args in &commands {
        let output = iii_bin()
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("failed to execute");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stderr.contains("iii-cli") && !stdout.contains("iii-cli"),
            "Command {:?} should not reference 'iii-cli' in output.\nstdout: {}\nstderr: {}",
            args,
            stdout,
            stderr
        );
    }
}

// ── Backward compatibility ──────────────────────────────────────────

#[test]
fn old_install_command_is_not_valid() {
    // "iii install" should not work — use "iii worker add" instead
    let output = iii_bin()
        .arg("install")
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "\"iii install\" should not be valid (use \"iii worker add\")"
    );
}

#[test]
fn old_uninstall_command_is_not_valid() {
    let output = iii_bin()
        .arg("uninstall")
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "\"iii uninstall\" should not be valid (use \"iii worker remove\")"
    );
}

#[test]
fn old_list_command_is_not_valid() {
    let output = iii_bin().arg("list").output().expect("failed to execute");
    assert!(
        !output.status.success(),
        "\"iii list\" should not be valid (use \"iii worker list\")"
    );
}

#[test]
fn old_info_command_is_not_valid() {
    let output = iii_bin()
        .args(["info", "pdfkit"])
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "\"iii info\" should not be valid (use \"iii worker info\")"
    );
}
