//! Unit tests for TemplateFetcher: Local (disk) and Remote (git clone from
//! a local bare repo) paths, shared-file renames, and hard-fail behavior on
//! missing files.

use scaffolder_core::{
    LanguageFiles, RootManifest, TemplateFetcher, TemplateManifest, TemplateSource,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn root_manifest_yaml() -> &'static str {
    r#"
templates:
  - quickstart
shared_files:
  - source: default-gitignore
    dest: .gitignore
language_files:
  common:
    - .gitignore
    - README.md
  typescript:
    - '*.ts'
"#
}

fn template_manifest_yaml() -> &'static str {
    r#"
name: Quickstart
description: Test template
version: 0.1.0
requires: []
optional:
  - typescript
files:
  - README.md
  - src/index.ts
"#
}

/// Write a product-subdir layout (iii/ containing the root manifest,
/// shared files, and the quickstart template) under `root`.
fn write_product_fixture(root: &Path) {
    let iii = root.join("iii");
    std::fs::create_dir_all(iii.join("quickstart/src")).unwrap();
    std::fs::write(iii.join("template.yaml"), root_manifest_yaml()).unwrap();
    std::fs::write(iii.join("default-gitignore"), b"node_modules\n").unwrap();
    std::fs::write(
        iii.join("quickstart/template.yaml"),
        template_manifest_yaml(),
    )
    .unwrap();
    std::fs::write(iii.join("quickstart/README.md"), b"# Quickstart\n").unwrap();
    std::fs::write(iii.join("quickstart/src/index.ts"), b"export {};\n").unwrap();
}

/// Initialize `dir` as a git repo with one commit on `main` containing the
/// fixture. Returns a file:// URL suitable for `git clone`.
fn init_git_fixture(dir: &Path) -> url::Url {
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    write_product_fixture(dir);
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["add", "-A"]);
    run(&["commit", "-m", "fixture"]);
    url::Url::from_file_path(dir).expect("valid file URL")
}

fn local_subdir(root: &Path) -> PathBuf {
    root.join("iii")
}

// ---------------------------------------------------------------------------
// Local mode
// ---------------------------------------------------------------------------

#[tokio::test]
async fn local_loads_root_manifest() {
    let tmp = TempDir::new().unwrap();
    write_product_fixture(tmp.path());

    let mut fetcher = TemplateFetcher::from_local(local_subdir(tmp.path()), "test");
    let root = fetcher.fetch_root_manifest().await.unwrap();

    assert_eq!(root.templates, vec!["quickstart"]);
    assert_eq!(root.shared_files.len(), 1);
    assert_eq!(root.shared_files[0].source, "default-gitignore");
    assert_eq!(root.shared_files[0].destination(), ".gitignore");
}

#[tokio::test]
async fn local_loads_template_manifest_and_files() {
    let tmp = TempDir::new().unwrap();
    write_product_fixture(tmp.path());

    let mut fetcher = TemplateFetcher::from_local(local_subdir(tmp.path()), "test");
    let manifest = fetcher.fetch_template_manifest("quickstart").await.unwrap();

    assert_eq!(manifest.name, "Quickstart");
    // Shared file dest should have been merged into the files list
    assert!(manifest.files.contains(&".gitignore".to_string()));
    assert!(manifest.files.contains(&"README.md".to_string()));
    assert!(manifest.files.contains(&"src/index.ts".to_string()));
}

#[tokio::test]
async fn local_shared_file_rename_reads_from_root() {
    let tmp = TempDir::new().unwrap();
    write_product_fixture(tmp.path());

    let mut fetcher = TemplateFetcher::from_local(local_subdir(tmp.path()), "test");
    let _ = fetcher.fetch_template_manifest("quickstart").await.unwrap();
    let gitignore = fetcher
        .fetch_file("quickstart", ".gitignore")
        .await
        .unwrap();

    assert_eq!(gitignore, "node_modules\n");
}

