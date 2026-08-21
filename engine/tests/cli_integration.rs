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
    assert!(!stdout.contains("  worker"), "help must not list worker");
    assert!(stdout.contains("project"), "help should list project");
    assert!(stdout.contains("update"), "help should list update");
    // "create" was replaced by `iii project init --template` — it must not
    // come back as a SUBCOMMAND. The word may legitimately appear in option
    // descriptions (the --config help says `iii` offers to create the file),
    // so check the subcommand-line pattern rather than a raw substring.
    let create_subcommand_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.trim_start().starts_with("create ") || l.trim() == "create")
        .collect();
    assert!(
        create_subcommand_lines.is_empty(),
        "help should NOT list create as a subcommand (replaced by `iii project init --template`), found: {:?}",
        create_subcommand_lines
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
fn worker_subcommand_is_rejected() {
    let output = iii_bin()
        .args(["worker", "add", "http"])
        .output()
        .expect("failed to execute");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand") && stderr.contains("worker"),
        "worker should be rejected by the root CLI, got: {stderr}"
    );
}

// ── Trigger subcommand ──────────────────────────────────────────────

#[test]
fn trigger_without_fn_path_fails() {
    let output = iii_bin()
        .args(["trigger"])
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "trigger with no FUNCTION_PATH should fail"
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
fn no_update_check_flag_accepted_with_subcommand() {
    let output = iii_bin()
        .args(["--no-update-check", "compose", "--help"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "--no-update-check should be accepted globally"
    );
}

#[test]
fn config_flag_is_rejected_with_compose() {
    let output = iii_bin()
        .args([
            "--config",
            "legacy-config.yaml",
            "compose",
            "--engine",
            "ws://127.0.0.1:49134",
        ])
        .output()
        .expect("failed to execute");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--config cannot be used with `iii compose`")
            && stderr.contains("engine: in worker-compose.yaml"),
        "compose should reject the legacy root config instead of silently ignoring it:\n{stderr}"
    );
}

// ── Error message quality ───────────────────────────────────────────

#[test]
fn error_messages_never_reference_iii_cli() {
    // Run several commands that produce errors and check none say "iii-cli"
    let dir = tempfile::tempdir().unwrap();

    let commands: Vec<Vec<&str>> = vec![
        vec!["start"],                           // invalid subcommand
        vec!["worker", "remove", "nonexistent"], // removed subcommand
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
    // "iii install" should not work; project workers are managed by Compose.
    let output = iii_bin()
        .arg("install")
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "\"iii install\" should not be valid"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand") && stderr.contains("install"),
        "install should be rejected by the root CLI, got: {stderr}"
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
        "\"iii uninstall\" should not be valid"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand") && stderr.contains("uninstall"),
        "uninstall should be rejected by the root CLI, got: {stderr}"
    );
}

#[test]
fn old_list_command_is_not_valid() {
    let output = iii_bin().arg("list").output().expect("failed to execute");
    assert!(!output.status.success(), "\"iii list\" should not be valid");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand") && stderr.contains("list"),
        "list should be rejected by the root CLI, got: {stderr}"
    );
}

#[test]
fn old_info_command_is_not_valid() {
    let output = iii_bin()
        .args(["info", "pdfkit"])
        .output()
        .expect("failed to execute");
    assert!(!output.status.success(), "\"iii info\" should not be valid");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand") && stderr.contains("info"),
        "info should be rejected by the root CLI, got: {stderr}"
    );
}

#[test]
fn trigger_help_shows_function_path_positional() {
    let output = iii_bin()
        .args(["trigger", "--help"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FUNCTION_PATH"),
        "trigger --help should show positional FUNCTION_PATH:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--json"),
        "trigger --help should show --json flag:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("--function-id"),
        "trigger --help must NOT show removed --function-id:\n{}",
        stdout
    );
}

#[test]
fn trigger_legacy_function_id_rejected_at_runtime() {
    // Pass a valid FUNCTION_PATH positional so the failure is unambiguously
    // due to the legacy --function-id flag, not a missing positional arg.
    let output = iii_bin()
        .args(["trigger", "test::fn", "--function-id", "legacy-id"])
        .output()
        .expect("failed to execute");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--function-id"),
        "stderr should reference unexpected --function-id:\n{}",
        stderr
    );
}

#[test]
fn update_list_targets_prints_targets() {
    let output = iii_bin()
        .args(["update", "--list-targets"])
        .output()
        .expect("failed to execute");
    assert!(output.status.success(), "exit: {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("self") && (stdout.contains("console") || stdout.contains("worker")),
        "expected list-targets to mention self + a managed binary:\n{}",
        stdout
    );
}

#[test]
fn update_unknown_target_hints_list_targets() {
    let output = iii_bin()
        .args(["update", "definitely-not-a-real-binary"])
        .output()
        .expect("failed to execute");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--list-targets"),
        "unknown target should hint --list-targets:\n{}",
        stderr
    );
}

#[test]
fn trigger_kv_with_json_merge_parses() {
    let output = iii_bin()
        .args([
            "trigger",
            "test::fn",
            "--json",
            r#"{"a":1,"b":2}"#,
            "a=99",
            "--port",
            "19999",
            "--timeout-ms",
            "200",
        ])
        .output()
        .expect("failed to execute");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("expected key=value") && !stderr.contains("must be an object"),
        "kv+json merge should parse cleanly:\n{}",
        stderr
    );
}
