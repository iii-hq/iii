// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Project auto-detection and manifest loading for worker dev sessions.

use colored::Colorize;
use std::collections::{BTreeMap, HashMap};

pub const WORKER_MANIFEST: &str = "iii.worker.yaml";

/// First-gate check on `runtime.base_image` — rejects shell
/// metacharacters, whitespace, NUL, etc. Does not replace
/// `Reference::parse`'s own grammar check.
fn is_plausible_image_ref(s: &str) -> bool {
    if s.is_empty() || s.len() > 512 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '@' | '+'))
}

/// Supported values for `runtime.kind` in `iii.worker.yaml`.
///
/// - `typescript` / `javascript`: node-based (needs `package_manager: npm|yarn|pnpm|bun`)
/// - `bun`: bun is runtime + package manager, no sub-field needed
/// - `python`: CPython + pip/venv
/// - `rust`: rustup + cargo
///
/// Keep in sync with the match arms in `infer_scripts()` and
/// `oci::oci_image_for_kind()`.
const SUPPORTED_KINDS: &[&str] = &["typescript", "javascript", "bun", "python", "rust"];

pub struct ProjectInfo {
    pub name: String,
    /// Runtime identifier from `runtime.kind` in the manifest (formerly
    /// `runtime.language`). Drives three things: default OCI image in
    /// `oci::oci_image_for_kind`, script inference in `infer_scripts`,
    /// and validation. `runtime.language` is still read for backwards
    /// compatibility with older manifests and mapped onto this field.
    pub kind: Option<String>,
    pub setup_cmd: String,
    pub install_cmd: String,
    pub run_cmd: String,
    pub env: HashMap<String, String>,
    /// Optional override for the base OCI image used as rootfs. When
    /// `None`, the default from `oci::oci_image_for_kind` is used. Set
    /// via `runtime.base_image` in `iii.worker.yaml` — e.g.
    /// `base_image: oven/bun:1` to pin a specific bun version instead
    /// of the default `oven/bun:latest`.
    pub base_image: Option<String>,
}

impl ProjectInfo {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(ref k) = self.kind
            && !k.is_empty()
            && !SUPPORTED_KINDS.contains(&k.as_str())
        {
            return Err(format!(
                "unrecognized runtime kind '{}' in {} — supported: {}",
                k,
                WORKER_MANIFEST,
                SUPPORTED_KINDS.join(", ")
            ));
        }
        if self.run_cmd.is_empty() {
            return Err(format!(
                "no run command could be determined — check {} for missing `scripts.start` or `runtime` section",
                WORKER_MANIFEST
            ));
        }
        Ok(())
    }
}

pub fn infer_scripts(kind: &str, package_manager: &str, entry: &str) -> (String, String, String) {
    match (kind, package_manager) {
        // `kind: bun` — no package_manager needed; bun IS the package
        // manager. The oven/bun:latest rootfs ships bun preinstalled so
        // setup is empty.
        ("bun", _) => (
            String::new(),
            "bun install".to_string(),
            format!("bun run {}", entry),
        ),
        // Legacy: `kind: typescript` + `package_manager: bun`. Same
        // bun commands, but without the oven rootfs the entrypoint may
        // need `$HOME/.bun/bin` on PATH — keep the bootstrap.
        ("typescript", "bun") => (
            "curl -fsSL https://bun.sh/install | bash".to_string(),
            "export PATH=$HOME/.bun/bin:$PATH && bun install".to_string(),
            format!("export PATH=$HOME/.bun/bin:$PATH && bun {}", entry),
        ),
        ("typescript" | "javascript", "npm") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "npm install".to_string(),
            format!("npx tsx {}", entry),
        ),
        // Note: pnpm exec and yarn exec require tsx in devDependencies (unlike npx which auto-installs)
        ("typescript" | "javascript", "pnpm") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "pnpm install".to_string(),
            format!("pnpm exec tsx {}", entry),
        ),
        ("typescript" | "javascript", "yarn") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "yarn install".to_string(),
            format!("yarn exec tsx {}", entry),
        ),
        ("typescript" | "javascript", other) => {
            eprintln!(
                "{} unrecognized package_manager '{}' for {} — supported: npm, yarn, pnpm, bun (or use `kind: bun` directly)",
                "error:".red(),
                other,
                kind
            );
            (String::new(), String::new(), String::new())
        },
        ("python", _) => (
            "command -v python3 >/dev/null || (apt-get update && apt-get install -y python3-venv python3-pip)".to_string(),
            "python3 -m venv .venv && .venv/bin/pip install -e .".to_string(),
            format!(".venv/bin/python -m {}", entry),
        ),
        ("rust", _) => (
            "command -v cargo >/dev/null || (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y)".to_string(),
            "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo build".to_string(),
            "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo run".to_string(),
        ),
        _ => (String::new(), String::new(), entry.to_string()),
    }
}