#[tokio::test]
async fn local_missing_file_hard_fails() {
    let tmp = TempDir::new().unwrap();
    write_product_fixture(tmp.path());
    std::fs::remove_file(tmp.path().join("iii/quickstart/src/index.ts")).unwrap();

    let mut fetcher = TemplateFetcher::from_local(local_subdir(tmp.path()), "test");
    let result = fetcher.fetch_template_manifest("quickstart").await;

    assert!(result.is_err(), "missing file must hard-fail");
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("src/index.ts"),
        "error should name the missing file: {err}"
    );
}

// ---------------------------------------------------------------------------
// Remote mode (clones a local git fixture)
// ---------------------------------------------------------------------------

fn remote_source(repo: &Path) -> TemplateSource {
    let url = init_git_fixture(repo);
    TemplateSource::Remote {
        repo_url: url,
        subdir: "iii".to_string(),
        branch: "main".to_string(),
    }
}

#[tokio::test]
async fn remote_clones_and_loads_root_manifest() {
    let repo = TempDir::new().unwrap();
    let source = remote_source(repo.path());

    let mut fetcher = TemplateFetcher::new(source, "test");
    let root = fetcher.fetch_root_manifest().await.unwrap();

    assert_eq!(root.templates, vec!["quickstart"]);
}

#[tokio::test]
async fn remote_loads_template_files_after_clone() {
    let repo = TempDir::new().unwrap();
    let source = remote_source(repo.path());

    let mut fetcher = TemplateFetcher::new(source, "test");
    let manifest = fetcher.fetch_template_manifest("quickstart").await.unwrap();
    assert_eq!(manifest.name, "Quickstart");

    let readme = fetcher.fetch_file("quickstart", "README.md").await.unwrap();
    assert_eq!(readme, "# Quickstart\n");

    let gitignore = fetcher
        .fetch_file("quickstart", ".gitignore")
        .await
        .unwrap();
    assert_eq!(gitignore, "node_modules\n");
}

#[tokio::test]
async fn remote_missing_subdir_fails() {
    let repo = TempDir::new().unwrap();
    // Init repo but skip writing the iii subdir
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "{:?}", out);
    };
    std::fs::write(repo.path().join("README.md"), b"empty\n").unwrap();
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "t@e.com"]);
    run(&["config", "user.name", "T"]);
    run(&["add", "-A"]);
    run(&["commit", "-m", "empty"]);
    let url = url::Url::from_file_path(repo.path()).unwrap();

    let source = TemplateSource::Remote {
        repo_url: url,
        subdir: "iii".to_string(),
        branch: "main".to_string(),
    };
    let mut fetcher = TemplateFetcher::new(source, "test");
    let result = fetcher.fetch_root_manifest().await;

    assert!(result.is_err());
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("Subdirectory 'iii'"),
        "should report missing subdir: {err}"
    );
}

#[tokio::test]
async fn remote_invalid_url_fails() {
    let source = TemplateSource::Remote {
        repo_url: url::Url::parse("https://invalid.invalid.example/nope.git").unwrap(),
        subdir: "iii".to_string(),
        branch: "main".to_string(),
    };
    let mut fetcher = TemplateFetcher::new(source, "test");
    let result = fetcher.fetch_root_manifest().await;
    assert!(result.is_err(), "bogus host should fail cleanly");
}

// ---------------------------------------------------------------------------
// Sanity: plain parsing round-trip
// ---------------------------------------------------------------------------

#[test]
fn manifest_yaml_roundtrips() {
    let root: RootManifest = serde_yaml::from_str(root_manifest_yaml()).unwrap();
    assert_eq!(root.templates, vec!["quickstart"]);
    let tpl: TemplateManifest = serde_yaml::from_str(template_manifest_yaml()).unwrap();
    assert_eq!(tpl.files.len(), 2);
    let _: LanguageFiles = root.language_files;
}
