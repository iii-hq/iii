// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Local-path worker helpers: extracted shared functions from `dev.rs` plus
//! `handle_local_add` and `start_local_worker` for directory-based workers.

use colored::Colorize;
use std::collections::HashMap;
use std::path::Path;

use super::project::{ProjectInfo, WORKER_MANIFEST, load_project_info};
use super::rootfs::clone_rootfs;

// ──────────────────────────────────────────────────────────────────────────────
// Shared helpers (extracted from dev.rs)
// ──────────────────────────────────────────────────────────────────────────────

pub async fn detect_lan_ip() -> Option<String> {
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

/// Ensure the terminal is in cooked (canonical) mode with proper input and
/// output processing.  Restores both output flags (NL→CRNL) and input flags
/// (canonical buffering, echo, CR→NL translation) so that interactive prompts
/// and line-oriented I/O work correctly after a raw-mode session (e.g. VM boot).
#[cfg(unix)]
pub fn restore_terminal_cooked_mode() {
    let stderr = std::io::stderr();
    if let Ok(mut termios) = nix::sys::termios::tcgetattr(&stderr) {
        // Output: enable post-processing and NL→CRNL
        termios
            .output_flags
            .insert(nix::sys::termios::OutputFlags::OPOST);
        termios
            .output_flags
            .insert(nix::sys::termios::OutputFlags::ONLCR);
        // Input: canonical mode, echo, CR→NL translation
        termios
            .local_flags
            .insert(nix::sys::termios::LocalFlags::ICANON);
        termios
            .local_flags
            .insert(nix::sys::termios::LocalFlags::ECHO);
        termios
            .input_flags
            .insert(nix::sys::termios::InputFlags::ICRNL);
        let _ = nix::sys::termios::tcsetattr(&stderr, nix::sys::termios::SetArg::TCSANOW, &termios);
    }
}

pub fn parse_manifest_resources(manifest_path: &Path) -> (u32, u32) {
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

/// Remove workspace contents except installed dependency directories.
/// This lets us re-copy source files without losing `npm install` artifacts.
fn clean_workspace_preserving_deps(workspace: &Path) {
    let preserve = ["node_modules", "target", ".venv", "__pycache__"];
    if let Ok(entries) = std::fs::read_dir(workspace) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if preserve.iter().any(|s| *s == name_str.as_ref()) {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

pub fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
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
        let meta = std::fs::symlink_metadata(&src_path)
            .map_err(|e| format!("Failed to read metadata {}: {}", src_path.display(), e))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.file_type().is_dir() {
            std::fs::create_dir_all(&dst_path).map_err(|e| e.to_string())?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn build_libkrun_local_script(project: &ProjectInfo, prepared: bool) -> String {
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

pub fn build_local_env(
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

// ──────────────────────────────────────────────────────────────────────────────
// New functions for local-path worker support
// ──────────────────────────────────────────────────────────────────────────────

/// Returns `true` if `input` looks like a local filesystem path rather than
/// a registry name or OCI reference.
pub fn is_local_path(input: &str) -> bool {
    input.starts_with('.') || input.starts_with('/') || input.starts_with('~')
}

/// Reads the worker `name` from `iii.worker.yaml` inside `project_path`.
/// Falls back to the directory name if no manifest or no `name` field is found.
pub fn resolve_worker_name(project_path: &Path) -> String {
    let manifest_path = project_path.join(WORKER_MANIFEST);
    if manifest_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&manifest_path) {
            if let Ok(doc) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                if let Some(name) = doc.get("name").and_then(|n| n.as_str()) {
                    if !name.is_empty() {
                        return name.to_string();
                    }
                }
            }
        }
    }
    project_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("worker")
        .to_string()
}

/// Full flow for adding a local-path worker.
///
/// 1. Resolve path, validate, detect language, resolve name
/// 2. Check config.yaml for duplicates (--force to override)
/// 3. Prepare base rootfs, clone, copy project files
/// 4. Run setup+install scripts inside a libkrun VM
/// 5. Extract default config from iii.worker.yaml
/// 6. Append to config.yaml with `worker_path`
pub async fn handle_local_add(path: &str, force: bool, reset_config: bool, brief: bool) -> i32 {
    // 1. Resolve path to absolute
    let project_path = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} Invalid path '{}': {}", "error:".red(), path, e);
            return 1;
        }
    };

    // 2. Validate directory exists
    if !project_path.is_dir() {
        eprintln!(
            "{} '{}' is not a directory",
            "error:".red(),
            project_path.display()
        );
        return 1;
    }

    // 3. Detect language / project type
    let _project = match load_project_info(&project_path) {
        Some(p) => p,
        None => {
            eprintln!(
                "{} Could not detect project type in '{}'. \
                 Add iii.worker.yaml or use package.json/Cargo.toml/pyproject.toml.",
                "error:".red(),
                project_path.display()
            );
            return 1;
        }
    };

    // 4. Resolve worker name
    let worker_name = resolve_worker_name(&project_path);

    if !brief {
        eprintln!("  Adding local worker {}...", worker_name.bold());
    }

    // 5. Check if already exists in config.yaml
    if super::config_file::worker_exists(&worker_name) {
        if !force {
            eprintln!(
                "{} Worker '{}' already exists in config.yaml. Use --force to replace.",
                "error:".red(),
                worker_name
            );
            return 1;
        }
        // --force: stop if running, clear artifacts
        if super::managed::is_worker_running(&worker_name) {
            eprintln!("  Stopping running worker {}...", worker_name.bold());
            super::managed::handle_managed_stop(&worker_name, "0.0.0.0", 49134).await;
        }
        let freed = super::managed::delete_worker_artifacts(&worker_name);
        if freed > 0 {
            eprintln!(
                "  Cleared {:.1} MB of artifacts",
                freed as f64 / 1_048_576.0
            );
        }
        if reset_config {
            let _ = super::config_file::remove_worker(&worker_name);
        }
    }

    // 6. Extract default config from iii.worker.yaml
    let manifest_path = project_path.join(WORKER_MANIFEST);
    let config_yaml = if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|content| serde_yaml::from_str::<serde_yaml::Value>(&content).ok())
            .and_then(|doc| doc.get("config").cloned())
            .and_then(|v| serde_yaml::to_string(&v).ok())
    } else {
        None
    };

    // 7. Append to config.yaml with worker_path
    let abs_path_str = project_path.to_string_lossy();
    if let Err(e) = super::config_file::append_worker_with_path(
        &worker_name,
        &abs_path_str,
        config_yaml.as_deref(),
    ) {
        eprintln!("{} {}", "error:".red(), e);
        return 1;
    }

    // 8. Print success
    if brief {
        eprintln!("        {} {}", "\u{2713}".green(), worker_name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "\u{2713}".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
        );
        eprintln!("  {}  {}", "Path".cyan().bold(), abs_path_str.bold());

        // Auto-start if engine is running (skip if already running)
        if super::managed::is_engine_running() {
            if super::managed::is_worker_running(&worker_name) {
                eprintln!("  {} Worker already running", "\u{2713}".green());
            } else {
                let port = super::app::DEFAULT_PORT;
                let result = start_local_worker(&worker_name, &abs_path_str, port).await;
                if result == 0 {
                    eprintln!("  {} Worker auto-started", "\u{2713}".green());
                } else {
                    eprintln!(
                        "  {} Could not auto-start worker. Run `iii worker start {}` manually.",
                        "\u{26a0}".yellow(),
                        worker_name
                    );
                }
            }
        } else {
            eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
        }
    }

    0
}

