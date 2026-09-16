//! End-to-end tests for `iii project init`.
//! Exercises the real binary so subcommand routing and filesystem state are
//! both verified.
//!
//! All tests pass `--template-dir` pointing at `engine/tests/fixtures/templates`
//! so they don't depend on the canonical `iii-hq/templates` repo being
//! reachable. The fixtures mirror what's in iii-hq/templates#2; if those
//! ever drift, copy them back over.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

fn iii_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iii"))
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("templates")
}

#[test]
fn project_init_creates_minimum_scaffold() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(
        out.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(dir.path().join(".iii").join("project.ini").exists());
    assert!(dir.path().join("config.yaml").exists());
    assert!(dir.path().join(".gitignore").exists());
}

#[test]
fn project_init_accepts_positional_name() {
    let parent = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "myapp", "--template-dir"])
        .arg(fixtures())
        .current_dir(parent.path())
        .output()
        .expect("failed to run iii");
    assert!(
        out.status.success(),
        "init <name> failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let project = parent.path().join("myapp");
    assert!(project.join(".iii").join("project.ini").exists());
    assert!(project.join("config.yaml").exists());
}

#[test]
fn project_init_preserves_project_id_and_user_edits_on_rerun() {
    let dir = tempdir().unwrap();
    let out1 = iii_bin()
        .args(["project", "init", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("first init");
    assert!(
        out1.status.success(),
        "first init must succeed: {}",
        String::from_utf8_lossy(&out1.stderr)
    );
    let ini1 = std::fs::read_to_string(dir.path().join(".iii").join("project.ini")).unwrap();

    let config_path = dir.path().join("config.yaml");
    let edited = "# user-edited config\nworkers: []\n";
    std::fs::write(&config_path, edited).unwrap();

    let out2 = iii_bin()
        .args(["project", "init", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("second init");
    assert!(
        out2.status.success(),
        "second init must preserve existing project state: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let ini2 = std::fs::read_to_string(dir.path().join(".iii").join("project.ini")).unwrap();

    let id_of = |s: &str| {
        s.lines()
            .find_map(|l| l.trim().strip_prefix("project_id="))
            .map(|v| v.trim().to_string())
            .expect("project_id present")
    };
    assert_eq!(id_of(&ini1), id_of(&ini2));
    assert_eq!(std::fs::read_to_string(&config_path).unwrap(), edited);
}

#[test]
fn project_init_with_empty_directory_flag_errors_clearly() {
    let out = iii_bin()
        .args(["project", "init", "--template-dir", ".", "--directory", ""])
        .output()
        .expect("failed to run iii");
    assert!(
        !out.status.success(),
        "empty --directory should fail to parse"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("directory argument cannot be empty"),
        "expected explicit empty-arg message:\n{stderr}"
    );
}

#[test]
fn project_init_writes_device_id_into_project_ini() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(out.status.success());

    let ini = std::fs::read_to_string(dir.path().join(".iii").join("project.ini")).unwrap();
    let device_id_line = ini
        .lines()
        .find(|l| l.starts_with("device_id="))
        .expect("project.ini should contain device_id=");
    let value = device_id_line.trim_start_matches("device_id=").trim();
    assert!(!value.is_empty(), "device_id should not be empty");
}

#[test]
fn project_init_errors_on_non_empty_dir_without_override() {
    // A pre-existing user file in the target directory should make init
    // refuse to scaffold (we'd otherwise silently overwrite it via
    // scaffolder-core's copy_template).
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# my project\n").unwrap();
    let out = iii_bin()
        .args(["project", "init", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(
        !out.status.success(),
        "init should refuse a non-empty directory without --allow-non-empty"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("contains README.md") || stderr.contains("cannot be initialized"),
        "expected non-empty error message:\n{stderr}"
    );
    assert!(
        stderr.contains("--allow-non-empty"),
        "error should suggest --allow-non-empty:\n{stderr}"
    );
    let readme = std::fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert_eq!(readme, "# my project\n", "user file must be untouched");
}

#[test]
fn project_init_with_allow_non_empty_succeeds() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "# my project\n").unwrap();
    let out = iii_bin()
        .args(["project", "init", "--allow-non-empty", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(
        out.status.success(),
        "init --allow-non-empty failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.path().join(".iii").join("project.ini").exists());
    assert!(dir.path().join("config.yaml").exists());
    // README.md isn't in the bare template's file list, so the user's copy
    // is left in place.
    assert!(dir.path().join("README.md").exists());
}

#[test]
fn project_init_into_dir_with_only_hidden_files_succeeds() {
    // A directory that only contains hidden dotfiles (e.g. a freshly
    // `git init`'d project) should not trigger the non-empty guard.
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    let out = iii_bin()
        .args(["project", "init", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(
        out.status.success(),
        "init in a dir with only .git/ should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn project_init_prints_next_steps_with_docs_link() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Next steps"),
        "expected 'Next steps' block in output:\n{stderr}"
    );
    assert!(
        stderr.contains("iii.dev/docs/quickstart"),
        "expected docs link in output:\n{stderr}"
    );
    assert!(
        stderr.contains("worker-compose.yaml") && stderr.contains("iii compose --up"),
        "next steps should mention Compose:\n{stderr}"
    );
}

#[test]
fn project_init_rejects_yes_flag() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "--yes", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(
        !out.status.success(),
        "init should reject --yes after removal"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--yes"),
        "expected clap rejection mentioning --yes:\n{stderr}"
    );
}

#[test]
fn project_init_rejects_languages_flag() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "--languages", "ts", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(
        !out.status.success(),
        "init should reject --languages after removal"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--languages"),
        "expected clap rejection mentioning --languages:\n{stderr}"
    );
}

#[test]
#[cfg(unix)]
fn project_init_failure_emits_problem_cause_fix() {
    let out = iii_bin()
        .args([
            "project",
            "init",
            "--template-dir",
            ".",
            "--directory",
            "/dev/null/cannot-create",
        ])
        .output()
        .expect("failed to run iii");
    assert!(
        !out.status.success(),
        "init should fail when target dir cannot be created"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:':\n{stderr}"
    );
    assert!(
        stderr.contains("cause:"),
        "stderr should contain 'cause:':\n{stderr}"
    );
    assert!(
        stderr.contains("fix:"),
        "stderr should contain 'fix:':\n{stderr}"
    );
}
