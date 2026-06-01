// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration tests for project auto-detection, manifest loading, and script inference.
//!
//! Covers requirements PROJ-01 through PROJ-05:
//! - PROJ-01: TypeScript detection from package.json
//! - PROJ-02: Rust detection from Cargo.toml
//! - PROJ-03: Python detection from pyproject.toml / requirements.txt
//! - PROJ-04: Package manager differentiation (bun vs npm default)
//! - PROJ-05: Script inference and manifest loading

mod common;

use iii_worker::cli::project::{
    WORKER_MANIFEST, auto_detect_project, infer_scripts, load_from_manifest, load_project_info,
};

// ---------------------------------------------------------------------------
// Group 1: auto_detect_project tests (PROJ-01 through PROJ-04)
// ---------------------------------------------------------------------------

/// PROJ-01: Detects TypeScript/npm project from package.json without bun lockfile.
#[test]
fn auto_detect_typescript_npm_from_package_json() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), "{}").unwrap();

    let result = auto_detect_project(dir.path());
    assert!(result.is_some(), "expected Some for package.json project");
    let info = result.unwrap();
    assert_eq!(info.kind.as_deref(), Some("typescript"));
    assert_eq!(info.name, "node (npm)");
    assert!(
        info.install_cmd.contains("npm"),
        "install_cmd should contain 'npm': {}",
        info.install_cmd
    );
}

/// PROJ-01, PROJ-04: Detects TypeScript/bun project from package.json + bun.lock.
#[test]
fn auto_detect_typescript_bun_from_bun_lock() {
    // A bun lockfile promotes auto-detection to `kind: bun` so the
    // default rootfs is the bun-native oven/bun:latest image rather
    // than node-plus-bun-bootstrap.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), "{}").unwrap();
    std::fs::write(dir.path().join("bun.lock"), "").unwrap();

    let result = auto_detect_project(dir.path());
    assert!(result.is_some(), "expected Some for bun project");
    let info = result.unwrap();
    assert_eq!(info.name, "bun");
    assert_eq!(info.kind.as_deref(), Some("bun"));
    assert!(
        info.install_cmd.contains("bun") || info.run_cmd.contains("bun"),
        "bun commands should reference 'bun': install={}, run={}",
        info.install_cmd,
        info.run_cmd
    );
}

/// PROJ-04: Detects TypeScript/bun project from package.json + bun.lockb (binary lockfile).
#[test]
fn auto_detect_typescript_bun_from_bun_lockb() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), "{}").unwrap();
    std::fs::write(dir.path().join("bun.lockb"), "").unwrap();

    let result = auto_detect_project(dir.path());
    assert!(result.is_some(), "expected Some for bun.lockb project");
    let info = result.unwrap();
    assert_eq!(info.name, "bun");
    assert_eq!(info.kind.as_deref(), Some("bun"));
    assert!(
        info.install_cmd.contains("bun") || info.run_cmd.contains("bun"),
        "bun commands should reference 'bun': install={}, run={}",
        info.install_cmd,
        info.run_cmd
    );
}

/// PROJ-02: Detects Rust project from Cargo.toml.
#[test]
fn auto_detect_rust_from_cargo_toml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();

    let result = auto_detect_project(dir.path());
    assert!(result.is_some(), "expected Some for Cargo.toml project");
    let info = result.unwrap();
    assert_eq!(info.name, "rust");
    assert_eq!(info.kind.as_deref(), Some("rust"));
    assert!(
        info.install_cmd.contains("cargo"),
        "install_cmd should contain 'cargo': {}",
        info.install_cmd
    );
}

/// PROJ-03: Detects Python project from pyproject.toml.
#[test]
fn auto_detect_python_from_pyproject_toml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();

    let result = auto_detect_project(dir.path());
    assert!(result.is_some(), "expected Some for pyproject.toml project");
    let info = result.unwrap();
    assert_eq!(info.name, "python");
    assert_eq!(info.kind.as_deref(), Some("python"));
}

/// PROJ-03: Detects Python project from requirements.txt.
#[test]
fn auto_detect_python_from_requirements_txt() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("requirements.txt"), "flask").unwrap();

    let result = auto_detect_project(dir.path());
    assert!(
        result.is_some(),
        "expected Some for requirements.txt project"
    );
    let info = result.unwrap();
    assert_eq!(info.name, "python");
    assert_eq!(info.kind.as_deref(), Some("python"));
}

/// Returns None for an empty directory with no recognizable project markers.
#[test]
fn auto_detect_unknown_project_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        auto_detect_project(dir.path()).is_none(),
        "expected None for empty directory"
    );
}

/// PROJ-04: Without bun.lock, auto_detect defaults to npm. With bun.lock, detects bun.
///
/// NOTE: auto_detect_project does NOT detect yarn (yarn.lock) or pnpm (pnpm-lock.yaml)
/// from lockfiles. It only differentiates bun (bun.lock/bun.lockb) from the npm default.
/// The infer_scripts function supports yarn/pnpm but only when called from a manifest
/// with an explicit package_manager field.
#[test]
fn package_manager_detection_bun_vs_npm_default() {
    // Without bun lockfile -> defaults to npm
    let npm_dir = tempfile::tempdir().unwrap();
    std::fs::write(npm_dir.path().join("package.json"), "{}").unwrap();
    let npm_info = auto_detect_project(npm_dir.path()).unwrap();
    assert_eq!(npm_info.name, "node (npm)");

    // With bun.lock -> promotes to `kind: bun` (not `node (bun)`);
    // auto-detection prefers the native bun runtime when the user
    // already has a bun lockfile on disk.
    let bun_dir = tempfile::tempdir().unwrap();
    std::fs::write(bun_dir.path().join("package.json"), "{}").unwrap();
    std::fs::write(bun_dir.path().join("bun.lock"), "").unwrap();
    let bun_info = auto_detect_project(bun_dir.path()).unwrap();
    assert_eq!(bun_info.name, "bun");
}

