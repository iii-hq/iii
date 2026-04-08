// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Dev workflow orchestration for `iii worker dev`.

use colored::Colorize;
use std::collections::HashMap;

use super::project::{ProjectInfo, WORKER_MANIFEST, load_project_info};
use super::rootfs::clone_rootfs;

async fn detect_lan_ip() -> Option<String> {
    use tokio::process::Command;
    let route = Command::new("route")
        .args(["-n", "get", "default"])
        .output()
        .await
        .ok()?;
    let route_out = String::from_utf8_lossy(&route.stdout);
    let iface = route_out
        .lines()
        .find(|l| l.contains("interface:"))?
        .split(':')
        .nth(1)?
        .trim()
        .to_string();

    let ifconfig = Command::new("ifconfig").arg(&iface).output().await.ok()?;
    let ifconfig_out = String::from_utf8_lossy(&ifconfig.stdout);
    let ip = ifconfig_out
        .lines()
        .find(|l| l.contains("inet ") && !l.contains("127.0.0.1"))?
        .split_whitespace()
        .nth(1)?
        .to_string();

    Some(ip)
}

pub fn engine_url_for_runtime(
    _runtime: &str,
    _address: &str,
    port: u16,
    _lan_ip: &Option<String>,
) -> String {
    format!("ws://localhost:{}", port)
}

/// Ensure the terminal is in cooked mode with proper NL→CRNL translation.
#[cfg(unix)]
pub fn restore_terminal_cooked_mode() {
    let stderr = std::io::stderr();
    if let Ok(mut termios) = nix::sys::termios::tcgetattr(&stderr) {
        termios
            .output_flags
            .insert(nix::sys::termios::OutputFlags::OPOST);
        termios
            .output_flags
            .insert(nix::sys::termios::OutputFlags::ONLCR);
        let _ = nix::sys::termios::tcsetattr(&stderr, nix::sys::termios::SetArg::TCSANOW, &termios);
    }
}

pub async fn handle_worker_dev(
    path: &str,
    name: Option<&str>,
    runtime: Option<&str>,
    rebuild: bool,
    address: &str,
    port: u16,
) -> i32 {
    #[cfg(unix)]
    restore_terminal_cooked_mode();

    let project_path = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} Invalid path '{}': {}", "error:".red(), path, e);
            return 1;
        }
    };

    if let Err(e) = super::firmware::download::ensure_libkrunfw().await {
        tracing::warn!(error = %e, "failed to ensure libkrunfw");
    }

    let selected_runtime = match detect_dev_runtime(runtime).await {
        Some(rt) => rt,
        None => {
            eprintln!(
                "{} No dev runtime available.\n  \
                 Rebuild with --features embed-libkrunfw or place libkrunfw in ~/.iii/lib/",
                "error:".red()
            );
            return 1;
        }
    };

    let project = match load_project_info(&project_path) {
        Some(p) => p,
        None => {
            eprintln!(
                "{} Could not detect project type in '{}'. Add iii.worker.yaml or use package.json/Cargo.toml/pyproject.toml.",
                "error:".red(),
                project_path.display()
            );
            return 1;
        }
    };

    let has_manifest = project_path.join(WORKER_MANIFEST).exists();

    let dir_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("worker");
    let sb_name = name
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("iii-dev-{}", dir_name));
    let project_str = project_path.to_string_lossy();

    let lan_ip = detect_lan_ip().await;
    let engine_url = engine_url_for_runtime(&selected_runtime, address, port, &lan_ip);

    tracing::debug!(runtime = %selected_runtime, "selected dev runtime");

    eprintln!();
    if has_manifest {
        eprintln!(
            "  {}    loaded from {}",
            "Config".cyan().bold(),
            WORKER_MANIFEST.bold()
        );
    }
    let lang_suffix = project
        .language
        .as_deref()
        .map(|l| format!(" ({})", l.dimmed()))
        .unwrap_or_default();
    eprintln!(
        "  {}   {}{}",
        "Project".cyan().bold(),
        project.name.bold(),
        lang_suffix
    );
    eprintln!("  {}   {}", "Sandbox".cyan().bold(), sb_name.bold());
    eprintln!("  {}    {}", "Engine".cyan().bold(), engine_url.bold());
    eprintln!();

    let exit_code = run_dev_worker(
        &selected_runtime,
        &sb_name,
        &project_str,
        &project,
        &engine_url,
        rebuild,
    )
    .await;

    exit_code
}

