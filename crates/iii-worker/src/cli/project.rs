// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Project auto-detection and manifest loading for worker dev sessions.

use colored::Colorize;
use std::collections::HashMap;

pub const WORKER_MANIFEST: &str = "iii.worker.yaml";

// Keep in sync with infer_scripts() match arms
const SUPPORTED_LANGUAGES: &[&str] = &["typescript", "python", "rust"];

pub struct ProjectInfo {
    pub name: String,
    pub language: Option<String>,
    pub setup_cmd: String,
    pub install_cmd: String,
    pub run_cmd: String,
    pub env: HashMap<String, String>,
}

impl ProjectInfo {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(ref lang) = self.language {
            if !lang.is_empty() && !SUPPORTED_LANGUAGES.contains(&lang.as_str()) {
                return Err(format!(
                    "unrecognized language '{}' in {} — supported: {}",
                    lang,
                    WORKER_MANIFEST,
                    SUPPORTED_LANGUAGES.join(", ")
                ));
            }
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

pub fn infer_scripts(
    language: &str,
    package_manager: &str,
    entry: &str,
) -> (String, String, String) {
    match (language, package_manager) {
        ("typescript", "bun") => (
            "curl -fsSL https://bun.sh/install | bash".to_string(),
            "export PATH=$HOME/.bun/bin:$PATH && bun install".to_string(),
            format!("export PATH=$HOME/.bun/bin:$PATH && bun {}", entry),
        ),
        ("typescript", "npm") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "npm install".to_string(),
            format!("npx tsx {}", entry),
        ),
        // Note: pnpm exec and yarn exec require tsx in devDependencies (unlike npx which auto-installs)
        ("typescript", "pnpm") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "pnpm install".to_string(),
            format!("pnpm exec tsx {}", entry),
        ),
        ("typescript", "yarn") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "yarn install".to_string(),
            format!("yarn exec tsx {}", entry),
        ),
        ("typescript", other) => {
            eprintln!(
                "{} unrecognized package_manager '{}' for typescript — supported: npm, yarn, pnpm, bun",
                "error:".red(),
                other
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
    let language = runtime
        .and_then(|r| r.get("language"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let package_manager = runtime
        .and_then(|r| r.get("package_manager"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let entry = runtime
        .and_then(|r| r.get("entry"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

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
        if !language.is_empty() && package_manager.is_empty() {
            eprintln!(
                "{} missing `package_manager` in {}\n\n  runtime:\n    language: {}\n    package_manager: npm    <-- add this line\n    entry: {}\n\nhint: valid options are: npm, yarn, pnpm, bun",
                "error:".red(),
                manifest_path.display(),
                language,
                entry
            );
            return None;
        }
        infer_scripts(language, package_manager, entry)
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
        language: Some(language.to_string()),
        setup_cmd,
        install_cmd,
        run_cmd,
        env,
    })
}

pub fn auto_detect_project(path: &std::path::Path) -> Option<ProjectInfo> {
    let info = if path.join("package.json").exists() {
        if path.join("bun.lock").exists() || path.join("bun.lockb").exists() {
            ProjectInfo {
                name: "node (bun)".into(),
                language: Some("typescript".into()),
                setup_cmd: "curl -fsSL https://bun.sh/install | bash".into(),
                install_cmd: "$HOME/.bun/bin/bun install".into(),
                run_cmd: "$HOME/.bun/bin/bun run dev".into(),
                env: HashMap::new(),
            }
        } else {
            ProjectInfo {
                name: "node (npm)".into(),
                language: Some("typescript".into()),
                setup_cmd: "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".into(),
                install_cmd: "npm install".into(),
                run_cmd: "npm run dev".into(),
                env: HashMap::new(),
            }
        }
    } else if path.join("Cargo.toml").exists() {
        ProjectInfo {
            name: "rust".into(),
            language: Some("rust".into()),
            setup_cmd: "command -v cargo >/dev/null || (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y)".into(),
            install_cmd: "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo build --release".into(),
            run_cmd: "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo run --release".into(),
            env: HashMap::new(),
        }
    } else if path.join("pyproject.toml").exists() || path.join("requirements.txt").exists() {
        ProjectInfo {
            name: "python".into(),
            language: Some("python".into()),
            setup_cmd: "command -v python3 >/dev/null || (apt-get update && apt-get install -y python3 python3-pip python3-venv)".into(),
            install_cmd: "python3 -m pip install -e .".into(),
            run_cmd: "python3 -m iii".into(),
            env: HashMap::new(),
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
  language: typescript
  package_manager: bun
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.name, "my-bun-worker");
        assert!(info.setup_cmd.contains("bun.sh/install"));
        assert!(info.install_cmd.contains("bun install"));
        assert!(info.run_cmd.contains("bun src/index.ts"));
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
    fn infer_scripts_bun() {
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
        assert_eq!(info.language.as_deref(), Some("typescript"));
        assert!(info.install_cmd.contains("npm"));
    }

    #[test]
    fn auto_detect_project_node_bun() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("bun.lock"), "").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "node (bun)");
        assert!(info.run_cmd.contains("bun"));
    }

    #[test]
    fn auto_detect_project_rust() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "rust");
        assert_eq!(info.language.as_deref(), Some("rust"));
    }

    #[test]
    fn auto_detect_project_python() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "python");
        assert_eq!(info.language.as_deref(), Some("python"));
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
  language: typescript
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
    fn load_manifest_missing_package_manager() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: broken-worker
runtime:
  language: typescript
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        assert!(load_from_manifest(&manifest_path).is_none());
    }

    #[test]
    fn validate_empty_run_cmd() {
        let project = ProjectInfo {
            name: "test".into(),
            language: Some("typescript".into()),
            setup_cmd: String::new(),
            install_cmd: String::new(),
            run_cmd: String::new(),
            env: HashMap::new(),
        };
        assert!(project.validate().is_err());
    }

    #[test]
    fn validate_unrecognized_language() {
        let project = ProjectInfo {
            name: "test".into(),
            language: Some("typescirpt".into()),
            setup_cmd: String::new(),
            install_cmd: String::new(),
            run_cmd: "node index.js".into(),
            env: HashMap::new(),
        };
        let err = project.validate().unwrap_err();
        assert!(err.contains("unrecognized language"));
        assert!(err.contains("typescirpt"));
    }
}
