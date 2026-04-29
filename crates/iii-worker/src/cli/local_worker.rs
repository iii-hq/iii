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
pub fn clean_workspace_preserving_deps(workspace: &Path) {
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

/// Build the boot script that `iii-init` will exec as the worker command.
///
/// No supervisor wrapping: the in-VM `iii-init` binary absorbs the
/// supervisor role (see `iii_init::supervisor`) and opens the control
/// channel itself when the host sets `III_CONTROL_PORT`. That removes
/// the separate `/opt/iii/supervisor` binary and its install plumbing
/// that this function used to emit as `exec /opt/iii/supervisor ...`.
pub fn build_libkrun_local_script(project: &ProjectInfo, prepared: bool) -> String {
    let env_exports = build_env_exports(&project.env);
    let mut parts: Vec<String> = Vec::new();

    parts.push("set -e".to_string());
    parts.push("export HOME=${HOME:-/root}".to_string());
    parts.push("export PATH=/usr/local/bin:/usr/bin:/bin:$PATH".to_string());
    parts.push("export LANG=${LANG:-C.UTF-8}".to_string());

    // Workspace strategy: host project is mounted live at /workspace via
    // virtiofs (by iii-init from III_VIRTIOFS_MOUNTS). Source edits flow
    // through naturally. Language dep dirs (node_modules, .venv, target,
    // etc.) are bind-mounted from the rootfs so their writes stay VM-local
    // and never hit the host repo. The rootfs-backed bind targets persist
    // across VM restarts, so npm install / pip install caches survive.
    //
    // We tried overlayfs with virtiofs as lower and hit errno 102 (kernel
    // copy-up path fails for PassthroughFs reads). Bind-mounts sidestep
    // that entirely.
    //
    // Tradeoff: each dep dir becomes a mountpoint, and mount(2) requires
    // the target to exist. If the host repo doesn't already have one of
    // these dirs, an empty directory appears on the host (standard
    // .gitignore entry in every dev setup).
    // Verify /workspace is an actual virtiofs mountpoint, not just a bare
    // directory. iii-init's mount_virtiofs_shares() calls mkdir_p on the
    // guest path before mounting and swallows mount failures as warnings,
    // so a silent virtiofs failure leaves /workspace existing-but-unmounted.
    // A plain `-d` check would pass and we'd bind-mount deps onto an empty
    // rootfs dir -- writes would leak onto rootfs instead of the host repo.
    parts.push(
        r#"if ! { mountpoint -q /workspace 2>/dev/null || awk '$5 == "/workspace" && / - virtiofs /' /proc/self/mountinfo | grep -q .; }; then
  echo "iii: ERROR /workspace is not a virtiofs mountpoint (share missing or mount failed)" >&2
  echo "--- III_VIRTIOFS_MOUNTS=${III_VIRTIOFS_MOUNTS:-<unset>} ---" >&2
  cat /proc/self/mountinfo >&2 2>/dev/null || cat /proc/mounts >&2 2>/dev/null || mount >&2
  exit 1
fi
DEPS_ROOT=/var/iii/deps
for d in node_modules .venv target dist __pycache__ .pytest_cache .next; do
  mkdir -p "$DEPS_ROOT/$d"
  if [ ! -e "/workspace/$d" ]; then
    mkdir "/workspace/$d"
  elif [ ! -d "/workspace/$d" ]; then
    echo "iii: WARN /workspace/$d exists but is not a directory, skipping bind" >&2
    continue
  fi
  mount --bind "$DEPS_ROOT/$d" "/workspace/$d"
done
cd /workspace
echo "iii: workspace ready; deps mounted VM-local from $DEPS_ROOT" >&2"#
            .to_string(),
    );

    parts.push("echo $$ > /sys/fs/cgroup/worker/cgroup.procs 2>/dev/null || true".to_string());

    // Host source changes are handled by the host-side `__watch-source`
    // sidecar (see source_watcher.rs), which restarts the whole VM on
    // change. In-VM watchers are not expected to detect host edits, so
    // no polling env vars are exported here — they'd just add overhead
    // and couldn't help tsx 4.x anyway (tsx uses fs.watch with no
    // polling fallback, and doesn't depend on chokidar).

    if !prepared {
        if !project.setup_cmd.is_empty() {
            parts.push(project.setup_cmd.clone());
        }
        if !project.install_cmd.is_empty() {
            parts.push(project.install_cmd.clone());
        }
        parts.push("mkdir -p /var && touch /var/.iii-prepared".to_string());
    }

    // Exec the user command directly. When the host has configured a
    // control port (III_CONTROL_PORT env set by vm_boot), `iii-init`
    // enters supervisor mode and wraps this command internally with
    // restart/shutdown RPC handling. When no control port is set,
    // iii-init takes its legacy path and just supervises the one
    // process until it exits.
    parts.push(format!("{} && exec {}", env_exports, project.run_cmd));
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

/// Build the virtiofs mount list for a local-path worker: the host project
/// dir is shared live at guest `/workspace`. Returns `(host_path, guest_path)`
/// pairs suitable for `libkrun::run_dev`.
pub fn build_local_mounts(project_path: &Path) -> Vec<(String, String)> {
    vec![(
        project_path.to_string_lossy().into_owned(),
        "/workspace".to_string(),
    )]
}

/// Returns `true` if `input` looks like a local filesystem path rather than
/// a registry name or OCI reference.
pub fn is_local_path(input: &str) -> bool {
    input.starts_with('.') || input.starts_with('/') || input.starts_with('~')
}

/// Reads the worker `name` from `iii.worker.yaml` inside `project_path`.
/// Falls back to the directory name if no manifest or no `name` field is found.
pub fn resolve_worker_name(project_path: &Path) -> String {
    let manifest_path = project_path.join(WORKER_MANIFEST);
    if manifest_path.exists()
        && let Ok(content) = std::fs::read_to_string(&manifest_path)
        && let Ok(doc) = serde_yaml::from_str::<serde_yaml::Value>(&content)
        && let Some(name) = doc.get("name").and_then(|n| n.as_str())
        && !name.is_empty()
    {
        return name.to_string();
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
pub async fn handle_local_add(
    path: &str,
    force: bool,
    reset_config: bool,
    brief: bool,
    wait: bool,
) -> i32 {
    // 1. Resolve path to absolute
    let project_path = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "{} Cannot resolve path '{}': {}\n  \
                 Fix: pass a path that exists, e.g. `iii worker add ./my-worker`.\n  \
                 If the directory should exist, check spelling and current working dir.",
                "error:".red(),
                path,
                e
            );
            return 1;
        }
    };

    // 2. Validate directory exists
    if !project_path.is_dir() {
        eprintln!(
            "{} '{}' exists but is not a directory.\n  \
             Fix: point at the worker's project directory, not a file.",
            "error:".red(),
            project_path.display()
        );
        return 1;
    }

    // 3. Detect language / project type
    let project = match load_project_info(&project_path) {
        Some(p) => p,
        None => {
            eprintln!(
                "{} No project manifest detected in '{}'.\n  \
                 Looked for: iii.worker.yaml, package.json, Cargo.toml, pyproject.toml.\n  \
                 Fix: run from inside your worker project, or create iii.worker.yaml:\n      \
                     name: my-worker\n      \
                     runtime:\n        \
                       kind: typescript\n      \
                     command: [\"node\", \"src/index.js\"]",
                "error:".red(),
                project_path.display()
            );
            return 1;
        }
    };

    if let Err(msg) = project.validate() {
        eprintln!(
            "{} Project manifest is invalid: {}\n  \
             Fix: see https://motia.dev/docs/iii/worker-manifest for the schema.",
            "error:".red(),
            msg
        );
        return 1;
    }

    // 4. Resolve worker name
    let worker_name = resolve_worker_name(&project_path);

    // Defense in depth: the manifest's `name:` field is attacker-
    // reachable via a hand-edited or copy-pasted iii.worker.yaml, and
    // it flows into filesystem operations below (delete_worker_artifacts
    // does remove_dir_all on ~/.iii/workers/<name> and ~/.iii/managed/<name>).
    // `Path::join` preserves `..` components, so a name like
    // `../../../some_dir` produces a real traversal path. The primary
    // validator at `append_worker_with_path` normally blocks such names
    // from entering config.yaml, but re-validate here so a stale or
    // malicious manifest can't slip past on the --force path.
    if let Err(msg) = super::registry::validate_worker_name(&worker_name) {
        eprintln!(
            "{} Worker name from manifest is invalid: {}\n  \
             Names must be single path segments with no `/`, `\\`, `..`, or shell metachars.",
            "error:".red(),
            msg
        );
        return 1;
    }

    // 5. Check if already exists in config.yaml
    if super::config_file::worker_exists(&worker_name) {
        if !force {
            eprintln!(
                "{} Worker '{}' is already in config.yaml.\n  \
                 Fix options:\n    \
                   - Keep it: `iii worker status {}` to see how it's doing.\n    \
                   - Replace it: rerun with --force (stops VM, clears artifacts).\n    \
                   - Wipe the config entry too: --force --reset-config.",
                "error:".red(),
                worker_name,
                worker_name
            );
            return 1;
        }
        // --force: stop if running, clear artifacts
        if super::managed::is_worker_running(&worker_name) {
            eprintln!("  Stopping running worker {}...", worker_name.bold());
            super::managed::handle_managed_stop(&worker_name).await;
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

    let manifest_path = project_path.join(WORKER_MANIFEST);

    // 5b. Resolve + install declared manifest dependencies BEFORE we touch
    //     config.yaml. A failure here leaves the local worker NOT in
    //     config.yaml and iii.lock unchanged — retry is just retry, no
    //     `--force` dance required. This is load-bearing: reversing this
    //     order recreates the "already in config.yaml" rerun trap.
    let declared_deps = match super::project::load_manifest_dependencies(&manifest_path) {
        Ok(deps) => deps,
        Err(e) => {
            eprintln!(
                "{} {} in {}\n  \
                 Fix: correct the `dependencies:` block and rerun \
                 `iii worker add {}`.",
                "error:".red(),
                e,
                manifest_path.display(),
                path,
            );
            return 1;
        }
    };
    if !declared_deps.is_empty()
        && let Err(e) = super::managed::install_manifest_dependencies(&declared_deps, brief).await
    {
        eprintln!(
            "{} {}\n  \
             Nothing was written to config.yaml or iii.lock. Rerun \
             `iii worker add {}` after fixing the failure.",
            "error:".red(),
            e,
            path,
        );
        return 1;
    }

    // 6. Extract default config from iii.worker.yaml
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
        eprintln!(
            "{} Failed to update config.yaml: {}\n  \
             Fix: check that config.yaml is writable and valid YAML.",
            "error:".red(),
            e
        );
        return 1;
    }

    // 8. Decide the output shape. The worker is queued in config.yaml but has
    //    NOT booted yet — never claim success with ✓. Output depends on three
    //    axes: brief (multi-add row), engine state, and whether the caller
    //    asked us to --wait.
    let engine_running = super::managed::is_engine_running();

    if brief {
        // Multi-worker add: one short row per worker. ⟳ if the engine will
        // pick it up, ⚠ if it won't.
        let glyph = if engine_running {
            "\u{27F3}"
        } else {
            "\u{26A0}"
        };
        eprintln!("        {} {}", glyph.cyan(), worker_name.bold());
        return 0;
    }

    // 9. --wait: skip the "follow along" nudge entirely (we ARE following
    //    along now), drop straight into the live snapshot, and print a
    //    one-line closer with elapsed time.
    if wait {
        if !engine_running {
            eprintln!(
                "\n  {} Added {} ({}) to config.yaml, but the engine isn't running.\n  \
                 --wait cannot observe it boot — run `iii start` in another terminal first.",
                "\u{26A0}".yellow(),
                worker_name.bold(),
                "local".dimmed()
            );
            return 0;
        }

        eprintln!(
            "\n  {} Adding {} ({})...",
            "→".cyan(),
            worker_name.bold(),
            "local".dimmed()
        );

        let started = std::time::Instant::now();
        let port = super::config_file::manager_port();
        let final_status = super::status::watch_until_ready(
            &worker_name,
            Some(std::time::Duration::from_secs(120)),
            port,
        )
        .await;
        let elapsed = started.elapsed();

        match final_status.phase {
            super::status::Phase::Ready => {
                eprintln!("  {} ready in {:.1}s", "✓".green(), elapsed.as_secs_f64());
                return 0;
            }
            _ => {
                eprintln!(
                    "  {} not ready after {:.0}s (worker is still queued in config.yaml).\n  \
                       Keep watching: iii worker status {}\n  \
                       Check logs:    iii worker logs {} -f",
                    "⚠".yellow(),
                    elapsed.as_secs_f64(),
                    worker_name,
                    worker_name
                );
                return 2;
            }
        }
    }

    // 10. Non-wait path (user passed --no-wait): two-branch tight message
    //     depending on engine state. Hints drop `--watch` because status
    //     live-refreshes by default now.
    if engine_running {
        eprintln!(
            "\n  {} Added {} ({}) — queued in config.yaml.\n  \
             Watch it boot: iii worker status {}\n  \
             Tail logs:     iii worker logs {} -f",
            "→".cyan(),
            worker_name.bold(),
            "local".dimmed(),
            worker_name,
            worker_name
        );
    } else {
        eprintln!(
            "\n  {} Added {} ({}) to config.yaml, but the engine isn't running.\n  \
             Start it:  iii start\n  \
             Then:      iii worker status {}",
            "\u{26A0}".yellow(),
            worker_name.bold(),
            "local".dimmed(),
            worker_name
        );
    }

    // Stash the absolute path in a debug-visible spot without cluttering the
    // happy-path output. Users who need it can run `iii worker status`.
    tracing::debug!(worker = %worker_name, path = %abs_path_str, "local worker queued");

    0
}

/// Return `true` when `managed_dir` needs a rootfs clone before VM boot.
///
/// The predicate is `!managed_dir.join("bin").exists()` — every OCI base
/// image we ship populates `/bin`, and none of the failure-mode artifacts
/// (iii-init runtime mkdirs for /dev /proc /sys /etc /tmp /run, our own
/// side-effect writes under /opt /workspace, stray vm.pid / watch.pid, a
/// leftover `/rootfs` subdir from an older codepath) do. Plain
/// `!managed_dir.exists()` is insufficient: a half-start that created
/// runtime mountpoints then died would falsely register as "already
/// prepared" and surface as a confusing `supervisor spawn_initial failed:
/// No such file or directory` from iii-init once the VM tried to exec
/// `/bin/sh`. Extracted as a named helper so the regression tests can
/// exercise the exact predicate the start path uses.
fn needs_rootfs_clone(managed_dir: &std::path::Path) -> bool {
    !managed_dir.join("bin").exists()
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

    if let Err(msg) = project.validate() {
        eprintln!("{} {}", "error:".red(), msg);
        return 1;
    }

    // Treat an empty/whitespace `kind:` field in the manifest the same
    // as missing — otherwise a YAML typo like `kind: ""` would silently
    // pass an empty string through the oci/inferred-script lookups and
    // fall into their `_` defaults, giving the wrong base image instead
    // of the documented "typescript" default.
    let kind = project
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .unwrap_or("typescript");

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

    // Detect "managed_dir exists but the rootfs was never cloned here"
    // by probing for `/bin` — every OCI base image we ship has it, and
    // neither iii-init's runtime mount dirs (/dev, /proc, /sys, /etc,
    // /tmp, /run) nor our own side-effect writes (/opt, /workspace,
    // vm.pid, watch.pid, /rootfs from an older codepath) populate it.
    //
    // Matches the `path.join("bin").exists()` idiom in
    // `worker_manager/oci.rs::prepare_rootfs`. A plain `managed_dir.exists()`
    // check is too weak: any prior half-start (VM booted, iii-init mkdir'd
    // /dev/proc/sys, then died before finishing) leaves the dir present
    // but FHS-less, so the next start would skip the clone and boot a VM
    // with no `/bin/sh` — which surfaces as a confusing
    // "supervisor spawn_initial failed: No such file or directory" from
    // iii-init rather than a clear "rootfs never got populated" error.
    //
    // Self-heal: if the marker is missing but the dir exists, wipe it
    // first so `cp -c -a SRC DEST` (rootfs::clone_rootfs) creates DEST
    // fresh as a copy of SRC. Without the wipe, cp's "copy INTO existing
    // dir" semantics would nest the rootfs at `managed_dir/python/`
    // instead of at `managed_dir/`. Anything the user cares about under
    // managed_dir is regenerated on boot (logs, vm.pid, dep caches come
    // back after pip install / npm install).
    if needs_rootfs_clone(&managed_dir) {
        eprintln!("  Preparing sandbox...");
        if managed_dir.exists()
            && let Err(e) = std::fs::remove_dir_all(&managed_dir)
        {
            eprintln!(
                "{} Failed to clear stale managed dir {}: {}",
                "error:".red(),
                managed_dir.display(),
                e
            );
            return 1;
        }
        let base_rootfs =
            match super::worker_manager::oci::prepare_rootfs(kind, project.base_image.as_deref())
                .await
            {
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

    // 5. Host project dir is shared live into the VM via virtiofs at
    //    /mnt/host-workspace; /workspace is assembled inside the VM as an
    //    overlay on top of that (see build_libkrun_local_script). No copy
    //    step — host edits flow through immediately, VM-side writes never
    //    touch the host.
    //
    //    Ensure the overlay mountpoint dir exists in the rootfs so the init
    //    doesn't fail cd-ing into it before the overlay is assembled.
    let workspace_dir = managed_dir.join("workspace");
    if let Err(e) = std::fs::create_dir_all(&workspace_dir) {
        eprintln!(
            "{} Failed to create workspace dir {}: {}",
            "error:".red(),
            workspace_dir.display(),
            e
        );
        return 1;
    }

    // 5. Check .iii-prepared marker. Silent when true — the boot script
    //    skips setup_cmd/install_cmd and nothing user-visible is happening
    //    in the fast path. Printing a "Using cached deps" banner every
    //    start made it look like install was running every restart (it
    //    wasn't), which confused users reading watcher.log tails.
    let prepared_marker = managed_dir.join("var").join(".iii-prepared");
    let is_prepared = prepared_marker.exists();

    // 6. Build env with engine URL + OCI env + config.yaml env
    let engine_url = engine_url_for_runtime("libkrun", "0.0.0.0", port, &None);
    let config_env = super::config_file::get_worker_config_as_env(worker_name);

    let mut combined_project_env = project.env.clone();
    for (k, v) in &config_env {
        combined_project_env.insert(k.clone(), v.clone());
    }

    let mut env = build_local_env(&engine_url, &combined_project_env);

    let base_rootfs =
        match super::worker_manager::oci::prepare_rootfs(kind, project.base_image.as_deref()).await
        {
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

    // 7. Build boot script. No supervisor wrap is emitted — the in-VM
    //    `iii-init` binary absorbs that role (see iii_init::supervisor).
    //    Fast-restart is enabled whenever vm_boot wires the control port,
    //    which sets `III_CONTROL_PORT` in the guest env for iii-init.
    let script = build_libkrun_local_script(&project, is_prepared);

    let script_path = managed_dir.join("opt").join("iii").join("dev-run.sh");
    std::fs::create_dir_all(managed_dir.join("opt").join("iii")).ok();
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
    // The script sets up the overlay at /workspace; no pre-cd needed (and
    // /workspace is empty until overlay mounts, so cd-before-exec would be
    // racy).
    let args = vec![
        "-c".to_string(),
        "exec bash /opt/iii/dev-run.sh".to_string(),
    ];

    let mounts = build_local_mounts(project_path);

    let managed_dir_for_watcher = managed_dir.clone();
    let exit_code = super::worker_manager::libkrun::run_dev(
        kind,
        worker_path,
        exec_path,
        &args,
        env,
        vcpus,
        ram,
        managed_dir,
        true,
        worker_name,
        &mounts,
    )
    .await;

    // Spawn the host-side source watcher sidecar. Virtiofs doesn't
    // propagate inotify, so in-VM watchers (tsx watch, node --watch,
    // cargo watch, etc.) don't see host edits — we watch from the host
    // and re-invoke `iii-worker start` on change to kill+restart.
    //
    // Only spawn after the VM was successfully started; otherwise a
    // watcher fire would race into kill_stale_worker against nothing.
    if exit_code == 0
        && let Err(e) =
            spawn_source_watcher(worker_name, project_path, &managed_dir_for_watcher).await
    {
        eprintln!(
            "  {} source watcher failed to start: {}. Source edits will not auto-restart.",
            "warning:".yellow(),
            e
        );
    }

    exit_code
}

// Sidecar pidfile writes delegate to super::pidfile::write_pid_file for
// unified hardening across all managed-dir pidfiles. See that module
// for rationale; in short, O_NOFOLLOW + 0o600 on Unix so a symlink
// pre-planted at watch.pid can't redirect our write to a sensitive
// target.
use super::pidfile::write_pid_file;

/// Spawn the hidden `__watch-source` sidecar process, detached, with
/// its PID recorded so `kill_stale_worker` can reap it on stop.
async fn spawn_source_watcher(
    worker_name: &str,
    project_path: &Path,
    managed_dir: &Path,
) -> std::io::Result<()> {
    use std::path::PathBuf;

    // If a watcher is already running for this worker (stale PID file
    // from a crashed previous start), reap it first so we don't stack
    // sidecars that fight over the same project dir. Delegates to the
    // grace-period reaper in managed.rs so both reap paths stay in sync.
    super::managed::reap_source_watcher(worker_name).await;

    let pid_file = managed_dir.join("watch.pid");
    let self_exe = std::env::current_exe()?;
    let logs_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".iii/logs")
        .join(worker_name);
    std::fs::create_dir_all(&logs_dir)?;
    // Lock log dir to 0o700 so another local user can't traverse in and
    // plant a symlink at watcher.log pointing at, e.g., ~/.ssh/authorized_keys.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&logs_dir, std::fs::Permissions::from_mode(0o700));
    }
    let log_path = logs_dir.join("watcher.log");
    let mut log_opts = std::fs::OpenOptions::new();
    log_opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NOFOLLOW on the final path component so a pre-planted
        // symlink (by another local user with dir-traverse access)
        // can't redirect our appends to an arbitrary file.
        log_opts.custom_flags(nix::libc::O_NOFOLLOW);
        // Create as 0o600 — owner-only.
        log_opts.mode(0o600);
    }
    let log_file = log_opts.open(&log_path)?;
    let log_file2 = log_file.try_clone()?;

    let project_abs =
        std::fs::canonicalize(project_path).unwrap_or_else(|_| project_path.to_path_buf());

    let mut cmd = std::process::Command::new(&self_exe);
    cmd.arg("__watch-source")
        .arg("--worker")
        .arg(worker_name)
        .arg("--project")
        .arg(&project_abs)
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(log_file2);

    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            nix::unistd::setsid().map_err(std::io::Error::other)?;
            Ok(())
        });
    }

    let child = cmd.spawn()?;
    let pid = child.id();
    write_pid_file(&pid_file, pid);

    eprintln!(
        "  {} source watcher online (pid: {})",
        "\u{2713}".green(),
        pid
    );
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_local_mounts_maps_project_to_workspace() {
        let mounts = build_local_mounts(Path::new("/abs/host/project"));
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].0, "/abs/host/project");
        assert_eq!(mounts[0].1, "/workspace");
    }

    #[test]
    fn build_local_mounts_preserves_relative_path_string() {
        // Path stringification is lossy on non-UTF8, but for typical macOS/Linux
        // paths we round-trip exactly. Documents intended behavior.
        let mounts = build_local_mounts(Path::new("./relative/path"));
        assert_eq!(mounts[0].0, "./relative/path");
        assert_eq!(mounts[0].1, "/workspace");
    }

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
        let yaml = "name: my-cool-worker\nruntime:\n  kind: typescript\n";
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
            kind: Some("typescript".to_string()),
            setup_cmd: "apt-get install nodejs".to_string(),
            install_cmd: "npm install".to_string(),
            run_cmd: "node server.js".to_string(),
            env: HashMap::new(),
            base_image: None,
        };
        let script = build_libkrun_local_script(&project, false);
        assert!(script.contains("apt-get install nodejs"));
        assert!(script.contains("npm install"));
        assert!(script.contains("node server.js"));
        assert!(script.contains(".iii-prepared"));
        assert!(script.contains("mount --bind"));
        assert!(script.contains("/var/iii/deps"));
        assert!(script.contains("node_modules"));
        // Polling env vars were removed — the host-side __watch-source
        // sidecar handles reload-on-edit by restarting the whole VM.
        assert!(!script.contains("CHOKIDAR_USEPOLLING"));
        assert!(!script.contains("WATCHFILES_FORCE_POLLING"));
        assert!(!script.contains("TSC_WATCHFILE"));
    }

    #[test]
    fn build_libkrun_local_script_prepared() {
        let project = ProjectInfo {
            name: "test".to_string(),
            kind: Some("typescript".to_string()),
            setup_cmd: "apt-get install nodejs".to_string(),
            install_cmd: "npm install".to_string(),
            run_cmd: "node server.js".to_string(),
            env: HashMap::new(),
            base_image: None,
        };
        let script = build_libkrun_local_script(&project, true);
        assert!(!script.contains("apt-get install nodejs"));
        assert!(!script.contains("npm install"));
        assert!(script.contains("node server.js"));
        assert!(script.contains("mount --bind"));
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

    // Symlink-defense + 0o600 mode tests moved to super::pidfile where
    // the shared implementation lives.

    /// Regression test for the "managed_dir exists but is missing /bin"
    /// failure mode. A half-populated managed_dir — created by a prior
    /// iii-init boot that mkdir'd /dev/proc/sys but died before the
    /// rootfs was cloned, or by an older codepath that placed the
    /// rootfs at `managed_dir/rootfs/` rather than at `managed_dir/` —
    /// must be detected as "needs re-clone" by `needs_rootfs_clone`.
    ///
    /// The pre-fix guard (`!managed_dir.exists()`) skipped the clone
    /// and let the VM boot against an FHS-less rootfs, surfacing as a
    /// confusing `supervisor spawn_initial failed: No such file or
    /// directory (os error 2)` from iii-init because `/bin/sh` didn't
    /// exist. This test calls the extracted helper so a refactor that
    /// weakens the predicate back to `!managed_dir.exists()` — or any
    /// other too-permissive check — fails here.
    #[test]
    fn needs_rootfs_clone_true_for_half_populated_managed_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("todo-worker-python");

        // Simulate iii-init-style runtime dirs (created on prior boot)
        // + a stale /rootfs subdir from an older codepath — the exact
        // layout the user hit in the wild.
        for d in [
            "dev",
            "proc",
            "sys",
            "etc",
            "tmp",
            "run",
            "opt",
            "workspace",
            "rootfs/bin",
            "rootfs/usr",
        ] {
            std::fs::create_dir_all(managed.join(d)).unwrap();
        }
        std::fs::write(managed.join("vm.pid"), "12345").unwrap();
        std::fs::write(managed.join("watch.pid"), "12346").unwrap();

        // Dir exists — the old too-weak guard would skip the clone.
        assert!(managed.exists());
        // But the helper must flag this as "rootfs never got populated
        // here, must re-clone" — otherwise the VM boots against an
        // FHS-less tree and iii-init dies trying to exec /bin/sh.
        assert!(
            needs_rootfs_clone(&managed),
            "half-populated managed_dir must register as `needs clone`"
        );
    }

    /// Complement: a fully-cloned managed_dir (with `/bin` present)
    /// must be recognized as ready so we don't nuke a healthy sandbox
    /// on every start and pay the re-clone cost. Exercises the helper
    /// directly for the same reason as the sibling test above.
    #[test]
    fn needs_rootfs_clone_false_for_fully_cloned_managed_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("todo-worker-python");
        std::fs::create_dir_all(managed.join("bin")).unwrap();
        std::fs::create_dir_all(managed.join("usr")).unwrap();
        assert!(
            !needs_rootfs_clone(&managed),
            "fully-cloned managed_dir must NOT trigger a re-clone"
        );
    }

    /// Absence case: `needs_rootfs_clone(nonexistent_path)` must be true.
    /// Covers the first-start path where the parent `~/.iii/managed/`
    /// exists but the per-worker subdir has never been created.
    #[test]
    fn needs_rootfs_clone_true_when_managed_dir_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let managed = tmp.path().join("never-started");
        assert!(!managed.exists());
        assert!(
            needs_rootfs_clone(&managed),
            "absent managed_dir must register as `needs clone`"
        );
    }
}