async fn detect_dev_runtime(explicit: Option<&str>) -> Option<String> {
    if let Some(rt) = explicit {
        return Some(rt.to_string());
    }

    {
        if super::worker_manager::libkrun::libkrun_available() {
            return Some("libkrun".to_string());
        }
    }

    None
}

async fn run_dev_worker(
    runtime: &str,
    sb_name: &str,
    project_str: &str,
    project: &ProjectInfo,
    engine_url: &str,
    rebuild: bool,
) -> i32 {
    match runtime {
        "libkrun" => {
            let language = project.language.as_deref().unwrap_or("typescript");
            let mut env = build_dev_env(engine_url, &project.env);

            let base_rootfs = match super::worker_manager::oci::prepare_rootfs(language).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{} {}", "error:".red(), e);
                    return 1;
                }
            };

            let oci_env = super::worker_manager::oci::read_oci_env(&base_rootfs);
            for (key, value) in oci_env {
                env.entry(key).or_insert(value);
            }

            let dev_dir = match dirs::home_dir() {
                Some(h) => h.join(".iii").join("dev").join(sb_name),
                None => {
                    eprintln!("{} Cannot determine home directory", "error:".red());
                    return 1;
                }
            };
            let prepared_marker = dev_dir.join("var").join(".iii-prepared");

            if rebuild && dev_dir.exists() {
                eprintln!("  Rebuilding: clearing cached sandbox...");
                let _ = std::fs::remove_dir_all(&dev_dir);
            }

            if !dev_dir.exists() {
                eprintln!("  Preparing sandbox...");
                if let Err(e) = clone_rootfs(&base_rootfs, &dev_dir) {
                    eprintln!("{} Failed to create project rootfs: {}", "error:".red(), e);
                    return 1;
                }
            }

            let is_prepared = prepared_marker.exists();
            if is_prepared {
                eprintln!(
                    "  {} Using cached deps {}",
                    "✓".green(),
                    "(use --rebuild to reinstall)".dimmed()
                );
            }

            let script = build_libkrun_dev_script(project, is_prepared);

            let script_path = dev_dir.join("tmp").join("iii-dev-run.sh");
            if let Err(e) = std::fs::write(&script_path, &script) {
                eprintln!("{} Failed to write dev script: {}", "error:".red(), e);
                return 1;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
            }

            let workspace = dev_dir.join("workspace");
            std::fs::create_dir_all(&workspace).ok();
            if let Err(e) = copy_dir_contents(std::path::Path::new(project_str), &workspace) {
                eprintln!("{} Failed to copy project to rootfs: {}", "error:".red(), e);
                return 1;
            }

            let init_path = match super::firmware::download::ensure_init_binary().await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{} Failed to provision iii-init: {}", "error:".red(), e);
                    return 1;
                }
            };

            if !iii_filesystem::init::has_init() {
                let dest = dev_dir.join("init.krun");
                if let Err(e) = std::fs::copy(&init_path, &dest) {
                    eprintln!(
                        "{} Failed to copy iii-init to rootfs: {}",
                        "error:".red(),
                        e
                    );
                    return 1;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
                }
            }

            let exec_path = "/bin/sh";
            let args = vec![
                "-c".to_string(),
                "cd /workspace && exec bash /tmp/iii-dev-run.sh".to_string(),
            ];
            let manifest_path = std::path::Path::new(project_str).join(WORKER_MANIFEST);
            let (vcpus, ram) = parse_manifest_resources(&manifest_path);

            super::worker_manager::libkrun::run_dev(
                language,
                project_str,
                exec_path,
                &args,
                env,
                vcpus,
                ram,
                dev_dir,
            )
            .await
        }
        _ => {
            eprintln!("{} Unknown runtime: {}", "error:".red(), runtime);
            1
        }
    }
}