// ---------------------------------------------------------------------------
// Group 2: infer_scripts tests (PROJ-05)
// ---------------------------------------------------------------------------

/// PROJ-05: Infer scripts for TypeScript/bun runtime.
#[test]
fn infer_scripts_typescript_bun() {
    let (setup, install, run) = infer_scripts("typescript", "bun", "src/index.ts");
    assert!(
        setup.contains("bun.sh/install"),
        "setup should contain 'bun.sh/install': {setup}"
    );
    assert!(
        install.contains("bun install"),
        "install should contain 'bun install': {install}"
    );
    assert!(
        run.contains("bun src/index.ts"),
        "run should contain 'bun src/index.ts': {run}"
    );
}

/// PROJ-05: Infer scripts for TypeScript/npm runtime.
#[test]
fn infer_scripts_typescript_npm() {
    let (setup, install, run) = infer_scripts("typescript", "npm", "src/index.ts");
    assert!(
        install.contains("npm install"),
        "install should contain 'npm install': {install}"
    );
    assert!(
        run.contains("npx tsx src/index.ts"),
        "run should contain 'npx tsx src/index.ts': {run}"
    );
}

/// PROJ-05: Infer scripts for Python/pip runtime.
#[test]
fn infer_scripts_python_pip() {
    let (setup, install, run) = infer_scripts("python", "pip", "my_module");
    assert!(
        setup.contains("python3"),
        "setup should contain 'python3': {setup}"
    );
    assert!(
        install.contains(".venv/bin/pip"),
        "install should contain '.venv/bin/pip': {install}"
    );
    assert!(
        run.contains(".venv/bin/python -m my_module"),
        "run should contain '.venv/bin/python -m my_module': {run}"
    );
}

/// PROJ-05: Infer scripts for Rust/cargo runtime.
#[test]
fn infer_scripts_rust_cargo() {
    let (setup, install, run) = infer_scripts("rust", "cargo", "src/main.rs");
    assert!(
        setup.contains("rustup"),
        "setup should contain 'rustup': {setup}"
    );
    assert!(
        install.contains("cargo build"),
        "install should contain 'cargo build': {install}"
    );
    assert!(
        run.contains("cargo run"),
        "run should contain 'cargo run': {run}"
    );
}

/// PROJ-05: Unknown language/package manager returns entry as run command.
#[test]
fn infer_scripts_unknown_returns_entry() {
    let (setup, install, run) = infer_scripts("unknown", "unknown", "main.sh");
    assert!(setup.is_empty(), "setup should be empty: {setup}");
    assert!(install.is_empty(), "install should be empty: {install}");
    assert_eq!(run, "main.sh");
}

// ---------------------------------------------------------------------------
// Group 3: Manifest loading and load_project_info (PROJ-05 augmentation)
// ---------------------------------------------------------------------------

/// PROJ-05: load_from_manifest with explicit scripts and env filtering.
#[test]
fn load_from_manifest_with_explicit_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join(WORKER_MANIFEST);
    let yaml = r#"
name: my-worker
scripts:
  setup: "apt-get update"
  install: "npm install"
  start: "node server.js"
env:
  FOO: bar
  III_URL: skip
  III_ENGINE_URL: skip
"#;
    std::fs::write(&manifest_path, yaml).unwrap();

    let info = load_from_manifest(&manifest_path).unwrap();
    assert_eq!(info.name, "my-worker");
    assert_eq!(info.setup_cmd, "apt-get update");
    assert_eq!(info.install_cmd, "npm install");
    assert_eq!(info.run_cmd, "node server.js");
    assert_eq!(info.env.get("FOO").unwrap(), "bar");
    assert!(
        !info.env.contains_key("III_URL"),
        "III_URL should be filtered out"
    );
    assert!(
        !info.env.contains_key("III_ENGINE_URL"),
        "III_ENGINE_URL should be filtered out"
    );
}

/// PROJ-05: load_from_manifest infers scripts from runtime section when no explicit scripts.
#[test]
fn load_from_manifest_infers_scripts_from_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join(WORKER_MANIFEST);
    let yaml = r#"
name: my-bun-worker
runtime:
  kind: typescript
  package_manager: bun
  entry: src/index.ts
"#;
    std::fs::write(&manifest_path, yaml).unwrap();

    let info = load_from_manifest(&manifest_path).unwrap();
    assert_eq!(info.name, "my-bun-worker");
    assert!(
        info.setup_cmd.contains("bun.sh/install"),
        "setup_cmd should contain 'bun.sh/install': {}",
        info.setup_cmd
    );
    assert!(
        info.run_cmd.contains("bun src/index.ts"),
        "run_cmd should contain 'bun src/index.ts': {}",
        info.run_cmd
    );
}

/// PROJ-05: load_project_info prefers manifest over auto-detection when both exist.
#[test]
fn load_project_info_prefers_manifest_over_auto_detect() {
    let dir = tempfile::tempdir().unwrap();
    // Create package.json (would auto-detect as node/npm)
    std::fs::write(dir.path().join("package.json"), "{}").unwrap();
    // Create manifest (should take precedence)
    let yaml = r#"
name: manifest-worker
runtime:
  kind: typescript
  package_manager: npm
  entry: src/index.ts
"#;
    std::fs::write(dir.path().join(WORKER_MANIFEST), yaml).unwrap();

    let info = load_project_info(dir.path()).unwrap();
    assert_eq!(
        info.name, "manifest-worker",
        "manifest should take precedence over auto-detection"
    );
}