pub fn load_project_info(path: &std::path::Path) -> Option<ProjectInfo> {
    let manifest_path = path.join(WORKER_MANIFEST);
    if manifest_path.exists() {
        return load_from_manifest(&manifest_path);
    }
    auto_detect_project(path)
}

pub fn load_from_manifest(manifest_path: &std::path::Path) -> Option<ProjectInfo> {
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    let name = doc.get("name")?.as_str()?.to_string();

    let runtime = doc.get("runtime");
    // Prefer `runtime.kind`; fall back to the legacy `runtime.language`
    // so older manifests keep working. When only `language` is set,
    // print a one-line deprecation note so the user knows to migrate.
    let kind_str = runtime.and_then(|r| r.get("kind")).and_then(|v| v.as_str());
    let legacy_language = runtime
        .and_then(|r| r.get("language"))
        .and_then(|v| v.as_str());
    let kind = match (kind_str, legacy_language) {
        (Some(k), _) => k,
        (None, Some(l)) => {
            if std::env::var_os("III_NO_DEPRECATION_WARN").is_none() {
                eprintln!(
                    "{} {}: `runtime.language` is deprecated; rename to \
                     `runtime.kind`. Still accepted in v0.11.x; scheduled \
                     for removal in v0.13 (set III_NO_DEPRECATION_WARN=1 \
                     to silence).",
                    "warning:".yellow(),
                    manifest_path.display()
                );
            }
            l
        }
        (None, None) => "",
    };
    let package_manager = runtime
        .and_then(|r| r.get("package_manager"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let entry = runtime
        .and_then(|r| r.get("entry"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let base_image = runtime
        .and_then(|r| r.get("base_image"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| {
            let trimmed = s.trim();
            if !is_plausible_image_ref(trimmed) {
                eprintln!(
                    "{} {}: `runtime.base_image: {:?}` contains characters that don't \
                     belong in an OCI image reference (allowed: alphanumerics, `._-/:@`). \
                     Ignoring the override and falling back to the `kind` default.",
                    "warning:".yellow(),
                    manifest_path.display(),
                    trimmed
                );
                None
            } else {
                Some(trimmed.to_string())
            }
        });

    let scripts = doc.get("scripts");
    let (setup_cmd, install_cmd, run_cmd) = if scripts.is_some() {
        let setup = scripts
            .and_then(|s| s.get("setup"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let install = scripts
            .and_then(|s| s.get("install"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let start = scripts
            .and_then(|s| s.get("start"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        (setup, install, start)
    } else {
        // `package_manager` is required for typescript/javascript
        // (we need to know whether to use npm/yarn/pnpm/bun); `bun`,
        // `python`, `rust` don't need it.
        let needs_pm = matches!(kind, "typescript" | "javascript");
        if needs_pm && package_manager.is_empty() {
            eprintln!(
                "{} missing `package_manager` in {}\n\n  runtime:\n    kind: {}\n    package_manager: npm    <-- add this line\n    entry: {}\n\nhint: valid options are: npm, yarn, pnpm, bun. Or use `kind: bun` directly and drop `package_manager`.",
                "error:".red(),
                manifest_path.display(),
                kind,
                entry
            );
            return None;
        }
        infer_scripts(kind, package_manager, entry)
    };

    let mut env = HashMap::new();
    if let Some(env_map) = doc.get("env").and_then(|e| e.as_mapping()) {
        for (k, v) in env_map {
            if let (Some(key), Some(val)) = (k.as_str(), v.as_str())
                && key != "III_URL"
                && key != "III_ENGINE_URL"
            {
                env.insert(key.to_string(), val.to_string());
            }
        }
    }

    Some(ProjectInfo {
        name,
        kind: Some(kind.to_string()),
        setup_cmd,
        install_cmd,
        run_cmd,
        env,
        base_image,
    })
}

/// Read only the `dependencies:` block from an `iii.worker.yaml` manifest.
/// Returns an empty map when the file is missing (authors without deps work
/// unchanged) or the field is absent/null. Returns `Err` only when the file
/// exists but is malformed YAML or the `dependencies:` block is invalid.
pub fn load_manifest_dependencies(
    manifest_path: &std::path::Path,
) -> Result<BTreeMap<String, String>, String> {
    if !manifest_path.exists() {
        return Ok(BTreeMap::new());
    }
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("failed to read {}: {e}", manifest_path.display()))?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&content)
        .map_err(|e| format!("failed to parse {}: {e}", manifest_path.display()))?;
    let self_name = doc.get("name").and_then(|v| v.as_str());
    super::worker_manifest_deps::parse_dependencies(&doc, self_name)
}

pub fn auto_detect_project(path: &std::path::Path) -> Option<ProjectInfo> {
    let info = if path.join("package.json").exists() {
        if path.join("bun.lock").exists() || path.join("bun.lockb").exists() {
            // A bun lockfile on disk means the user is committed to
            // bun. Pick `kind: bun` so the oven/bun:latest rootfs gets
            // used — no bun bootstrap script needed.
            ProjectInfo {
                name: "bun".into(),
                kind: Some("bun".into()),
                setup_cmd: String::new(),
                install_cmd: "bun install".into(),
                run_cmd: "bun run dev".into(),
                env: HashMap::new(),
                base_image: None,
            }
        } else {
            ProjectInfo {
                name: "node (npm)".into(),
                kind: Some("typescript".into()),
                setup_cmd: "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".into(),
                install_cmd: "npm install".into(),
                run_cmd: "npm run dev".into(),
                env: HashMap::new(),
                base_image: None,
            }
        }
    } else if path.join("Cargo.toml").exists() {
        ProjectInfo {
            name: "rust".into(),
            kind: Some("rust".into()),
            setup_cmd: "command -v cargo >/dev/null || (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y)".into(),
            install_cmd: "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo build --release".into(),
            run_cmd: "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo run --release".into(),
            env: HashMap::new(),
            base_image: None,
        }
    } else if path.join("pyproject.toml").exists() || path.join("requirements.txt").exists() {
        ProjectInfo {
            name: "python".into(),
            kind: Some("python".into()),
            setup_cmd: "command -v python3 >/dev/null || (apt-get update && apt-get install -y python3 python3-pip python3-venv)".into(),
            install_cmd: "python3 -m pip install -e .".into(),
            run_cmd: "python3 -m iii".into(),
            env: HashMap::new(),
            base_image: None,
        }
    } else {
        return None;
    };
    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_manifest_with_explicit_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
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
        assert!(!info.env.contains_key("III_URL"));
        assert!(!info.env.contains_key("III_ENGINE_URL"));
    }

    #[test]
    fn load_manifest_auto_detects_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: my-bun-worker
runtime:
  kind: bun
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.name, "my-bun-worker");
        assert_eq!(info.kind.as_deref(), Some("bun"));
        assert!(info.install_cmd.contains("bun install"));
        assert!(info.run_cmd.contains("bun run src/index.ts"));
    }

    #[test]
    fn load_manifest_parses_base_image_override() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: bun-worker
runtime:
  kind: bun
  base_image: oven/bun:1
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.base_image.as_deref(), Some("oven/bun:1"));
    }

    #[test]
    fn load_manifest_base_image_absent_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: default-worker
runtime:
  kind: typescript
  package_manager: npm
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert!(info.base_image.is_none());
    }

    #[test]
    fn load_manifest_base_image_blank_is_none() {
        // Empty / whitespace-only values are treated as "not set" so a
        // stray `base_image: ""` doesn't get slugified into an empty
        // rootfs dir name downstream.
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: blank-worker
runtime:
  kind: typescript
  package_manager: npm
  base_image: "   "
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert!(info.base_image.is_none());
    }

    #[test]
    fn load_manifest_filters_engine_url_env() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: env-test
env:
  FOO: bar
  III_URL: skip
  III_ENGINE_URL: skip
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.env.get("FOO").unwrap(), "bar");
        assert!(!info.env.contains_key("III_URL"));
        assert!(!info.env.contains_key("III_ENGINE_URL"));
    }

    #[test]
    fn load_manifest_accepts_legacy_language_field() {
        // Backwards compat: old manifests with `runtime.language`
        // keep working. Emits a deprecation warning to stderr but
        // parses equivalently.
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: legacy-worker
runtime:
  language: typescript
  package_manager: bun
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.kind.as_deref(), Some("typescript"));
        assert!(info.install_cmd.contains("bun install"));
    }

    #[test]
    fn load_manifest_kind_wins_over_legacy_language() {
        // If both are set, `kind` wins — lets a user migrate
        // incrementally by adding `kind` before removing `language`.
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: migrating-worker
runtime:
  kind: bun
  language: typescript
  package_manager: npm
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.kind.as_deref(), Some("bun"));
    }

    #[test]
    fn infer_scripts_python() {
        let (setup, install, run) = infer_scripts("python", "pip", "my_module");
        assert!(setup.contains("python3-venv") || setup.contains("python3"));
        assert!(install.contains(".venv/bin/pip"));
        assert!(run.contains(".venv/bin/python -m my_module"));
    }

    #[test]
    fn infer_scripts_rust() {
        let (setup, install, run) = infer_scripts("rust", "cargo", "src/main.rs");
        assert!(setup.contains("rustup"));
        assert!(install.contains("cargo build"));
        assert!(run.contains("cargo run"));
    }

    #[test]
    fn infer_scripts_bun_as_kind() {
        // New path: `kind: bun` produces native bun commands with no
        // bootstrap (the oven/bun rootfs ships bun preinstalled).
        let (setup, install, run) = infer_scripts("bun", "", "src/index.ts");
        assert!(setup.is_empty(), "bun rootfs has bun preinstalled");
        assert!(install.contains("bun install"));
        assert!(run.contains("bun run src/index.ts"));
    }

    #[test]
    fn infer_scripts_typescript_plus_bun_pm() {
        // Legacy path: `kind: typescript` + `package_manager: bun`
        // still works, bootstraps bun via curl since the default
        // node rootfs doesn't ship it.
        let (setup, install, run) = infer_scripts("typescript", "bun", "src/index.ts");
        assert!(setup.contains("bun.sh/install"));
        assert!(install.contains("bun install"));
        assert!(run.contains("bun src/index.ts"));
    }

    #[test]
    fn infer_scripts_npm() {
        let (setup, install, run) = infer_scripts("typescript", "npm", "src/index.ts");
        assert!(setup.contains("nodejs") || setup.contains("nodesource"));
        assert!(install.contains("npm install"));
        assert!(run.contains("npx tsx src/index.ts"));
    }

    #[test]
    fn auto_detect_project_node_npm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "node (npm)");
        assert_eq!(info.kind.as_deref(), Some("typescript"));
        assert!(info.install_cmd.contains("npm"));
    }

    #[test]
    fn auto_detect_project_bun_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("bun.lock"), "").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "bun");
        assert_eq!(info.kind.as_deref(), Some("bun"));
        assert!(info.run_cmd.contains("bun"));
    }

    #[test]
    fn auto_detect_project_rust() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "rust");
        assert_eq!(info.kind.as_deref(), Some("rust"));
    }

    #[test]
    fn auto_detect_project_python() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "python");
        assert_eq!(info.kind.as_deref(), Some("python"));
    }

    #[test]
    fn auto_detect_project_unknown_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(auto_detect_project(dir.path()).is_none());
    }

    #[test]
    fn load_project_info_prefers_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let yaml = r#"
