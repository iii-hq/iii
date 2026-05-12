//! End-to-end tests for `iii worker init`. Uses --template-dir against a
//! fixture so the tests are hermetic and don't depend on iii-hq/templates.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

fn worker_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iii-worker"))
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("templates")
}

#[test]
fn init_subcommand_is_reachable() {
    let out = worker_bin()
        .args(["init", "--help"])
        .output()
        .expect("run iii-worker");
    assert!(
        out.status.success(),
        "iii-worker init --help should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("--directory") || stdout.contains("Target directory"),
        "help should describe --directory; got: {stdout}"
    );
}

#[test]
fn init_creates_minimum_scaffold_and_templates_name() {
    let parent = tempdir().unwrap();
    let out = worker_bin()
        .args(["init", "mywkr", "--template-dir"])
        .arg(fixtures())
        .current_dir(parent.path())
        .output()
        .expect("failed to run iii-worker");
    assert!(
        out.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let root = parent.path().join("mywkr");
    assert!(
        root.join(".iii").join("worker.ini").exists(),
        "expected .iii/worker.ini"
    );
    assert!(
        root.join("iii.worker.yaml").exists(),
        "expected iii.worker.yaml"
    );
    assert!(root.join(".gitignore").exists(), "expected .gitignore");

    // Codex finding: worker.ini must include name and source, not just worker_id.
    let ini = std::fs::read_to_string(root.join(".iii").join("worker.ini")).unwrap();
    assert!(ini.contains("worker_id="), "worker.ini missing worker_id");
    assert!(
        ini.contains("name=mywkr"),
        "worker.ini name should match dirname, got: {ini}"
    );
    assert!(
        ini.contains("source=init"),
        "worker.ini missing source: {ini}"
    );

    // Name templating: iii.worker.yaml must reflect the dirname, not the
    // hardcoded fixture placeholder.
    let yaml = std::fs::read_to_string(root.join("iii.worker.yaml")).unwrap();
    assert!(
        yaml.contains("name: mywkr"),
        "iii.worker.yaml name should be templated to 'mywkr', got: {yaml}"
    );
    assert!(
        !yaml.contains("{{worker_name}}"),
        "iii.worker.yaml still contains the unresolved placeholder, got: {yaml}"
    );

    // Stderr should reference iii.worker.yaml and iii worker add (the new
    // next-steps message; per Issue 5).
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("iii.worker.yaml"),
        "next-steps should mention iii.worker.yaml; got: {stderr}"
    );
    assert!(
        stderr.contains("iii worker add"),
        "next-steps should mention `iii worker add`; got: {stderr}"
    );
}
