//! Integration tests that validate template directories on disk.
//!
//! These tests discover all templates under `templates/iii/` (and `templates/motia/`
//! when present) and verify:
//!   - Manifests parse into the expected types
//!   - Every file listed in a template manifest exists on disk
//!   - Shared files referenced from the root manifest exist
//!   - Every listed file matches at least one `language_files` pattern
//!   - Zip builds succeed end-to-end
//!   - SDK version strings are consistent within a template

use scaffolder_core::{LanguageFiles, RootManifest, TemplateManifest};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("cannot resolve workspace root")
}

/// All product template directories that exist on disk.
fn template_dirs() -> Vec<PathBuf> {
    let root = workspace_root();
    ["templates/iii", "templates/motia"]
        .iter()
        .map(|p| root.join(p))
        .filter(|p| p.join("template.yaml").exists())
        .collect()
}

fn read_root_manifest(dir: &Path) -> RootManifest {
    let content = std::fs::read_to_string(dir.join("template.yaml"))
        .unwrap_or_else(|e| panic!("read {}/template.yaml: {e}", dir.display()));
    serde_yaml::from_str(&content)
        .unwrap_or_else(|e| panic!("parse {}/template.yaml: {e}", dir.display()))
}

fn read_template_manifest(dir: &Path, name: &str) -> TemplateManifest {
    let path = dir.join(name).join("template.yaml");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_yaml::from_str(&content)
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn merged_language_files(root: &RootManifest, template: &TemplateManifest) -> LanguageFiles {
    let mut merged = root.language_files.clone();
    merged.merge(&template.language_files);
    merged
}

// ---------------------------------------------------------------------------
// Manifest parsing
// ---------------------------------------------------------------------------

#[test]
fn root_manifests_parse() {
    for dir in template_dirs() {
        let manifest = read_root_manifest(&dir);
        assert!(
            !manifest.templates.is_empty(),
            "{}: templates list is empty",
            dir.display()
        );
    }
}

#[test]
fn template_manifests_parse() {
    for dir in template_dirs() {
        let root = read_root_manifest(&dir);
        for name in &root.templates {
            let m = read_template_manifest(&dir, name);
            assert!(!m.name.is_empty(), "{name}: name is empty");
            assert!(!m.description.is_empty(), "{name}: description is empty");
            assert!(!m.version.is_empty(), "{name}: version is empty");
            assert!(!m.files.is_empty(), "{name}: files list is empty");
        }
    }
}

// ---------------------------------------------------------------------------
// File existence
// ---------------------------------------------------------------------------

#[test]
fn all_listed_files_exist() {
    for dir in template_dirs() {
        let root = read_root_manifest(&dir);
        let shared_dests: HashSet<String> = root
            .shared_files
            .iter()
            .map(|s| s.destination().to_string())
            .collect();

        for name in &root.templates {
            let manifest = read_template_manifest(&dir, name);
            let template_dir = dir.join(name);

            for file_path in &manifest.files {
                if shared_dests.contains(file_path.as_str()) {
                    continue;
                }
                let full = template_dir.join(file_path);
                assert!(
                    full.exists(),
                    "{name}: listed file does not exist: {file_path}"
                );
            }
        }
    }
}

#[test]
fn shared_file_sources_exist() {
    for dir in template_dirs() {
        let root = read_root_manifest(&dir);
        for shared in &root.shared_files {
            let full = dir.join(&shared.source);
            assert!(
                full.exists(),
                "shared file source does not exist: {}",
                shared.source
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Language file coverage
// ---------------------------------------------------------------------------

#[test]
fn every_file_matches_a_language_pattern() {
    for dir in template_dirs() {
        let root = read_root_manifest(&dir);
        for name in &root.templates {
            let manifest = read_template_manifest(&dir, name);
            let lang_files = merged_language_files(&root, &manifest);

            for file_path in &manifest.files {
                let lang = lang_files.get_language_for_file(file_path);
                assert!(
                    lang.is_some(),
                    "{name}: file '{file_path}' does not match any language_files pattern"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Zip build
// ---------------------------------------------------------------------------

#[test]
fn zip_build_succeeds() {
    use scaffolder_core::TemplateFetcher;

    for dir in template_dirs() {
        let root = read_root_manifest(&dir);
        for name in &root.templates {
            let result =
                TemplateFetcher::build_local_zip(&dir.to_path_buf(), name, &root.shared_files);
            assert!(
                result.is_ok(),
                "{name}: zip build failed: {}",
                result.unwrap_err()
            );

            let zip_bytes = result.unwrap();
            assert!(
                zip_bytes.len() > 100,
                "{name}: zip is suspiciously small ({} bytes)",
                zip_bytes.len()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// SDK version consistency
// ---------------------------------------------------------------------------

/// Extract the iii-sdk version from a package.json string.
fn npm_sdk_version(content: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(content).ok()?;
    v.get("dependencies")?
        .get("iii-sdk")?
        .as_str()
        .map(String::from)
}

/// Extract the iii-sdk version from a Cargo.toml string.
fn cargo_sdk_version(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("iii-sdk") {
            if let Some(eq) = trimmed.find('=') {
                let val = trimmed[eq + 1..].trim().trim_matches('"');
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Extract the iii-sdk version from a requirements.txt string.
fn pip_sdk_version(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("iii-sdk") {
            return Some(trimmed.to_string());
        }
    }
    None
}

#[test]
fn sdk_versions_consistent_within_template() {
    for dir in template_dirs() {
        let root = read_root_manifest(&dir);
        for name in &root.templates {
            let manifest = read_template_manifest(&dir, name);
            let template_dir = dir.join(name);

            let mut npm_versions: Vec<(String, String)> = Vec::new();
            let mut cargo_versions: Vec<(String, String)> = Vec::new();
            let mut pip_versions: Vec<(String, String)> = Vec::new();

            for file_path in &manifest.files {
                let full = template_dir.join(file_path);
                if !full.exists() {
                    continue;
                }

                let basename = file_path.rsplit('/').next().unwrap_or(file_path);
                let Ok(content) = std::fs::read_to_string(&full) else {
                    continue;
                };

                match basename {
                    "package.json" => {
                        if let Some(v) = npm_sdk_version(&content) {
                            npm_versions.push((file_path.clone(), v));
                        }
                    }
                    "Cargo.toml" => {
                        if let Some(v) = cargo_sdk_version(&content) {
                            cargo_versions.push((file_path.clone(), v));
                        }
                    }
                    "requirements.txt" => {
                        if let Some(v) = pip_sdk_version(&content) {
                            pip_versions.push((file_path.clone(), v));
                        }
                    }
                    _ => {}
                }
            }

            if npm_versions.len() > 1 {
                let first = &npm_versions[0].1;
                for (path, ver) in &npm_versions[1..] {
                    assert_eq!(
                        first, ver,
                        "{name}: npm SDK version mismatch — {} has {first} but {path} has {ver}",
                        npm_versions[0].0
                    );
                }
            }

            if cargo_versions.len() > 1 {
                let first = &cargo_versions[0].1;
                for (path, ver) in &cargo_versions[1..] {
                    assert_eq!(
                        first, ver,
                        "{name}: cargo SDK version mismatch — {} has {first} but {path} has {ver}",
                        cargo_versions[0].0
                    );
                }
            }

            if pip_versions.len() > 1 {
                let first = &pip_versions[0].1;
                for (path, ver) in &pip_versions[1..] {
                    assert_eq!(
                        first, ver,
                        "{name}: pip SDK version mismatch — {} has {first} but {path} has {ver}",
                        pip_versions[0].0
                    );
                }
            }
        }
    }
}
