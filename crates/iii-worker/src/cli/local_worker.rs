// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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

/// Best-effort read of `resources.cpus`/`resources.memory` for VM sizing.
/// Lenient by design: any read/parse/shape failure falls back to the defaults
/// (2 vCPUs, 2048 MiB) — strictness lives in the add/start validation and
/// `worker::validate`, not in this sizing probe. Backed by the typed
/// [`super::worker_manifest::WorkerManifest`] so add, start, and validate all
/// read resources through one schema (previously this used the `serde_yml`
/// fork while everything else used `serde_yaml`).
pub fn parse_manifest_resources(manifest_path: &Path) -> (u32, u32) {
    let default = (2, 2048);
    let Ok(Some(doc)) = super::project::read_manifest_doc(manifest_path) else {
        return default;
    };
    let Ok(manifest) = super::worker_manifest::WorkerManifest::from_value(&doc) else {
        return default;
    };
    let r = manifest.resources.unwrap_or_default();
    (r.cpus.unwrap_or(2), r.memory.unwrap_or(2048))
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
///
/// `is_bundle` controls dep-directory bind-mount behavior:
///   * local-path workers (false): bind-mount VM-local dirs over
///     /workspace/{node_modules,.venv,target,dist,...} so dependency
///     installs run inside the guest without polluting the host repo.
///   * bundle workers (true): SKIP the bind-mount block entirely. The
///     bundle is immutable and ships vendored deps (a bundle's
///     `dist/index.js` IS the worker; masking it with an empty rootfs
///     dir breaks the boot).
pub fn build_libkrun_local_script(
    project: &ProjectInfo,
    prepared: bool,
    is_bundle: bool,
    overlay: bool,
) -> String {
    let env_exports = build_env_exports(&project.env);
    let mut parts: Vec<String> = Vec::new();

    parts.push("set -e".to_string());
    parts.push("export HOME=${HOME:-/root}".to_string());
    parts.push("export PATH=/usr/local/bin:/usr/bin:/bin:$PATH".to_string());
    parts.push("export LANG=${LANG:-C.UTF-8}".to_string());

    // Workspace strategy (three cases):
    //
    //  - BUNDLE: the immutable install dir is mounted live at /workspace via
    //    virtiofs; vendored deps ship inside it, so no dep handling is needed.
    //
    //  - OVERLAY local (W1 copy-in): the host project is mounted READ-ONLY at
    //    /mnt/host-src and COPIED into a VM-local /workspace (on the ext4
    //    upper). Dep installs (node_modules, .venv, …) and build outputs land
    //    in the VM-local /workspace and persist across restarts on the upper —
    //    and the host project is never WRITTEN, so the empty dep-dir folders
    //    the bind-mount model created in the host repo no longer appear. Host
    //    edits propagate via the source watcher restarting the VM (which
    //    re-copies). The dep dirs are excluded from the copy so VM-local deps
    //    survive a re-copy and host clutter isn't pulled in.
    //
    //  - LEGACY local: host project mounted live at /workspace (virtiofs) with
    //    dep dirs bind-mounted from the rootfs to keep their writes VM-local.
    //    Tradeoff: each bind target must exist, so a `mkdir /workspace/$d`
    //    creates an empty dir in the host repo (the bug W1 fixes for overlay).
    //    We tried overlayfs with a virtiofs lower and hit errno 102 (copy-up
    //    fails for PassthroughFs reads); bind-mounts sidestep that.
    //
    // The mountpoint checks guard against iii-init's mount_virtiofs_shares()
    // swallowing a mount failure as a warning (leaving the path
    // existing-but-unmounted), which would otherwise silently work against the
    // wrong backing store.
    if is_bundle {
        parts.push(
            r#"if ! { mountpoint -q /workspace 2>/dev/null || awk '$5 == "/workspace" && / - virtiofs /' /proc/self/mountinfo | grep -q .; }; then
  echo "iii: ERROR /workspace is not a virtiofs mountpoint (share missing or mount failed)" >&2
  echo "--- III_VIRTIOFS_MOUNTS=${III_VIRTIOFS_MOUNTS:-<unset>} ---" >&2
  cat /proc/self/mountinfo >&2 2>/dev/null || cat /proc/mounts >&2 2>/dev/null || mount >&2
  exit 1
fi
cd /workspace
echo "iii: workspace ready (bundle worker; dep-dir bind-mounts skipped — vendored deps are shipped in the bundle)" >&2"#
                .to_string(),
        );
    } else if overlay {
        parts.push(
            r#"SRC=/mnt/host-src
mkdir -p /workspace
if ! { mountpoint -q "$SRC" 2>/dev/null || awk -v s="$SRC" '$5 == s && / - virtiofs /' /proc/self/mountinfo | grep -q .; }; then
  echo "iii: ERROR $SRC (host source share) is not a virtiofs mountpoint (share missing or mount failed)" >&2
  echo "--- III_VIRTIOFS_MOUNTS=${III_VIRTIOFS_MOUNTS:-<unset>} ---" >&2
  cat /proc/self/mountinfo >&2 2>/dev/null || cat /proc/mounts >&2 2>/dev/null || mount >&2
  exit 1
fi
if ! command -v tar >/dev/null 2>&1; then
  echo "iii: ERROR workspace copy-in requires 'tar' in the base image" >&2
  exit 1
fi
# Re-sync deletions: /workspace lives on the persistent ext4 upper and tar -x
# only adds/overwrites (never deletes), so a source file removed on the host
# would keep running across a watcher restart. Clear every top-level entry
# EXCEPT the VM-local dep/build dirs (excluded from the copy, must persist),
# then re-extract the current source. On first boot /workspace is empty so this
# is a no-op. (Exclusion list MUST match the tar --exclude list below.)
find /workspace -mindepth 1 -maxdepth 1 \
  ! -name node_modules ! -name .venv ! -name target ! -name dist \
  ! -name __pycache__ ! -name .pytest_cache ! -name .next ! -name .git \
  -exec rm -rf {} +
# Copy host source into the VM-local /workspace, excluding dep/build dirs (at
# any depth) so VM-local installs persist across a re-copy and host clutter
# isn't pulled in. Only /mnt/host-src is read; the host project is never
# written, so no empty dep folders appear there.
( set -o pipefail; ( cd "$SRC" && tar -cf - --exclude=node_modules --exclude=.venv --exclude=target --exclude=dist --exclude=__pycache__ --exclude=.pytest_cache --exclude=.next --exclude=.git . ) | ( cd /workspace && tar -xpf - ) ) || { echo "iii: ERROR workspace copy-in failed" >&2; exit 1; }
# Enforce "host project untouched": DETACH the host source now that it's copied
# in, so worker code cannot write back to the host repo via /mnt/host-src. cd
# out first so the mount isn't busy. The unmount is FATAL: the share is mounted
# read-write, so if we can't detach it we refuse to run user code rather than
# risk mutating the host repo. The host-side watcher re-copies on the next boot.
cd /
umount "$SRC" || { echo "iii: ERROR could not detach $SRC; refusing to run with host source writable" >&2; exit 1; }
cd /workspace
echo "iii: workspace ready (copy-in from host; deps stay VM-local, host project untouched)" >&2"#
                .to_string(),
        );
    } else {
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
    }

    parts.push("echo $$ > /sys/fs/cgroup/worker/cgroup.procs 2>/dev/null || true".to_string());

    // Host source changes are handled by the host-side `__watch-source`
    // sidecar (see source_watcher.rs), which restarts the whole VM on
    // change. In-VM watchers are not expected to detect host edits, so
    // no polling env vars are exported here — they'd just add overhead
    // and couldn't help tsx 4.x anyway (tsx uses fs.watch with no
    // polling fallback, and doesn't depend on chokidar).

    // Provisioning (setup + install) gating:
    //   - legacy: the host decides via `prepared` — it can see the marker in
    //     the per-worker clone (`managed_dir/var/.iii-prepared`) and omits the
    //     block entirely once prepared.
    //   - overlay: the marker lives on the persistent ext4 upper, which the
    //     host CANNOT see (it's inside a block device). So always emit the
    //     block but guard it IN-GUEST on the marker. The upper persists across
    //     restarts, so install runs once and is skipped thereafter — matching
    //     legacy's skip semantics without needing host visibility. `set -e`
    //     still aborts (and leaves the marker unwritten) if install fails.
    if overlay {
        let mut prov = String::new();
        if !project.setup_cmd.is_empty() {
            prov.push_str(&project.setup_cmd);
            prov.push('\n');
        }
        if !project.install_cmd.is_empty() {
            prov.push_str(&project.install_cmd);
            prov.push('\n');
        }
        // `&& sync` flushes the just-installed deps + the marker from the guest
        // page cache to the ext4 upper (/dev/vdb) the instant install finishes.
        // Without it, a worker that exits within ext4's ~5s commit window tears
        // the VM down before the write lands, losing both and re-running install
        // next boot. ponytail: belt-and-suspenders with iii-init's sync-on-exit
        // (supervisor::sync_and_exit); this also covers a SIGKILL that lands
        // after install but before the worker exits.
        parts.push(format!(
            "if [ ! -e /var/.iii-prepared ]; then\n{prov}mkdir -p /var && touch /var/.iii-prepared && sync\nfi"
        ));
        // Host-visible readiness: /opt/iii is a virtiofs mount of the host's
        // managed_dir/runtime, so the host (status / `--wait`) can see this
        // even though the /var marker lives on the host-invisible ext4 upper.
        // Touched every boot after provisioning, just before exec.
        parts.push("touch /opt/iii/.iii-ready 2>/dev/null || true".to_string());
    } else if !prepared {
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
    // `exec` binds tighter than `&&`/`;`: a compound run_cmd like
    // "pip install -e . && python -m src.main" would parse as
    // `exec pip install -e .` — the shell is replaced by the FIRST
    // command, the rest never runs, and the VM silently tears down when
    // it exits. Re-quote the whole command into a single `sh -c` word so
    // exec applies to the entire scripts.start string; sh tail-execs
    // simple commands anyway, so single-command starts keep their PID
    // semantics.
    let quoted_run_cmd = format!("'{}'", project.run_cmd.replace('\'', "'\\''"));
    parts.push(format!(
        "{} && exec /bin/sh -c {}",
        env_exports, quoted_run_cmd
    ));
    parts.join("\n")
}

pub fn build_env_exports(env: &HashMap<String, String>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (k, v) in env {
        if k == "III_ENGINE_URL" || k == "III_URL" || k == "III_WORKER_NAME" {
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
    worker_name: &str,
    project_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("III_ENGINE_URL".to_string(), engine_url.to_string());
    env.insert("III_URL".to_string(), engine_url.to_string());
    // The config.yaml entry name, so SDKs can self-report the managed
    // identity. Engine truth (`iii worker status`/`list`, MOT-3931) matches
    // connections by this name; a worker that reports the SDK's
    // `hostname:pid` fallback instead reads as "not registered" forever.
    env.insert("III_WORKER_NAME".to_string(), worker_name.to_string());
    for (key, value) in project_env {
        if key != "III_ENGINE_URL" && key != "III_URL" && key != "III_WORKER_NAME" {
            env.insert(key.clone(), value.clone());
        }
    }
    env
}

// ──────────────────────────────────────────────────────────────────────────────
// New functions for local-path worker support
// ──────────────────────────────────────────────────────────────────────────────

/// Build the virtiofs mount list for a local-path worker: the host project
/// dir is shared at `workspace_guest`. Returns `(host_path, guest_path)` pairs
/// suitable for `libkrun::run_dev`.
///
/// `workspace_guest` is `/workspace` for the live-mount models (bundle, legacy)
/// and `/mnt/host-src` for the overlay W1 copy-in model (the script copies it
/// into a VM-local `/workspace`, so the host project is read-only in practice
/// and never gets the empty dep-dir folders).
pub fn build_local_mounts(project_path: &Path, workspace_guest: &str) -> Vec<(String, String)> {
    vec![(
        project_path.to_string_lossy().into_owned(),
        workspace_guest.to_string(),
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

    // 3a. Read the manifest ONCE, size-capped. The parsed doc is threaded
    //     through key validation, dependency resolution, and config extraction
    //     below so the file is read a single time, an oversize manifest can't
    //     be slurped whole into memory, and a malformed one fails fast instead
    //     of being silently skipped.
    let manifest_path = project_path.join(WORKER_MANIFEST);
    let manifest_doc = match super::project::read_manifest_doc(&manifest_path) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!(
                "{} {}\n  \
                 Fix: ensure iii.worker.yaml is valid YAML and within the size limit.",
                "error:".red(),
                e
            );
            return 1;
        }
    };

    // 3b. Strict manifest key/shape validation + deprecation warnings, BEFORE
    //     project detection so a typo'd or malformed manifest gets the precise
    //     validator error instead of a misleading "No project manifest
    //     detected". Unknown keys fail the add before anything is persisted;
    //     deprecated keys warn but proceed. Skipped when there's no
    //     iii.worker.yaml (auto-detected workers have nothing to validate).
    if let Some(doc) = &manifest_doc {
        if let Err(msg) = super::project::validate_manifest_keys(doc, &manifest_path) {
            eprintln!(
                "{} {}\n  \
                 Fix: remove or correct the offending key(s). Dry-run with \
                 `worker::validate`, or see \
                 https://motia.dev/docs/iii/worker-manifest for the supported schema.",
                "error:".red(),
                msg
            );
            return 1;
        }
        if let Err(msg) = super::project::require_manifest_name(doc, &manifest_path) {
            eprintln!(
                "{} {}\n  \
                 Fix: add `name: <worker-name>` to iii.worker.yaml — it becomes \
                 the config.yaml entry and the artifact directory name.",
                "error:".red(),
                msg
            );
            return 1;
        }
        super::project::warn_deprecated_manifest_keys(doc, &manifest_path);
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
                     scripts:\n        \
                       start: \"node src/index.js\"",
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

    // 4. Resolve worker name from the already-parsed manifest doc (no
    //    re-read); fall back to the directory name like resolve_worker_name.
    //    Trimmed to match `worker::validate`, which reports the trimmed name
    //    as valid — without it `name: " w "` validated clean but failed here
    //    on the embedded space.
    let worker_name = manifest_doc
        .as_ref()
        .and_then(|d| d.get("name"))
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            project_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("worker")
                .to_string()
        });

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
        // Defense-in-depth (MOT-3585): guarantee the in-VM dependency install
        // reruns on --force even if delete_worker_artifacts above partially
        // failed and left the managed dir (and its `.iii-prepared` marker) on
        // disk. The marker is what gates setup_cmd/install_cmd in
        // build_libkrun_local_script; if it survives, a user who changed a lock
        // file (e.g. added a package to pyproject.toml) gets the stale cache
        // and a ModuleNotFoundError at runtime. Removing the tiny marker file
        // is far more reliable than the recursive dir wipe.
        let prepared_marker = super::managed::prepared_marker_path(&worker_name);
        match std::fs::remove_file(&prepared_marker) {
            Ok(()) => {
                tracing::debug!(marker = %prepared_marker.display(), "removed prepared marker on --force");
            }
            // Expected when the full managed-dir wipe above already succeeded.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                // Abort: a surviving marker makes the next boot skip
                // setup/install and reuse stale deps (MOT-3585). Failing here
                // is better than reporting --force success with stale gating.
                eprintln!(
                    "{} could not remove the prepared marker {}: {}\n  \
                     Aborting --force so the worker is not left with a stale \
                     reinstall marker; fix the error (e.g. permissions) and retry.",
                    "error:".red(),
                    prepared_marker.display(),
                    e
                );
                return 1;
            }
        }
        if reset_config {
            let _ = super::config_file::remove_worker(&worker_name);
        }
    }

    // 5b. Resolve + install declared manifest dependencies BEFORE we touch
    //     config.yaml. A failure here leaves the local worker NOT in
    //     config.yaml and iii.lock unchanged — retry is just retry, no
    //     `--force` dance required. This is load-bearing: reversing this
    //     order recreates the "already in config.yaml" rerun trap.
    //     Parsed from the single manifest doc read in step 3a.
    let declared_deps = match manifest_doc.as_ref() {
        Some(doc) => match super::project::manifest_dependencies_from_doc(doc) {
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
        },
        None => std::collections::BTreeMap::new(),
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

    // 6. Append to config.yaml with worker_path. Bare entry — no `config:`
    //    block: workers own their configuration through the configuration
    //    worker, so the manifest's `config:` defaults are not copied into
    //    the project config file (a hand-written block there still wins via
    //    the append merge).
    let abs_path_str = project_path.to_string_lossy();
    if let Err(e) = super::config_file::append_worker_with_path(&worker_name, &abs_path_str, None) {
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
                 --wait cannot observe it boot — run `iii` in another terminal first.",
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
             Start it:  iii\n  \
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

pub async fn start_local_worker(worker_name: &str, worker_path: &str, port: u16) -> i32 {
    start_worker_impl(
        worker_name,
        worker_path,
        port,
        /*disable_watcher=*/ false,
        /*is_bundle=*/ false,
    )
    .await
}

/// Start a bundle worker VM. Same libkrun rails as
/// `start_local_worker`, but two bundle-specific behaviors apply:
///
/// 1. The host-side source watcher is NOT spawned (bundle is
///    immutable; nothing to watch).
/// 2. Resources from `iii.worker.yaml` are clamped against the
///    bundle resource caps BEFORE libkrun boot. The install-time
///    clamp (handle_bundle_add) only emits a warning; without this
///    second clamp at start-time, a manifest declaring 64 CPUs /
///    1 TiB RAM would still boot a libkrun guest with those values.
pub async fn start_bundle_worker(worker_name: &str, worker_path: &str, port: u16) -> i32 {
    // Operator kill switch — same gate as the CLI install path. An
    // installed bundle worker must not boot while bundle support is
    // disabled. Checked BEFORE any sandbox/libkrun setup so the engine
    // logs a clear refusal instead of a deeper-failure error.
    if super::bundle_download::bundle_workers_disabled() {
        eprintln!(
            "{} bundle workers are disabled via {}=1; refusing to start '{}'",
            "error:".red(),
            super::bundle_download::ENV_BUNDLE_WORKERS_DISABLED,
            worker_name,
        );
        return 1;
    }
    start_worker_impl(
        worker_name,
        worker_path,
        port,
        /*disable_watcher=*/ true,
        /*is_bundle=*/ true,
    )
    .await
}

/// Shared body for `start_local_worker` and `start_bundle_worker`.
///
/// Three flags differ between callers:
///   * `disable_watcher` — bundle installs are immutable, so the
///     host-side source watcher is suppressed.
///   * `is_bundle` — when true, resources are parsed and clamped via
///     `bundle_download::parse_bundle_resources` (saturating + capped)
///     rather than the permissive `parse_manifest_resources`. Local
///     workers continue to honor whatever the user wrote.
///
/// Re-copies project files, builds env, and runs via libkrun.
async fn start_worker_impl(
    worker_name: &str,
    worker_path: &str,
    port: u16,
    disable_watcher: bool,
    is_bundle: bool,
) -> i32 {
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

    // 1a. Bundle workers: re-run the strict manifest validator before
    // we touch the publisher-controlled YAML via load_project_info. The
    // install-time validator (handle_bundle_add) already ran, but the
    // install dir is on the host filesystem — any local-FS-write
    // attacker (or a future bug that lets a bundle write to its own
    // install dir) could modify ~/.iii/workers-bundle/{name}/iii.worker.yaml
    // between install and start to introduce scripts.setup,
    // scripts.install, or runtime.base_image. The permissive
    // load_project_info path below would honor those fields. The
    // strict validator also enforces the 64 KiB manifest cap, which
    // defuses billion-laughs YAML expansion before serde_yaml sees it.
    if is_bundle
        && let Err(e) = super::bundle_download::validate_bundle_manifest(project_path, worker_name)
    {
        eprintln!(
            "{} bundle manifest re-validation failed at start: {}",
            "error:".red(),
            e,
        );
        return 1;
    }

    // 1b. Local (non-bundle) workers: strict key validation at start too, so a
    // manifest hand-edited or swapped after `add` can't smuggle unknown keys
    // (or an oversize/billion-laughs YAML) past the engine via the permissive
    // load_project_info path below. Deprecation warnings are intentionally NOT
    // re-emitted here — they fired at add time; repeating them on every engine
    // boot would be noise.
    if !is_bundle {
        let manifest_path = project_path.join(WORKER_MANIFEST);
        match super::project::read_manifest_doc(&manifest_path) {
            Ok(Some(doc)) => {
                if let Err(e) = super::project::validate_manifest_keys(&doc, &manifest_path) {
                    eprintln!(
                        "{} manifest validation failed at start: {}",
                        "error:".red(),
                        e
                    );
                    return 1;
                }
            }
            Ok(None) => {} // auto-detected worker, no manifest to validate
            Err(e) => {
                eprintln!("{} {}", "error:".red(), e);
                return 1;
            }
        }
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
    // Overlay mode serves the shared read-only erofs base as the rootfs
    // lower (built later from the cache) plus a per-worker ext4 upper — so
    // there is NO per-worker rootfs clone. The managed_dir is just a minimal
    // trampoline: embed-init injects /init.krun from memory (overlay requires
    // an embedded init) and iii-init creates its runtime mount dirs at boot.
    // This is what eliminates both the per-worker clone AND the empty dep
    // dirs the legacy clone left behind.
    let overlay = crate::cli::overlay::overlay_active();
    if overlay {
        if let Err(e) = std::fs::create_dir_all(&managed_dir) {
            eprintln!(
                "{} Failed to create worker dir {}: {}",
                "error:".red(),
                managed_dir.display(),
                e
            );
            return 1;
        }
        // Reclaim orphaned legacy-clone artifacts (dep cache + prepared
        // marker) and stamp the overlay layout marker. Idempotent and a no-op
        // for a worker already on the overlay layout.
        crate::cli::overlay::migrate_to_overlay(&managed_dir);
    } else if needs_rootfs_clone(&managed_dir) {
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

    // 5. Workspace staging. The actual strategy lives in the boot script
    //    (build_libkrun_local_script) and depends on mode:
    //      - legacy/bundle: the host project (or install dir) is shared LIVE at
    //        guest /workspace via virtiofs; legacy also bind-mounts dep dirs
    //        VM-local.
    //      - overlay: the host project is shared at /mnt/host-src and COPIED
    //        into a VM-local /workspace on the ext4 upper (W1 copy-in), so the
    //        host repo is never written.
    //    Pre-create the managed-dir mountpoint so the legacy clone (which IS
    //    the guest root) has /workspace to mount onto; harmless under overlay.
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

    // Check the .iii-prepared marker. Silent when true — the boot script
    //    skips setup_cmd/install_cmd and nothing user-visible is happening
    //    in the fast path. Printing a "Using cached deps" banner every
    //    start made it look like install was running every restart (it
    //    wasn't), which confused users reading watcher.log tails.
    // Derive from the already-resolved `managed_dir` (built behind the strict
    // home_dir() guard above) rather than re-resolving via worker name, so the
    // marker check can't diverge from the dir this function actually uses.
    let prepared_marker = super::managed::prepared_marker_in(&managed_dir);
    let is_prepared = prepared_marker.exists();

    // 6. Build env with engine URL + OCI env + config.yaml env
    let engine_url = engine_url_for_runtime("libkrun", "0.0.0.0", port, &None);
    let config_env = super::config_file::get_worker_config_as_env(worker_name);

    let mut combined_project_env = project.env.clone();
    for (k, v) in &config_env {
        combined_project_env.insert(k.clone(), v.clone());
    }

    let mut env = build_local_env(&engine_url, worker_name, &combined_project_env);

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
    let script = build_libkrun_local_script(&project, is_prepared, is_bundle, overlay);

    // Both modes exec `bash /opt/iii/dev-run.sh`; only the SOURCE of that file
    // differs:
    //   - legacy: written into the per-worker clone at managed_dir/opt/iii,
    //     which IS the guest root (PassthroughFs) — directly visible.
    //   - overlay: after pivot the guest root is the read-only erofs lower
    //     + ext4 upper, which the host can't write into directly. Write the
    //     script to a host runtime dir and virtiofs-mount it at /opt/iii
    //     (iii-init mounts virtiofs shares POST-pivot, so it lands in the
    //     overlay root).
    //
    // The script is delivered as a FILE, never inlined into III_WORKER_CMD:
    // msb_krun passes the whole guest env via the kernel cmdline (krun_env),
    // and a multi-KB inlined script overflows CMDLINE_MAX_SIZE and panics the
    // VM builder. Keeping III_WORKER_CMD tiny (`exec bash /opt/iii/dev-run.sh`)
    // keeps the env within the cmdline budget for both modes.
    let script_dir = if overlay {
        managed_dir.join("runtime")
    } else {
        managed_dir.join("opt").join("iii")
    };
    if let Err(e) = std::fs::create_dir_all(&script_dir) {
        eprintln!("{} Failed to create run-script dir: {}", "error:".red(), e);
        return 1;
    }
    let script_path = script_dir.join("dev-run.sh");
    if let Err(e) = std::fs::write(&script_path, &script) {
        eprintln!("{} Failed to write run script: {}", "error:".red(), e);
        return 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
    }
    // Overlay: clear the prior boot's host-visible readiness marker so status
    // reports Preparing until THIS boot's guest re-touches /opt/iii/.iii-ready
    // after provisioning. It persists in managed_dir/runtime across boots, so
    // without this a restart flashes a stale Ready and `--wait` returns before
    // the new VM actually serves. (Legacy reads the in-clone /var marker, which
    // the VM teardown leaves consistent, so this only applies to overlay.)
    if overlay {
        let _ = std::fs::remove_file(script_dir.join(".iii-ready"));
    }
    // The script sets up the workspace; no pre-cd needed (and /workspace is
    // empty until virtiofs mounts, so cd-before-exec would be racy).
    let exec_path = "/bin/sh";
    let args = vec![
        "-c".to_string(),
        "exec bash /opt/iii/dev-run.sh".to_string(),
    ];

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
    let (vcpus, ram) = if is_bundle {
        // Strict clamp at start-time so a publisher-controlled manifest
        // can't boot a libkrun guest with attacker-chosen resources.
        // Install-time clamp (handle_bundle_add) only emits a WARN
        // event; the actual VM boot still re-reads the manifest from
        // the install dir, which is exactly what a malicious bundle
        // counts on.
        match super::bundle_download::parse_bundle_resources(
            project_path,
            super::bundle_download::ResourceCaps::default(),
        ) {
            Ok(r) => {
                if let Some(req) = r.clamped_cpus {
                    tracing::warn!(
                        worker = %worker_name,
                        bundle.requested_cpus = req,
                        bundle.cpus = r.cpus,
                        "bundle resources.cpus clamped at start"
                    );
                }
                if let Some(req) = r.clamped_memory_mb {
                    tracing::warn!(
                        worker = %worker_name,
                        bundle.requested_memory_mb = req,
                        bundle.memory_mb = r.memory_mb,
                        "bundle resources.memory clamped at start"
                    );
                }
                (r.cpus, r.memory_mb)
            }
            Err(e) => {
                // Manifest was validated at install time. If reading
                // it fails now, something tampered with the install
                // dir or the disk is failing. Fall back to safe
                // defaults rather than booting with raw values.
                tracing::error!(
                    worker = %worker_name,
                    error = %e,
                    "bundle resource re-clamp failed; using safe defaults"
                );
                (2, 2048)
            }
        }
    } else {
        parse_manifest_resources(&manifest_path)
    };

    // W1 copy-in (overlay local only): share the host project READ-ONLY at
    // /mnt/host-src and let the boot script copy it into a VM-local /workspace,
    // so dep installs/build outputs never write the host repo (no empty dep
    // folders). Bundles and legacy keep the live /workspace virtiofs mount.
    let copy_in = overlay && !is_bundle;
    let workspace_guest = if copy_in {
        "/mnt/host-src"
    } else {
        "/workspace"
    };
    let mut mounts = build_local_mounts(project_path, workspace_guest);
    if overlay {
        // Make the host-written dev-run.sh visible in the (post-pivot) overlay
        // root at /opt/iii. iii-init mkdir_p's the guest path before mounting,
        // and overlayfs makes /opt writable (upper) even though the erofs
        // lower is read-only.
        mounts.push((
            script_dir.to_string_lossy().into_owned(),
            "/opt/iii".to_string(),
        ));
    }

    // Overlay: build the shared read-only base erofs from the prepared base
    // rootfs (host-side, image-independent) and materialize this worker's
    // persistent ext4 upper from the embedded golden image, both when overlay
    // is active. Built in the start path so the cost is visible and a failure
    // fails the start cleanly (rather than inside the detached __vm-boot child),
    // and never silently falls back to a per-worker clone.
    let (rootfs_lower, rootfs_mode, rootfs_upper) = if overlay {
        let erofs_img = match crate::cli::erofs::ensure_base_erofs(&base_rootfs) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{} failed to build base erofs: {}", "error:".red(), e);
                return 1;
            }
        };
        let upper = if crate::cli::upper::has_golden() {
            match crate::cli::upper::ensure_upper_ext4(&managed_dir) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!(
                        "{} failed to materialize overlay upper: {}",
                        "error:".red(),
                        e
                    );
                    return 1;
                }
            }
        } else {
            None
        };
        (Some(erofs_img), "overlay".to_string(), upper)
    } else {
        (None, String::new(), None)
    };

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
        rootfs_lower,
        rootfs_mode,
        rootfs_upper,
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
    //
    // Bundle workers (disable_watcher=true) skip this entirely: the
    // install dir is immutable and there is nothing to watch.
    if !disable_watcher
        && exit_code == 0
        && let Err(e) =
            spawn_source_watcher(worker_name, project_path, &managed_dir_for_watcher, overlay).await
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
    overlay: bool,
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
    // Hand the authoritative workspace model to the watcher so it never has to
    // re-derive overlay-ness from the (best-effort, ambiguous) .iii-layout
    // marker. Only passed when overlay; absence means live-mount.
    if overlay {
        cmd.arg("--overlay");
    }

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
        let mounts = build_local_mounts(Path::new("/abs/host/project"), "/workspace");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].0, "/abs/host/project");
        assert_eq!(mounts[0].1, "/workspace");
    }

    #[test]
    fn build_local_mounts_overlay_uses_host_src_read_path() {
        // W1 copy-in mounts the host project at /mnt/host-src (read-only in
        // practice); the script copies it into a VM-local /workspace.
        let mounts = build_local_mounts(Path::new("/abs/host/project"), "/mnt/host-src");
        assert_eq!(mounts[0].0, "/abs/host/project");
        assert_eq!(mounts[0].1, "/mnt/host-src");
    }

    #[test]
    fn build_local_mounts_preserves_relative_path_string() {
        // Path stringification is lossy on non-UTF8, but for typical macOS/Linux
        // paths we round-trip exactly. Documents intended behavior.
        let mounts = build_local_mounts(Path::new("./relative/path"), "/workspace");
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
        let script = build_libkrun_local_script(
            &project, false, /*is_bundle=*/ false, /*overlay=*/ false,
        );
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
        let script = build_libkrun_local_script(
            &project, true, /*is_bundle=*/ false, /*overlay=*/ false,
        );
        assert!(!script.contains("apt-get install nodejs"));
        assert!(!script.contains("npm install"));
        assert!(script.contains("node server.js"));
        assert!(script.contains("mount --bind"));
    }

    #[test]
    fn build_libkrun_local_script_overlay_guards_install_in_guest() {
        let project = ProjectInfo {
            name: "test".to_string(),
            kind: Some("typescript".to_string()),
            setup_cmd: "apt-get install nodejs".to_string(),
            install_cmd: "npm install".to_string(),
            run_cmd: "node server.js".to_string(),
            env: HashMap::new(),
            base_image: None,
        };
        // Under overlay the host `prepared` flag is ignored: the block is
        // always emitted but guarded in-guest on the persistent marker, so the
        // install runs once and is skipped on later boots from the same upper.
        let script = build_libkrun_local_script(
            &project, /*prepared=*/ true, /*is_bundle=*/ false, true,
        );
        assert!(script.contains("if [ ! -e /var/.iii-prepared ]; then"));
        assert!(script.contains("apt-get install nodejs"));
        assert!(script.contains("npm install"));
        // Durability: the marker write is followed by `sync` so deps + marker
        // flush to the ext4 upper before a fast worker exit tears the VM down.
        assert!(script.contains("touch /var/.iii-prepared && sync"));
        assert!(script.contains("node server.js"));
        // W1 copy-in: overlay copies host source from /mnt/host-src into a
        // VM-local /workspace and does NOT bind-mount dep dirs (which would
        // mkdir empty folders in the host repo).
        assert!(script.contains("SRC=/mnt/host-src"));
        assert!(script.contains("tar -cf -"));
        // Re-sync deletions: clear /workspace (minus dep dirs) before extract so
        // host-deleted files don't keep running on the persistent upper.
        assert!(script.contains("find /workspace -mindepth 1 -maxdepth 1"));
        assert!(script.contains("! -name node_modules"));
        // Host-untouched: the unmount is FATAL, not best-effort.
        assert!(script.contains("refusing to run with host source writable"));
        assert!(
            !script.contains("umount \"$SRC\" 2>/dev/null || true"),
            "overlay host-src unmount must be fatal, not swallowed"
        );
        assert!(
            !script.contains("mount --bind"),
            "overlay must not use the dep-dir bind loop (W1 copy-in replaces it)"
        );
        assert!(
            !script.contains("/var/iii/deps"),
            "overlay must not create host-visible dep bind targets"
        );
    }

    #[test]
    fn build_libkrun_local_script_legacy_still_uses_bind_loop() {
        // Legacy (overlay=false) keeps the live /workspace + dep bind-mounts;
        // W1 copy-in is overlay-only.
        let project = ProjectInfo {
            name: "test".to_string(),
            kind: Some("typescript".to_string()),
            setup_cmd: String::new(),
            install_cmd: "npm install".to_string(),
            run_cmd: "node server.js".to_string(),
            env: HashMap::new(),
            base_image: None,
        };
        let script = build_libkrun_local_script(
            &project, /*prepared=*/ false, /*is_bundle=*/ false, false,
        );
        assert!(script.contains("mount --bind"));
        assert!(script.contains("/var/iii/deps"));
        assert!(!script.contains("SRC=/mnt/host-src"));
    }

    #[test]
    fn build_local_env_sets_engine_urls_and_worker_name() {
        let env = build_local_env("ws://localhost:49134", "my-worker", &HashMap::new());
        assert_eq!(env.get("III_ENGINE_URL").unwrap(), "ws://localhost:49134");
        assert_eq!(env.get("III_URL").unwrap(), "ws://localhost:49134");
        assert_eq!(env.get("III_WORKER_NAME").unwrap(), "my-worker");
    }

    #[test]
    fn build_local_env_preserves_custom_env() {
        let mut project_env = HashMap::new();
        project_env.insert("CUSTOM".to_string(), "value".to_string());
        let env = build_local_env("ws://localhost:49134", "my-worker", &project_env);
        assert_eq!(env.get("CUSTOM").unwrap(), "value");
        assert_eq!(env.get("III_ENGINE_URL").unwrap(), "ws://localhost:49134");
        assert_eq!(env.get("III_URL").unwrap(), "ws://localhost:49134");
    }

    #[test]
    fn build_local_env_worker_name_not_overridable_by_project_env() {
        let mut project_env = HashMap::new();
        project_env.insert("III_WORKER_NAME".to_string(), "impostor".to_string());
        let env = build_local_env("ws://localhost:49134", "my-worker", &project_env);
        assert_eq!(env.get("III_WORKER_NAME").unwrap(), "my-worker");
    }

    #[test]
    fn build_env_exports_excludes_engine_urls_and_worker_name() {
        let mut env = HashMap::new();
        env.insert(
            "III_ENGINE_URL".to_string(),
            "ws://localhost:49134".to_string(),
        );
        env.insert("III_URL".to_string(), "ws://localhost:49134".to_string());
        env.insert("III_WORKER_NAME".to_string(), "my-worker".to_string());
        env.insert("CUSTOM_VAR".to_string(), "custom-val".to_string());

        let exports = build_env_exports(&env);
        assert!(!exports.contains("III_ENGINE_URL"));
        assert!(!exports.contains("III_URL"));
        assert!(!exports.contains("III_WORKER_NAME"));
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

    #[test]
    fn parse_manifest_resources_defaults_when_shape_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join(WORKER_MANIFEST);
        // Valid YAML, but `resources` is a scalar where the typed manifest
        // requires a mapping — WorkerManifest::from_value fails and the
        // lenient sizing probe must fall back to defaults.
        std::fs::write(&manifest_path, "name: w\nresources: lots\n").unwrap();

        let (cpus, memory) = parse_manifest_resources(&manifest_path);

        assert_eq!(cpus, 2);
        assert_eq!(memory, 2048);
    }

    #[tokio::test]
    async fn local_add_rejects_invalid_dependency_semver_range() {
        let dir = tempfile::tempdir().unwrap();
        let yaml = "name: iii-cov-test-dep-range\n\
                    scripts:\n  install: \"npm install\"\n  start: \"node server.js\"\n\
                    dependencies:\n  other-worker: \"not a semver range\"\n";
        std::fs::write(dir.path().join(WORKER_MANIFEST), yaml).unwrap();

        let code = handle_local_add(
            dir.path().to_str().unwrap(),
            /*force=*/ false,
            /*reset_config=*/ false,
            /*brief=*/ false,
            /*wait=*/ false,
        )
        .await;

        // Fails at dependency resolution, BEFORE config.yaml is touched.
        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn start_rejects_manifest_with_unknown_keys_before_boot() {
        let dir = tempfile::tempdir().unwrap();
        let yaml = "name: w\nscripts:\n  start: \"node server.js\"\ntypo_key: 1\n";
        std::fs::write(dir.path().join(WORKER_MANIFEST), yaml).unwrap();

        let code = start_local_worker(
            "iii-cov-test-start-unknown-key",
            dir.path().to_str().unwrap(),
            49134,
        )
        .await;

        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn start_rejects_oversized_manifest_before_boot() {
        let dir = tempfile::tempdir().unwrap();
        let mut yaml = String::from("name: w\nscripts:\n  start: \"node server.js\"\n");
        let cap = super::super::project::MAX_LOCAL_MANIFEST_BYTES as usize;
        yaml.push_str(&"# pad\n".repeat(cap / 6 + 16));
        assert!(yaml.len() as u64 > super::super::project::MAX_LOCAL_MANIFEST_BYTES);
        std::fs::write(dir.path().join(WORKER_MANIFEST), yaml).unwrap();

        let code = start_local_worker(
            "iii-cov-test-start-oversize",
            dir.path().to_str().unwrap(),
            49134,
        )
        .await;

        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn start_skips_manifest_validation_when_manifest_absent() {
        // Empty dir: read_manifest_doc returns Ok(None) (nothing to validate),
        // then start fails at project detection — never at the validator.
        let dir = tempfile::tempdir().unwrap();

        let code = start_local_worker(
            "iii-cov-test-start-no-manifest",
            dir.path().to_str().unwrap(),
            49134,
        )
        .await;

        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn start_passes_key_validation_then_rejects_unrecognized_kind() {
        // Keys are all known (runtime.kind is deprecated, not unknown), so the
        // strict start-time validator falls through; the run then fails at
        // ProjectInfo::validate on the unsupported kind — still pre-boot.
        let dir = tempfile::tempdir().unwrap();
        let yaml = "name: w\nruntime:\n  kind: cobol\n\
                    scripts:\n  install: \"true\"\n  start: \"node server.js\"\n";
        std::fs::write(dir.path().join(WORKER_MANIFEST), yaml).unwrap();

        let code = start_local_worker(
            "iii-cov-test-start-bad-kind",
            dir.path().to_str().unwrap(),
            49134,
        )
        .await;

        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn bundle_start_skips_local_validation_then_rejects_unrecognized_kind() {
        // is_bundle=true takes validate_bundle_manifest (which allows the
        // deprecated runtime.kind) and skips the local strict-validation
        // block; the run then fails pre-boot at ProjectInfo::validate.
        let dir = tempfile::tempdir().unwrap();
        let yaml = "name: iii-cov-test-bundle-kind\nruntime:\n  kind: cobol\n\
                    scripts:\n  start: \"node bundle.js\"\n";
        std::fs::write(dir.path().join(WORKER_MANIFEST), yaml).unwrap();

        let code = start_bundle_worker(
            "iii-cov-test-bundle-kind",
            dir.path().to_str().unwrap(),
            49134,
        )
        .await;

        assert_eq!(code, 1);
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
