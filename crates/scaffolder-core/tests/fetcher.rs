//! Unit tests for TemplateFetcher: Remote (wiremock) and Local paths.
//!
//! Covers: root manifest loading, template manifest loading, file fetching with
//! and without shared-file renames, hard-fail semantics on missing files, and
//! end-to-end scaffolding against a mock templates repo.

use scaffolder_core::{LanguageFiles, RootManifest, TemplateFetcher, TemplateManifest};
use std::path::PathBuf;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn write_local_fixture(dir: &PathBuf) {
    // Root
    std::fs::write(dir.join("template.yaml"), root_manifest_yaml()).unwrap();
    std::fs::write(dir.join("default-gitignore"), b"node_modules\n").unwrap();
    // Template
    let tpl = dir.join("quickstart");
    std::fs::create_dir_all(tpl.join("src")).unwrap();
    std::fs::write(tpl.join("template.yaml"), template_manifest_yaml()).unwrap();
    std::fs::write(tpl.join("README.md"), b"# Quickstart\n").unwrap();
    std::fs::write(tpl.join("src/index.ts"), b"export {};\n").unwrap();
}

async fn mock_remote_fixture(server: &MockServer) {
    // Root manifest
    Mock::given(method("GET"))
        .and(path("/template.yaml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(root_manifest_yaml()))
        .mount(server)
        .await;
    // Shared file (at base, not inside template dir)
    Mock::given(method("GET"))
        .and(path("/default-gitignore"))
        .respond_with(ResponseTemplate::new(200).set_body_string("node_modules\n"))
        .mount(server)
        .await;
    // Template manifest
    Mock::given(method("GET"))
        .and(path("/quickstart/template.yaml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(template_manifest_yaml()))
        .mount(server)
        .await;
    // Template files
    Mock::given(method("GET"))
        .and(path("/quickstart/README.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string("# Quickstart\n"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/quickstart/src/index.ts"))
        .respond_with(ResponseTemplate::new(200).set_body_string("export {};\n"))
        .mount(server)
        .await;
}

// ---------------------------------------------------------------------------
// Local mode
// ---------------------------------------------------------------------------

#[tokio::test]
async fn local_loads_root_manifest() {
    let tmp = TempDir::new().unwrap();
    write_local_fixture(&tmp.path().to_path_buf());

    let mut fetcher = TemplateFetcher::from_local(tmp.path().to_path_buf(), "test");
    let root = fetcher.fetch_root_manifest().await.unwrap();

    assert_eq!(root.templates, vec!["quickstart"]);
    assert_eq!(root.shared_files.len(), 1);
    assert_eq!(root.shared_files[0].source, "default-gitignore");
    assert_eq!(root.shared_files[0].destination(), ".gitignore");
}

#[tokio::test]
async fn local_loads_template_manifest_and_files() {
    let tmp = TempDir::new().unwrap();
    write_local_fixture(&tmp.path().to_path_buf());

    let mut fetcher = TemplateFetcher::from_local(tmp.path().to_path_buf(), "test");
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
    write_local_fixture(&tmp.path().to_path_buf());

    let mut fetcher = TemplateFetcher::from_local(tmp.path().to_path_buf(), "test");
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
    write_local_fixture(&tmp.path().to_path_buf());
    // Corrupt: remove a file declared in the template manifest
    std::fs::remove_file(tmp.path().join("quickstart/src/index.ts")).unwrap();

    let mut fetcher = TemplateFetcher::from_local(tmp.path().to_path_buf(), "test");
    let result = fetcher.fetch_template_manifest("quickstart").await;

    assert!(result.is_err(), "missing file must hard-fail");
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("src/index.ts"),
        "error should name the missing file: {err}"
    );
}

// ---------------------------------------------------------------------------
// Remote mode (wiremock)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn remote_loads_root_manifest() {
    let server = MockServer::start().await;
    mock_remote_fixture(&server).await;

    let url = url::Url::parse(&server.uri()).unwrap();
    let mut fetcher =
        TemplateFetcher::new(scaffolder_core::TemplateSource::Remote(url), "test");
    let root = fetcher.fetch_root_manifest().await.unwrap();

    assert_eq!(root.templates, vec!["quickstart"]);
}

#[tokio::test]
async fn remote_loads_template_manifest_and_files() {
    let server = MockServer::start().await;
    mock_remote_fixture(&server).await;

    let url = url::Url::parse(&server.uri()).unwrap();
    let mut fetcher =
        TemplateFetcher::new(scaffolder_core::TemplateSource::Remote(url), "test");
    let manifest = fetcher.fetch_template_manifest("quickstart").await.unwrap();

    assert_eq!(manifest.name, "Quickstart");
    let readme = fetcher
        .fetch_file("quickstart", "README.md")
        .await
        .unwrap();
    assert_eq!(readme, "# Quickstart\n");
}

#[tokio::test]
async fn remote_shared_file_rename_fetches_from_root() {
    let server = MockServer::start().await;
    mock_remote_fixture(&server).await;

    let url = url::Url::parse(&server.uri()).unwrap();
    let mut fetcher =
        TemplateFetcher::new(scaffolder_core::TemplateSource::Remote(url), "test");
    let _ = fetcher.fetch_template_manifest("quickstart").await.unwrap();
    let gitignore = fetcher
        .fetch_file("quickstart", ".gitignore")
        .await
        .unwrap();

    assert_eq!(gitignore, "node_modules\n");
}

#[tokio::test]
async fn remote_404_on_required_file_hard_fails() {
    let server = MockServer::start().await;
    // Root and template manifest are fine
    Mock::given(method("GET"))
        .and(path("/template.yaml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(root_manifest_yaml()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/quickstart/template.yaml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(template_manifest_yaml()))
        .mount(&server)
        .await;
    // Shared file OK, README OK, src/index.ts returns 404
    Mock::given(method("GET"))
        .and(path("/default-gitignore"))
        .respond_with(ResponseTemplate::new(200).set_body_string("node_modules\n"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/quickstart/README.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string("# Quickstart\n"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/quickstart/src/index.ts"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let url = url::Url::parse(&server.uri()).unwrap();
    let mut fetcher =
        TemplateFetcher::new(scaffolder_core::TemplateSource::Remote(url), "test");
    let result = fetcher.fetch_template_manifest("quickstart").await;

    assert!(result.is_err(), "404 on required file must hard-fail");
    let err = format!("{:#}", result.unwrap_err());
    assert!(
        err.contains("src/index.ts") && err.contains("404"),
        "error should name file and HTTP status: {err}"
    );
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