pub fn parse_manifest_resources(manifest_path: &std::path::Path) -> (u32, u32) {
    let default = (2, 2048);
    let content = match std::fs::read_to_string(manifest_path) {
        Ok(c) => c,
        Err(_) => return default,
    };
    let yaml: serde_yml::Value = match serde_yml::from_str(&content) {
        Ok(v) => v,
        Err(_) => return default,
    };
    let cpus = yaml
        .get("resources")
        .and_then(|r| r.get("cpus"))
        .and_then(|v| v.as_u64())
        .unwrap_or(2) as u32;
    let memory = yaml
        .get("resources")
        .and_then(|r| r.get("memory"))
        .and_then(|v| v.as_u64())
        .unwrap_or(2048) as u32;
    (cpus, memory)
}

pub fn copy_dir_contents(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    let skip = [
        "node_modules",
        ".git",
        "target",
        "__pycache__",
        ".venv",
        "dist",
    ];
    for entry in
        std::fs::read_dir(src).map_err(|e| format!("Failed to read {}: {}", src.display(), e))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if skip.iter().any(|s| *s == name_str.as_ref()) {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path).map_err(|e| e.to_string())?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn build_libkrun_dev_script(project: &ProjectInfo, prepared: bool) -> String {
    let env_exports = build_env_exports(&project.env);
    let mut parts: Vec<String> = Vec::new();

    parts.push("export PATH=/usr/local/bin:/usr/bin:/bin:$PATH".to_string());
    parts.push("export LANG=${LANG:-C.UTF-8}".to_string());
    parts.push("echo $$ > /sys/fs/cgroup/worker/cgroup.procs 2>/dev/null || true".to_string());

    if !prepared {
        if !project.setup_cmd.is_empty() {
            parts.push(project.setup_cmd.clone());
        }
        if !project.install_cmd.is_empty() {
            parts.push(project.install_cmd.clone());
        }
        parts.push("mkdir -p /var && touch /var/.iii-prepared".to_string());
    }

    parts.push(format!("{} && {}", env_exports, project.run_cmd));
    parts.join("\n")
}

pub fn build_dev_env(
    engine_url: &str,
    project_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("III_ENGINE_URL".to_string(), engine_url.to_string());
    env.insert("III_URL".to_string(), engine_url.to_string());
    for (key, value) in project_env {
        if key != "III_ENGINE_URL" && key != "III_URL" {
            env.insert(key.clone(), value.clone());
        }
    }
    env
}

pub fn build_env_exports(env: &HashMap<String, String>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (k, v) in env {
        if k == "III_ENGINE_URL" || k == "III_URL" {
            continue;
        }
        if !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || k.is_empty() {
            continue;
        }
        parts.push(format!("export {}='{}'", k, shell_escape(v)));
    }
    if parts.is_empty() {
        "true".to_string()
    } else {
        parts.join(" && ")
    }
}

pub fn shell_escape(s: &str) -> String {
    s.replace('\'', "'\\''")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managed_engine_url(bind_addr: &str) -> String {
        let (_host, port) = match bind_addr.rsplit_once(':') {
            Some((h, p)) => (h, p),
            None => (bind_addr, "49134"),
        };
        format!("ws://localhost:{}", port)
    }

    #[test]
    fn managed_engine_url_uses_localhost() {
        let url = managed_engine_url("0.0.0.0:49134");
        assert_eq!(url, "ws://localhost:49134");
    }

    #[test]
    fn build_env_exports_excludes_engine_urls() {
        let mut env = HashMap::new();
        env.insert(
            "III_ENGINE_URL".to_string(),
            "ws://localhost:49134".to_string(),
        );
        env.insert("III_URL".to_string(), "ws://localhost:49134".to_string());
        env.insert("CUSTOM_VAR".to_string(), "custom-val".to_string());

        let exports = build_env_exports(&env);
        assert!(!exports.contains("III_ENGINE_URL"));
        assert!(!exports.contains("III_URL"));
        assert!(exports.contains("CUSTOM_VAR='custom-val'"));
    }

    #[test]
    fn build_env_exports_empty_env() {
        let env = HashMap::new();
        let exports = build_env_exports(&env);
        assert_eq!(exports, "true");
    }

    #[test]
    fn engine_url_for_runtime_libkrun_uses_localhost() {
        let url = engine_url_for_runtime("libkrun", "0.0.0.0", 49134, &None);
        assert_eq!(url, "ws://localhost:49134");
    }

    #[test]
    fn build_libkrun_dev_script_first_run() {
        let project = ProjectInfo {
            name: "test".to_string(),
            language: Some("typescript".to_string()),
            setup_cmd: "apt-get install nodejs".to_string(),
            install_cmd: "npm install".to_string(),
            run_cmd: "node server.js".to_string(),
            env: HashMap::new(),
        };
        let script = build_libkrun_dev_script(&project, false);
        assert!(script.contains("apt-get install nodejs"));
        assert!(script.contains("npm install"));
        assert!(script.contains("node server.js"));
        assert!(script.contains(".iii-prepared"));
    }

    #[test]
    fn build_libkrun_dev_script_prepared() {
        let project = ProjectInfo {
            name: "test".to_string(),
            language: Some("typescript".to_string()),
            setup_cmd: "apt-get install nodejs".to_string(),
            install_cmd: "npm install".to_string(),
            run_cmd: "node server.js".to_string(),
            env: HashMap::new(),
        };
        let script = build_libkrun_dev_script(&project, true);
        assert!(!script.contains("apt-get install nodejs"));
        assert!(!script.contains("npm install"));
        assert!(script.contains("node server.js"));
    }

    #[test]
    fn build_dev_env_sets_engine_urls() {
        let env = build_dev_env("ws://localhost:49134", &HashMap::new());
        assert_eq!(env.get("III_ENGINE_URL").unwrap(), "ws://localhost:49134");
        assert_eq!(env.get("III_URL").unwrap(), "ws://localhost:49134");
    }

    #[test]
    fn build_dev_env_preserves_custom_env() {
        let mut project_env = HashMap::new();
        project_env.insert("CUSTOM".to_string(), "value".to_string());
        let env = build_dev_env("ws://localhost:49134", &project_env);
        assert_eq!(env.get("CUSTOM").unwrap(), "value");
        assert_eq!(env.get("III_ENGINE_URL").unwrap(), "ws://localhost:49134");
        assert_eq!(env.get("III_URL").unwrap(), "ws://localhost:49134");
    }

    #[test]
    fn build_dev_env_does_not_override_engine_urls() {
        let mut project_env = HashMap::new();
        project_env.insert("III_URL".to_string(), "custom".to_string());
        let env = build_dev_env("ws://localhost:49134", &project_env);
        assert_eq!(env.get("III_URL").unwrap(), "ws://localhost:49134");
    }

    #[test]
    fn parse_manifest_resources_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let nonexistent = dir.path().join("nonexistent.yaml");
        let (cpus, memory) = parse_manifest_resources(&nonexistent);
        assert_eq!(cpus, 2);
        assert_eq!(memory, 2048);
    }

    #[test]
    fn parse_manifest_resources_custom() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: resource-test
resources:
  cpus: 4
  memory: 4096
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let (cpus, memory) = parse_manifest_resources(&manifest_path);
        assert_eq!(cpus, 4);
        assert_eq!(memory, 4096);
    }

    #[test]
    fn shell_escape_single_quote() {
        let result = shell_escape("it's");
        assert_eq!(result, "it'\\''s");
    }

    #[test]
    fn copy_dir_contents_skips_ignored_dirs() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        std::fs::create_dir_all(src.path().join("src")).unwrap();
        std::fs::write(src.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::create_dir_all(src.path().join("node_modules/pkg")).unwrap();
        std::fs::write(src.path().join("node_modules/pkg/index.js"), "").unwrap();
        std::fs::create_dir_all(src.path().join(".git")).unwrap();
        std::fs::write(src.path().join(".git/config"), "").unwrap();
        std::fs::create_dir_all(src.path().join("target/debug")).unwrap();
        std::fs::write(src.path().join("target/debug/bin"), "").unwrap();

        copy_dir_contents(src.path(), dst.path()).unwrap();

        assert!(dst.path().join("src/main.rs").exists());
        assert!(!dst.path().join("node_modules").exists());
        assert!(!dst.path().join(".git").exists());
        assert!(!dst.path().join("target").exists());
    }
}