name: manifest-worker
runtime:
  kind: typescript
  package_manager: npm
  entry: src/index.ts
"#;
        std::fs::write(dir.path().join("iii.worker.yaml"), yaml).unwrap();
        let info = load_project_info(dir.path()).unwrap();
        assert_eq!(info.name, "manifest-worker");
    }

    #[test]
    fn infer_scripts_pnpm() {
        let (setup, install, run) = infer_scripts("typescript", "pnpm", "src/index.ts");
        assert!(setup.contains("nodejs") || setup.contains("nodesource"));
        assert!(install.contains("pnpm install"));
        assert!(run.contains("pnpm exec tsx src/index.ts"));
    }

    #[test]
    fn infer_scripts_yarn() {
        let (setup, install, run) = infer_scripts("typescript", "yarn", "src/index.ts");
        assert!(setup.contains("nodejs") || setup.contains("nodesource"));
        assert!(install.contains("yarn install"));
        assert!(run.contains("yarn exec tsx src/index.ts"));
    }

    #[test]
    fn infer_scripts_typescript_unknown_pm() {
        let (setup, install, run) = infer_scripts("typescript", "deno", "src/index.ts");
        assert!(setup.is_empty());
        assert!(install.is_empty());
        assert!(run.is_empty());
    }

    #[test]
    fn load_manifest_missing_package_manager_for_typescript() {
        // typescript/javascript require package_manager; bun/python/
        // rust don't. Verify the error only fires where it should.
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: broken-worker
runtime:
  kind: typescript
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        assert!(load_from_manifest(&manifest_path).is_none());
    }

    #[test]
    fn load_manifest_bun_kind_without_package_manager_works() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: bun-only
