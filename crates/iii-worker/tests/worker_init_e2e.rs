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