/// Start a local-path worker VM.
///
/// Re-copies project files, builds env, and runs via libkrun.
pub async fn start_local_worker(worker_name: &str, worker_path: &str, port: u16) -> i32 {
    // Kill any stale process from a previous engine run
    super::managed::kill_stale_worker(worker_name).await;

    #[cfg(unix)]
    restore_terminal_cooked_mode();

    // 1. Validate worker_path directory exists
    let project_path = Path::new(worker_path);
    if !project_path.is_dir() {
        eprintln!(
            "{} Worker path '{}' does not exist or is not a directory",
            "error:".red(),
            worker_path
        );
        return 1;
    }

    // 2. Detect language
    let project = match load_project_info(project_path) {
        Some(p) => p,
        None => {
            eprintln!(
                "{} Could not detect project type in '{}'",
                "error:".red(),
                worker_path
            );
            return 1;
        }
    };

    let language = project.language.as_deref().unwrap_or("typescript");

    // 3. Ensure libkrunfw available
    if let Err(e) = super::firmware::download::ensure_libkrunfw().await {
        tracing::warn!(error = %e, "failed to ensure libkrunfw");
    }

    if !super::worker_manager::libkrun::libkrun_available() {
        eprintln!(
            "{} No runtime available.\n  \
             Rebuild with --features embed-libkrunfw or place libkrunfw in ~/.iii/lib/",
            "error:".red()
        );
        return 1;
    }

    // 4. Prepare managed dir — clone rootfs on first start
    let managed_dir = match dirs::home_dir() {
        Some(h) => h.join(".iii").join("managed").join(worker_name),
        None => {
            eprintln!("{} Cannot determine home directory", "error:".red());
            return 1;
        }
    };

    if !managed_dir.exists() {
        eprintln!("  Preparing sandbox...");
        let base_rootfs = match super::worker_manager::oci::prepare_rootfs(language).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{} {}", "error:".red(), e);
                return 1;
            }
        };
        if let Err(e) = clone_rootfs(&base_rootfs, &managed_dir) {
            eprintln!("{} Failed to create project rootfs: {}", "error:".red(), e);
            return 1;
        }
    }

    // 5. Re-copy project files to workspace (fresh source each start)
    //    Preserve installed dependency dirs (node_modules, target, .venv)
    //    so setup/install doesn't re-run every time.
    let workspace = managed_dir.join("workspace");
    let ws = workspace.clone();
    let pp = project_path.to_path_buf();
    let copy_result = tokio::task::spawn_blocking(move || {
        if ws.exists() {
            clean_workspace_preserving_deps(&ws);
        }
        std::fs::create_dir_all(&ws).ok();
        copy_dir_contents(&pp, &ws)
    })
    .await;

    match copy_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("{} Failed to copy project to rootfs: {}", "error:".red(), e);
            return 1;
        }
        Err(e) => {
            eprintln!("{} Copy task panicked: {}", "error:".red(), e);
            return 1;
        }
    }

    // 5. Check .iii-prepared marker
    let prepared_marker = managed_dir.join("var").join(".iii-prepared");
    let is_prepared = prepared_marker.exists();

    if is_prepared {
        eprintln!(
            "  {} Using cached deps {}",
            "\u{2713}".green(),
            "(use --force to reinstall)".dimmed()
        );
    }

    // 6. Build env with engine URL + OCI env + config.yaml env
    let engine_url = engine_url_for_runtime("libkrun", "0.0.0.0", port, &None);
    let config_env = super::config_file::get_worker_config_as_env(worker_name);

    let mut combined_project_env = project.env.clone();
    for (k, v) in &config_env {
        combined_project_env.insert(k.clone(), v.clone());
    }

    let mut env = build_local_env(&engine_url, &combined_project_env);

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

    // 7. Build script
    let script = build_libkrun_local_script(&project, is_prepared);

    let script_path = managed_dir.join("tmp").join("iii-dev-run.sh");
    std::fs::create_dir_all(managed_dir.join("tmp")).ok();
    if let Err(e) = std::fs::write(&script_path, &script) {
        eprintln!("{} Failed to write run script: {}", "error:".red(), e);
        return 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
    }

    // 8. Copy iii-init if needed
    let init_path = match super::firmware::download::ensure_init_binary().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} Failed to provision iii-init: {}", "error:".red(), e);
            return 1;
        }
    };

    if !iii_filesystem::init::has_init() {
        let dest = managed_dir.join("init.krun");
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

    // 9. Run via libkrun
    let manifest_path = project_path.join(WORKER_MANIFEST);
    let (vcpus, ram) = parse_manifest_resources(&manifest_path);

    let exec_path = "/bin/sh";
    let args = vec![
        "-c".to_string(),
        "cd /workspace && exec bash /tmp/iii-dev-run.sh".to_string(),
    ];

    super::worker_manager::libkrun::run_dev(
        language,
        worker_path,
        exec_path,
        &args,
        env,
        vcpus,
        ram,
        managed_dir,
        true,
        worker_name,
    )
    .await
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_local_path_detects_relative() {
        assert!(is_local_path("."));
        assert!(is_local_path(".."));
        assert!(is_local_path("./my-worker"));
        assert!(is_local_path("../sibling"));
        assert!(is_local_path("/absolute/path"));
        assert!(is_local_path("~/projects/worker"));
    }

    #[test]
    fn is_local_path_rejects_names_and_oci() {
        assert!(!is_local_path("pdfkit"));
        assert!(!is_local_path("pdfkit@1.0.0"));
        assert!(!is_local_path("ghcr.io/org/worker:tag"));
    }

    #[test]
    fn resolve_worker_name_from_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let yaml = "name: my-cool-worker\nruntime:\n  language: typescript\n";
        std::fs::write(dir.path().join(WORKER_MANIFEST), yaml).unwrap();
        let name = resolve_worker_name(dir.path());
        assert_eq!(name, "my-cool-worker");
    }

    #[test]
    fn resolve_worker_name_falls_back_to_dir_name() {
        let dir = tempfile::tempdir().unwrap();
        // No iii.worker.yaml — should fall back to directory name
        let name = resolve_worker_name(dir.path());
        let expected = dir
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(name, expected);
    }

    #[test]
    fn build_libkrun_local_script_first_run() {
        let project = ProjectInfo {
            name: "test".to_string(),
            language: Some("typescript".to_string()),
            setup_cmd: "apt-get install nodejs".to_string(),
            install_cmd: "npm install".to_string(),
            run_cmd: "node server.js".to_string(),
            env: HashMap::new(),
        };
        let script = build_libkrun_local_script(&project, false);
        assert!(script.contains("apt-get install nodejs"));
        assert!(script.contains("npm install"));
        assert!(script.contains("node server.js"));
        assert!(script.contains(".iii-prepared"));
    }

    #[test]
    fn build_libkrun_local_script_prepared() {
        let project = ProjectInfo {
            name: "test".to_string(),
            language: Some("typescript".to_string()),
            setup_cmd: "apt-get install nodejs".to_string(),
            install_cmd: "npm install".to_string(),
            run_cmd: "node server.js".to_string(),
            env: HashMap::new(),
        };
        let script = build_libkrun_local_script(&project, true);
        assert!(!script.contains("apt-get install nodejs"));
        assert!(!script.contains("npm install"));
        assert!(script.contains("node server.js"));
    }

    #[test]
    fn build_local_env_sets_engine_urls() {
        let env = build_local_env("ws://localhost:49134", &HashMap::new());
        assert_eq!(env.get("III_ENGINE_URL").unwrap(), "ws://localhost:49134");
        assert_eq!(env.get("III_URL").unwrap(), "ws://localhost:49134");
    }

    #[test]
    fn build_local_env_preserves_custom_env() {
        let mut project_env = HashMap::new();
        project_env.insert("CUSTOM".to_string(), "value".to_string());
        let env = build_local_env("ws://localhost:49134", &project_env);
        assert_eq!(env.get("CUSTOM").unwrap(), "value");
        assert_eq!(env.get("III_ENGINE_URL").unwrap(), "ws://localhost:49134");
        assert_eq!(env.get("III_URL").unwrap(), "ws://localhost:49134");
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

    #[test]
    fn clean_workspace_preserving_deps_keeps_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();

        // Create dep dirs that should be preserved
        std::fs::create_dir_all(ws.join("node_modules/pkg")).unwrap();
        std::fs::write(ws.join("node_modules/pkg/index.js"), "mod").unwrap();
        std::fs::create_dir_all(ws.join("target/debug")).unwrap();
        std::fs::write(ws.join("target/debug/bin"), "elf").unwrap();
        std::fs::create_dir_all(ws.join(".venv/lib")).unwrap();
        std::fs::write(ws.join(".venv/lib/site.py"), "py").unwrap();
        std::fs::create_dir_all(ws.join("__pycache__")).unwrap();
        std::fs::write(ws.join("__pycache__/mod.pyc"), "pyc").unwrap();

        // Create source files/dirs that should be removed
        std::fs::write(ws.join("main.ts"), "console.log()").unwrap();
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/lib.ts"), "export {}").unwrap();

        clean_workspace_preserving_deps(ws);

        // Dep dirs preserved
        assert!(ws.join("node_modules/pkg/index.js").exists());
        assert!(ws.join("target/debug/bin").exists());
        assert!(ws.join(".venv/lib/site.py").exists());
        assert!(ws.join("__pycache__/mod.pyc").exists());

        // Source files/dirs removed
        assert!(!ws.join("main.ts").exists());
        assert!(!ws.join("src").exists());
    }

    #[test]
    fn clean_workspace_preserving_deps_handles_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Should not panic on empty directory
        clean_workspace_preserving_deps(dir.path());
        assert!(dir.path().exists());
    }

    #[test]
    fn clean_workspace_preserving_deps_handles_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("nope");
        // Should not panic on nonexistent directory
        clean_workspace_preserving_deps(&gone);
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
}
