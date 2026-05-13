//! End-to-end tests for `iii worker init`. Uses --template-dir against a
//! fixture so the tests are hermetic and don't depend on iii-hq/templates.
//!
//! Tests always pass `--language <l> --skip-iii` so the run is fully
//! non-interactive (no cliclack prompt, no version check).

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
    assert!(
        stdout.contains("--language") || stdout.contains("language"),
        "help should describe --language; got: {stdout}"
    );
}

/// Shared assertions for a successful language-specific scaffold.
fn assert_lang_scaffold(
    lang_short: &str,
    expected_manifest: &str,
    expected_entry: &str,
    expected_files: &[&str],
) {
    let parent = tempdir().unwrap();
    let out = worker_bin()
        .args([
            "init",
            "mywkr",
            "--language",
            lang_short,
            "--skip-iii",
            "--template-dir",
        ])
        .arg(fixtures())
        .current_dir(parent.path())
        .output()
        .expect("failed to run iii-worker");
    assert!(
        out.status.success(),
        "init --language {lang_short} failed: {}",
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

    // .iii/worker.ini captures name, source, AND language.
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
    assert!(
        ini.contains(&format!("language={lang_short}")),
        "worker.ini missing language={lang_short}, got: {ini}"
    );

    // iii.worker.yaml: placeholders substituted.
    let yaml = std::fs::read_to_string(root.join("iii.worker.yaml")).unwrap();
    assert!(
        yaml.contains("name: mywkr"),
        "yaml name not templated: {yaml}"
    );
    assert!(
        yaml.contains(&format!("language: {expected_manifest}")),
        "yaml language wrong: {yaml}"
    );
    assert!(
        yaml.contains(&format!("entry: {expected_entry}")),
        "yaml entry wrong: {yaml}"
    );
    assert!(
        !yaml.contains("{{"),
        "yaml still has unresolved placeholders: {yaml}"
    );

    // Language-specific files exist.
    for f in expected_files {
        assert!(
            root.join(f).exists(),
            "expected language-specific file {f} for {lang_short}; tree: {:?}",
            std::fs::read_dir(&root).ok().map(|rd| rd
                .filter_map(|e| e.ok().map(|e| e.file_name()))
                .collect::<Vec<_>>())
        );
    }
}

#[test]
fn init_typescript_creates_node_scaffold_with_sdk() {
    assert_lang_scaffold(
        "ts",
        "typescript",
        "./src/index.ts",
        &["package.json", "tsconfig.json", "src/index.ts"],
    );
    let parent = tempdir().unwrap();
    let out = worker_bin()
        .args([
            "init",
            "ts-wkr",
            "--language",
            "ts",
            "--skip-iii",
            "--template-dir",
        ])
        .arg(fixtures())
        .current_dir(parent.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let pkg = std::fs::read_to_string(parent.path().join("ts-wkr").join("package.json")).unwrap();
    assert!(
        pkg.contains("@iii-hq/iii"),
        "package.json must pin @iii-hq/iii, got: {pkg}"
    );
}

#[test]
fn init_javascript_creates_node_scaffold() {
    assert_lang_scaffold(
        "js",
        "javascript",
        "./src/index.js",
        &["package.json", "src/index.js"],
    );
}

#[test]
fn init_python_creates_pyproject_with_sdk() {
    assert_lang_scaffold("py", "python", "./main.py", &["pyproject.toml", "main.py"]);
    let parent = tempdir().unwrap();
    let out = worker_bin()
        .args([
            "init",
            "py-wkr",
            "--language",
            "py",
            "--skip-iii",
            "--template-dir",
        ])
        .arg(fixtures())
        .current_dir(parent.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let pyproj =
        std::fs::read_to_string(parent.path().join("py-wkr").join("pyproject.toml")).unwrap();
    assert!(
        pyproj.contains("iii-sdk"),
        "pyproject.toml must pin iii-sdk, got: {pyproj}"
    );
}

#[test]
fn init_rust_creates_cargo_with_sdk() {
    assert_lang_scaffold(
        "rust",
        "rust",
        "./src/main.rs",
        &["Cargo.toml", "src/main.rs"],
    );
    let parent = tempdir().unwrap();
    let out = worker_bin()
        .args([
            "init",
            "rs-wkr",
            "--language",
            "rust",
            "--skip-iii",
            "--template-dir",
        ])
        .arg(fixtures())
        .current_dir(parent.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let cargo = std::fs::read_to_string(parent.path().join("rs-wkr").join("Cargo.toml")).unwrap();
    assert!(
        cargo.contains("iii"),
        "Cargo.toml must pin iii crate, got: {cargo}"
    );
}

#[test]
fn init_without_language_in_non_tty_fails_with_hint() {
    let parent = tempdir().unwrap();
    let out = worker_bin()
        .args(["init", "auto", "--skip-iii", "--template-dir"])
        .arg(fixtures())
        .current_dir(parent.path())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "non-TTY init without --language must fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--language"),
        "stderr should suggest --language, got: {stderr}"
    );
}

#[test]
fn init_preserves_worker_id_and_user_edits_on_rerun() {
    let dir = tempdir().unwrap();
    let run = || {
        worker_bin()
            .args(["init", "--language", "ts", "--skip-iii", "--template-dir"])
            .arg(fixtures())
            .arg("--directory")
            .arg(dir.path())
            .output()
            .expect("init run")
    };

    let out1 = run();
    assert!(
        out1.status.success(),
        "first init: {}",
        String::from_utf8_lossy(&out1.stderr)
    );
    let ini1 = std::fs::read_to_string(dir.path().join(".iii").join("worker.ini")).unwrap();

    // User edits iii.worker.yaml between runs.
    let yaml_path = dir.path().join("iii.worker.yaml");
    let edited = "name: my-custom-worker\nlanguage: typescript\nentry: ./custom.ts\n";
    std::fs::write(&yaml_path, edited).unwrap();

    let out2 = run();
    assert!(
        out2.status.success(),
        "second init: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let ini2 = std::fs::read_to_string(dir.path().join(".iii").join("worker.ini")).unwrap();

    let id_of = |s: &str| {
        s.lines()
            .find_map(|l| l.trim().strip_prefix("worker_id="))
            .map(|v| v.trim().to_string())
            .expect("worker_id present")
    };
    assert_eq!(
        id_of(&ini1),
        id_of(&ini2),
        "worker_id must persist across reruns"
    );

    // OV-3: idempotent apply_template must NOT clobber user edits.
    let yaml_after = std::fs::read_to_string(&yaml_path).unwrap();
    assert_eq!(
        yaml_after, edited,
        "re-init must not clobber edited iii.worker.yaml"
    );
}

#[test]
fn init_refuses_non_empty_directory_without_flag() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("user-file.txt"), "hello").unwrap();

    let out = worker_bin()
        .args(["init", "--language", "ts", "--skip-iii", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("run iii-worker");

    assert!(!out.status.success(), "init must fail on non-empty dir");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not empty"),
        "expected non-empty hint in stderr, got: {stderr}"
    );
}

#[test]
fn init_allows_non_empty_with_flag() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("user-file.txt"), "hello").unwrap();

    let out = worker_bin()
        .args(["init", "--language", "ts", "--skip-iii", "--template-dir"])
        .arg(fixtures())
        .arg("--directory")
        .arg(dir.path())
        .arg("--allow-non-empty")
        .output()
        .expect("run iii-worker");

    assert!(
        out.status.success(),
        "init --allow-non-empty must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.path().join(".iii").join("worker.ini").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("user-file.txt")).unwrap(),
        "hello",
        "init --allow-non-empty must not delete pre-existing files"
    );
}

#[test]
fn init_reports_clear_error_when_template_dir_missing() {
    let dir = tempdir().unwrap();
    let bogus = tempdir().unwrap(); // empty: no template.yaml at root
    let out = worker_bin()
        .args(["init", "--language", "ts", "--skip-iii", "--template-dir"])
        .arg(bogus.path())
        .arg("--directory")
        .arg(dir.path())
        .output()
        .expect("run iii-worker");

    assert!(
        !out.status.success(),
        "init must fail when template tree is empty"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("worker-bare")
            || stderr.contains("template")
            || stderr.contains("scaffold"),
        "stderr should mention the failing template, got: {stderr}"
    );
}