runtime:
  kind: bun
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.kind.as_deref(), Some("bun"));
        assert!(info.run_cmd.contains("bun run"));
    }

    #[test]
    fn validate_empty_run_cmd() {
        let project = ProjectInfo {
            name: "test".into(),
            kind: Some("typescript".into()),
            setup_cmd: String::new(),
            install_cmd: String::new(),
            run_cmd: String::new(),
            env: HashMap::new(),
            base_image: None,
        };
        assert!(project.validate().is_err());
    }

    #[test]
    fn validate_unrecognized_kind() {
        let project = ProjectInfo {
            name: "test".into(),
            kind: Some("typescirpt".into()),
            setup_cmd: String::new(),
            install_cmd: String::new(),
            run_cmd: "node index.js".into(),
            env: HashMap::new(),
            base_image: None,
        };
        let err = project.validate().unwrap_err();
        assert!(err.contains("unrecognized runtime kind"));
        assert!(err.contains("typescirpt"));
    }

    #[test]
    fn validate_accepts_bun_kind() {
        let project = ProjectInfo {
            name: "test".into(),
            kind: Some("bun".into()),
            setup_cmd: String::new(),
            install_cmd: "bun install".into(),
            run_cmd: "bun run src/index.ts".into(),
            env: HashMap::new(),
            base_image: None,
        };
        assert!(project.validate().is_ok());
    }

    #[test]
    fn load_manifest_dependencies_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: caller
runtime:
  kind: bun
  entry: src/index.ts
dependencies:
  math-worker: "^0.1.0"
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let deps = super::load_manifest_dependencies(&manifest_path).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps.get("math-worker").unwrap(), "^0.1.0");
    }

    #[test]
    fn load_manifest_dependencies_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let deps = super::load_manifest_dependencies(&manifest_path).unwrap();
        assert!(deps.is_empty());
    }
}
