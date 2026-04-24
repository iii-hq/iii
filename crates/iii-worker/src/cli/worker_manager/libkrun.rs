// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! libkrun VM runtime for `iii worker dev`.
//!
//! Provides VM-based isolated execution using libkrun (Apple Hypervisor.framework
//! on macOS, KVM on Linux). The VM runs in a separate helper process
//! for crash isolation.

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::path::PathBuf;

use super::oci::{
    expected_oci_arch, pull_and_extract_rootfs, read_cached_rootfs_arch, read_oci_entrypoint,
    read_oci_env,
};
use crate::cli::rootfs::clone_rootfs;

/// Forward optional VM-boot tuning flags to the `__vm-boot` child
/// based on opt-in environment variables. Keeps the public API of
/// `run_dev` / `LibkrunAdapter::start` unchanged while giving
/// operators a single place to enable hyperthreading, nested
/// virtualization, virtiofs DAX window tuning, and the worker-side
/// NOFILE rlimit for perf experiments.
///
/// Variables read (all optional; omit to take each CLI default):
///   - `III_VM_HYPERTHREADING=1|true|on|yes` → `--hyperthreading`
///   - `III_VM_NESTED_VIRT=1|true|on|yes`   → `--nested-virt`
///   - `III_VM_VIRTIOFS_SHM_SIZE_MIB=<int>` → `--virtiofs-shm-size-mib <n>` (0 = skip)
///   - `III_VM_NOFILE_LIMIT=<int>`     → `--nofile-limit <n>` (0 = let iii-init own it)
///
/// Bad values are ignored silently (a typo in an opt-in perf flag
/// should never fail a worker boot). Appending args to both
/// `std::process::Command` and `tokio::process::Command` needs the
/// same logic, so we take `&mut CommandArgsExt` via the closure
/// rather than committing to either concrete type.
fn apply_vm_tuning_env(mut push: impl FnMut(&str, Option<&str>)) {
    let parse_bool =
        |v: &str| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes");

    if std::env::var("III_VM_HYPERTHREADING")
        .ok()
        .filter(|v| parse_bool(v))
        .is_some()
    {
        push("--hyperthreading", None);
    }
    if std::env::var("III_VM_NESTED_VIRT")
        .ok()
        .filter(|v| parse_bool(v))
        .is_some()
    {
        push("--nested-virt", None);
    }
    if let Some(n) = std::env::var("III_VM_VIRTIOFS_SHM_SIZE_MIB")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
    {
        let owned = n.to_string();
        push("--virtiofs-shm-size-mib", Some(&owned));
    }
    if let Some(n) = std::env::var("III_VM_NOFILE_LIMIT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    {
        let owned = n.to_string();
        push("--nofile-limit", Some(&owned));
    }
}

/// Check if libkrun runtime is available on this system.
/// msb_krun (the VMM) is compiled into the binary; this checks for libkrunfw.
pub fn libkrun_available() -> bool {
    crate::cli::firmware::resolve::resolve_libkrunfw_dir().is_some()
}

/// Build the VM boot env. Launcher wins: `III_ISOLATION=libkrun` is written
/// after caller env so an OCI image `ENV III_ISOLATION=docker` cannot override it.
pub(crate) fn build_vm_env(caller_env: HashMap<String, String>) -> HashMap<String, String> {
    let mut merged = HashMap::with_capacity(caller_env.len() + 1);
    for (key, value) in caller_env {
        merged.insert(key, value);
    }
    merged.insert("III_ISOLATION".to_string(), "libkrun".to_string());
    merged
}

/// Run a dev worker session inside a libkrun VM.
///
/// Spawns `iii-worker __vm-boot` as a child process which boots the VM via libkrun FFI.
/// Uses a separate process for crash isolation.
pub async fn run_dev(
    _kind: &str,
    _project_path: &str,
    exec_path: &str,
    args: &[String],
    env: HashMap<String, String>,
    vcpus: u32,
    ram_mib: u32,
    rootfs: PathBuf,
    background: bool,
    worker_name: &str,
    mounts: &[(String, String)],
) -> i32 {
    let env = build_vm_env(env);

    let self_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot locate iii-worker binary: {}", e);
            return 1;
        }
    };

    #[cfg(target_os = "macos")]
    {
        if let Err(e) = super::platform::ensure_macos_entitlements(&self_exe) {
            eprintln!(
                "warning: failed to codesign for Hypervisor entitlement: {}",
                e
            );
        }
    }

    let mut cmd = tokio::process::Command::new(&self_exe);
    cmd.arg("__vm-boot");
    cmd.arg("--rootfs").arg(&rootfs);
    cmd.arg("--exec").arg(exec_path);
    cmd.arg("--workdir").arg("/workspace");
    cmd.arg("--vcpus").arg(vcpus.to_string());
    cmd.arg("--ram").arg(ram_mib.to_string());
    // Control channel for host-driven fast restarts. __vm-boot owns the
    // proxy thread + socketpair; we just tell it where to put the unix
    // socket so the watcher (and stop handler) knows where to connect.
    cmd.arg("--control-sock").arg(rootfs.join("control.sock"));
    // Shell-exec channel for `iii worker exec`. Colocated with the
    // control socket so a single managed dir holds every endpoint for
    // this VM. __vm-boot spawns the async relay if the path is given.
    cmd.arg("--shell-sock").arg(rootfs.join("shell.sock"));

    for (key, value) in &env {
        cmd.arg("--env").arg(format!("{}={}", key, value));
    }

    for (host, guest) in mounts {
        cmd.arg("--mount").arg(format!("{}:{}", host, guest));
    }

    for arg in args {
        cmd.arg("--arg").arg(arg);
    }

    // Forward optional VM-tuning env vars (III_VM_*) to __vm-boot.
    apply_vm_tuning_env(|flag, val| {
        cmd.arg(flag);
        if let Some(v) = val {
            cmd.arg(v);
        }
    });

    if let Some(fw_dir) = crate::cli::firmware::resolve::resolve_libkrunfw_dir() {
        cmd.env(
            crate::cli::firmware::resolve::lib_path_env_var(),
            fw_dir.to_string_lossy().as_ref(),
        );
    }

    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid().map_err(std::io::Error::other)?;
            Ok(())
        });
    }

    cmd.stdin(std::process::Stdio::null());

    if background {
        let logs_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".iii/logs")
            .join(worker_name);
        if let Err(e) = std::fs::create_dir_all(&logs_dir) {
            eprintln!("{} Failed to create logs dir: {}", "error:".red(), e);
            return 1;
        }
        let stdout_file = match std::fs::File::create(logs_dir.join("stdout.log")) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{} Failed to create stdout log: {}", "error:".red(), e);
                return 1;
            }
        };
        let stderr_file = match std::fs::File::create(logs_dir.join("stderr.log")) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{} Failed to create stderr log: {}", "error:".red(), e);
                return 1;
            }
        };
        cmd.arg("--console-output").arg(logs_dir.join("stdout.log"));
        cmd.stdout(stdout_file).stderr(stderr_file);
    }

    match cmd.spawn() {
        Ok(mut child) => {
            // Write PID file so is_worker_running / stop / kill_stale_worker can find us.
            // Use the hardened writer: O_NOFOLLOW + 0o600 on Unix so a
            // symlink pre-planted at vm.pid can't redirect our write to
            // a sensitive file. Matches the watch.pid hardening.
            let pid_file = rootfs.join("vm.pid");
            let pid = child.id().unwrap_or(0);
            if pid > 0
                && let Err(e) = crate::cli::pidfile::write_pid_file_strict(&pid_file, pid)
            {
                eprintln!(
                    "{} Failed to write PID file {}: {}",
                    "error:".red(),
                    pid_file.display(),
                    e
                );
                // Kill the child so we don't leave an untracked VM running
                let _ = child.kill().await;
                return 1;
            }

            if background {
                eprintln!(
                    "  {} {} started (pid: {})",
                    "✓".green(),
                    worker_name.bold(),
                    pid
                );
                return 0;
            }

            let exit_code = tokio::select! {
                result = child.wait() => {
                    match result {
                        Ok(status) => status.code().unwrap_or(1),
                        Err(e) => {
                            eprintln!("error: VM boot process failed: {}", e);
                            1
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    child.kill().await.ok();
                    0
                }
                _ = super::platform::ensure_terminal_isig() => {
                    unreachable!()
                }
            };

            // Clean up PID file on exit
            let _ = std::fs::remove_file(&pid_file);

            #[cfg(unix)]
            super::super::local_worker::restore_terminal_cooked_mode();

            exit_code
        }
        Err(e) => {
            eprintln!("error: Failed to spawn VM boot: {}", e);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// LibkrunAdapter — RuntimeAdapter implementation for managed workers
// ---------------------------------------------------------------------------

use super::adapter::{ContainerSpec, ContainerStatus, ImageInfo, RuntimeAdapter};

pub struct LibkrunAdapter;

impl Default for LibkrunAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl LibkrunAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn worker_dir(name: &str) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".iii")
            .join("managed")
            .join(name)
    }

    /// Tighten `~/.iii/managed` to 0o700 so a same-UID attacker
    /// can't race `connect()` on per-worker sockets before
    /// per-worker dir permissions land.
    fn ensure_managed_parent_restricted() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let parent = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".iii")
                .join("managed");
            if let Err(e) = std::fs::create_dir_all(&parent) {
                tracing::warn!(
                    path = %parent.display(),
                    error = %e,
                    "failed to create ~/.iii/managed parent dir"
                );
                return;
            }
            match std::fs::metadata(&parent) {
                Ok(meta) => {
                    let mode = meta.permissions().mode() & 0o777;
                    if mode != 0o700 {
                        if let Err(e) = std::fs::set_permissions(
                            &parent,
                            std::fs::Permissions::from_mode(0o700),
                        ) {
                            tracing::warn!(
                                path = %parent.display(),
                                current_mode = format!("{mode:o}"),
                                error = %e,
                                "could not tighten ~/.iii/managed to 0o700; \
                                 per-worker dirs still land 0o700 but TOCTOU \
                                 window remains on socket create",
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        path = %parent.display(),
                        error = %e,
                        "stat ~/.iii/managed failed; skipping permission tighten"
                    );
                }
            }
        }
    }

    /// Directory containing the image cache entry: `<home>/.iii/images/<hash>/`.
    /// Hash input is the CANONICAL form of the image ref (any trailing
    /// `@sha256:...` stripped via [`oci_ref::canonical_cache_key`]) so pinned
    /// and unpinned variants of the same image share one cache dir. This
    /// matters because the engine at worker-start time may resolve the image
    /// ref from `config.yaml` (pinned) while `iii worker add` pulled it
    /// originally from `/resolve` (unpinned) — both must hit the same dir or
    /// we re-pull gigabytes for no reason.
    ///
    /// The cache dir wraps two children:
    /// - `rootfs/` — the extracted image contents (what used to live
    ///   directly under `<hash>/` in the pre-fix layout). `clone_rootfs`
    ///   copies this into per-worker rootfs at VM start.
    /// - `digest.txt` — sidecar metadata (Task 5). Lives OUTSIDE `rootfs/`
    ///   so it does NOT end up inside worker filesystems.
    pub fn image_cache_dir(image: &str) -> PathBuf {
        use crate::cli::oci_ref;
        let hash = {
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(oci_ref::canonical_cache_key(image).as_bytes());
            hex::encode(&hasher.finalize()[..8])
        };
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".iii")
            .join("images")
            .join(hash)
    }

    /// Path to the extracted rootfs inside the image cache dir.
    /// Callers mount / clone / read files from this dir.
    ///
    /// Layout changed from the pre-fix version: the rootfs used to live
    /// directly at `<hash>/`; it now lives at `<hash>/rootfs/`. This
    /// naturally invalidates old caches (the cache-hit check looks for
    /// `<hash>/rootfs/bin/` which old layouts don't have, forcing a
    /// re-pull). Orphaned old dirs waste disk but don't break anything.
    pub fn image_rootfs(image: &str) -> PathBuf {
        Self::image_cache_dir(image).join("rootfs")
    }

    pub fn pid_file(name: &str) -> PathBuf {
        Self::worker_dir(name).join("vm.pid")
    }

    pub fn logs_dir(name: &str) -> PathBuf {
        Self::worker_dir(name).join("logs")
    }

    fn stdout_log(name: &str) -> PathBuf {
        Self::logs_dir(name).join("stdout.log")
    }

    fn stderr_log(name: &str) -> PathBuf {
        Self::logs_dir(name).join("stderr.log")
    }

    fn pid_alive(pid: u32) -> bool {
        unsafe { nix::libc::kill(pid as i32, 0) == 0 }
    }
}

#[async_trait::async_trait]
impl RuntimeAdapter for LibkrunAdapter {
    async fn pull(&self, image: &str) -> Result<ImageInfo> {
        let rootfs_dir = Self::image_rootfs(image);
        let expected_arch = expected_oci_arch().to_string();

        if rootfs_dir.exists() && rootfs_dir.join("bin").exists() {
            let cached_arch = read_cached_rootfs_arch(&rootfs_dir);
            let arch_match = cached_arch
                .as_deref()
                .map(|a| a == expected_arch)
                .unwrap_or(false);
            if arch_match {
                tracing::info!(image = %image, "image rootfs cached, skipping pull");
            } else {
                tracing::warn!(
                    image = %image,
                    expected_arch = %expected_arch,
                    cached_arch = ?cached_arch,
                    "cached rootfs architecture mismatch, rebuilding cache"
                );
                let _ = std::fs::remove_dir_all(&rootfs_dir);
                tracing::info!(image = %image, "pulling OCI image via libkrun");
                pull_and_extract_rootfs(image, &rootfs_dir).await?;
                let hosts_path = rootfs_dir.join("etc/hosts");
                if !hosts_path.exists() {
                    let _ = std::fs::write(&hosts_path, "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n");
                }
            }
        } else {
            tracing::info!(image = %image, "pulling OCI image via libkrun");
            pull_and_extract_rootfs(image, &rootfs_dir).await?;
            let hosts_path = rootfs_dir.join("etc/hosts");
            if !hosts_path.exists() {
                let _ = std::fs::write(&hosts_path, "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n");
            }
        }

        let final_arch = read_cached_rootfs_arch(&rootfs_dir);
        let final_match = final_arch
            .as_deref()
            .map(|a| a == expected_arch)
            .unwrap_or(false);
        if !final_match {
            anyhow::bail!(
                "image architecture mismatch for {}: expected linux/{} but pulled {:?}. \
This image likely does not publish arm64. Rebuild/push a multi-arch image (linux/arm64,linux/amd64).",
                image,
                expected_arch,
                final_arch
            );
        }

        let size_bytes = fs_dir_size(&rootfs_dir).ok();

        Ok(ImageInfo {
            image: image.to_string(),
            size_bytes,
        })
    }

    async fn extract_file(&self, image: &str, path: &str) -> Result<Vec<u8>> {
        let rootfs_dir = Self::image_rootfs(image);
        let file_path = rootfs_dir.join(path.trim_start_matches('/'));
        std::fs::read(&file_path)
            .with_context(|| format!("failed to read {} from rootfs", file_path.display()))
    }

    async fn start(&self, spec: &ContainerSpec) -> Result<String> {
        Self::ensure_managed_parent_restricted();
        let worker_dir = Self::worker_dir(&spec.name);
        std::fs::create_dir_all(&worker_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&worker_dir, std::fs::Permissions::from_mode(0o700));
        }

        let rootfs_dir = Self::image_rootfs(&spec.image);
        if !rootfs_dir.exists() {
            tracing::info!(image = %spec.image, "rootfs not found, pulling automatically");
            eprintln!("  Pulling rootfs ({})...", spec.image);
            self.pull(&spec.image).await?;
        }

        let worker_rootfs = worker_dir.join("rootfs");
        let expected_arch = expected_oci_arch().to_string();
        let mut needs_clone = !worker_rootfs.exists();
        if !needs_clone {
            let worker_arch = read_cached_rootfs_arch(&worker_rootfs);
            let arch_match = worker_arch
                .as_deref()
                .map(|a| a == expected_arch)
                .unwrap_or(false);
            if !arch_match {
                let _ = std::fs::remove_dir_all(&worker_rootfs);
                needs_clone = true;
            }
        }
        if needs_clone {
            clone_rootfs(&rootfs_dir, &worker_rootfs)
                .map_err(|e| anyhow::anyhow!("failed to clone rootfs: {}", e))?;
        }

        if !iii_filesystem::init::has_init() {
            let init_path = crate::cli::firmware::download::ensure_init_binary().await?;
            let dest = worker_rootfs.join("init.krun");
            std::fs::copy(&init_path, &dest).with_context(|| {
                format!("failed to copy iii-init to rootfs: {}", dest.display())
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
            }
        }

        let self_exe = std::env::current_exe().context("cannot locate iii-worker binary")?;
        #[cfg(target_os = "macos")]
        {
            let _ = super::platform::ensure_macos_entitlements(&self_exe);
        }

        let logs_dir = Self::logs_dir(&spec.name);
        std::fs::create_dir_all(&logs_dir)
            .with_context(|| format!("failed to create logs dir: {}", logs_dir.display()))?;

        let stdout_file = std::fs::File::create(Self::stdout_log(&spec.name))
            .with_context(|| "failed to create stdout.log")?;
        let stderr_file = std::fs::File::create(Self::stderr_log(&spec.name))
            .with_context(|| "failed to create stderr.log")?;

        let (exec_path, mut exec_args) =
            read_oci_entrypoint(&worker_rootfs).unwrap_or_else(|| ("/bin/sh".to_string(), vec![]));

        if let Some(url) = spec.env.get("III_ENGINE_URL").or(spec.env.get("III_URL")) {
            let mut i = 0;
            let mut found = false;
            while i < exec_args.len() {
                if exec_args[i] == "--url" && i + 1 < exec_args.len() {
                    exec_args[i + 1] = url.clone();
                    found = true;
                    break;
                }
                i += 1;
            }
            if !found {
                exec_args.push("--url".to_string());
                exec_args.push(url.clone());
            }
        }

        let workdir =
            super::oci::read_oci_workdir(&worker_rootfs).unwrap_or_else(|| "/".to_string());

        let mut cmd = std::process::Command::new(&self_exe);
        cmd.arg("__vm-boot");
        cmd.arg("--rootfs").arg(&worker_rootfs);
        cmd.arg("--exec").arg(&exec_path);
        cmd.arg("--workdir").arg(&workdir);
        let vcpus = spec
            .cpu_limit
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|v| v.ceil().max(1.0) as u32)
            .unwrap_or(2);
        cmd.arg("--vcpus").arg(vcpus.to_string());
        cmd.arg("--ram").arg(
            spec.memory_limit
                .as_deref()
                .and_then(k8s_mem_to_mib)
                .unwrap_or_else(|| "2048".to_string()),
        );

        let pid_file_path = Self::pid_file(&spec.name);
        cmd.arg("--pid-file").arg(&pid_file_path);

        cmd.arg("--console-output")
            .arg(Self::stdout_log(&spec.name));

        // Control channel for host-driven fast restarts. The socket is
        // colocated with the pid file under ~/.iii/managed/<name>/ so
        // supervisor_ctl::control_socket_path resolves to the same place
        // the watcher and stop handler use. Without this, iii-init's
        // supervisor mode stays dormant and every source edit falls back
        // to a full VM restart.
        cmd.arg("--control-sock")
            .arg(worker_dir.join("control.sock"));
        // Shell-exec channel alongside the control channel. `iii worker
        // exec` connects to shell.sock; the in-VM dispatcher thread
        // handles requests. Absent => exec refuses with a clear error.
        cmd.arg("--shell-sock").arg(worker_dir.join("shell.sock"));

        let image_env = read_oci_env(&worker_rootfs);
        let mut caller_env: HashMap<String, String> = image_env.into_iter().collect();
        for (key, value) in &spec.env {
            caller_env.insert(key.clone(), value.clone());
        }
        let merged_env = build_vm_env(caller_env);

        for (key, value) in &merged_env {
            cmd.arg("--env").arg(format!("{}={}", key, value));
        }
        for arg in &exec_args {
            cmd.arg("--arg").arg(arg);
        }

        // Forward optional VM-tuning env vars (III_VM_*) to __vm-boot.
        // Same opt-in model as `run_dev` — unset vars keep defaults
        // so this is strictly additive.
        apply_vm_tuning_env(|flag, val| {
            cmd.arg(flag);
            if let Some(v) = val {
                cmd.arg(v);
            }
        });

        if let Some(fw_dir) = crate::cli::firmware::resolve::resolve_libkrunfw_dir() {
            cmd.env(
                crate::cli::firmware::resolve::lib_path_env_var(),
                fw_dir.to_string_lossy().as_ref(),
            );
        }

        cmd.stdout(stdout_file);
        cmd.stderr(stderr_file);
        cmd.stdin(std::process::Stdio::null());

        let child = cmd.spawn().context("failed to spawn VM boot process")?;

        let pid = child.id();
        crate::cli::pidfile::write_pid_file_strict(&Self::pid_file(&spec.name), pid)?;

        tracing::info!(name = %spec.name, pid = pid, "started libkrun VM");

        Ok(pid.to_string())
    }

    async fn stop(&self, container_id: &str, timeout_secs: u32) -> Result<()> {
        if let Ok(pid) = container_id.parse::<u32>()
            && Self::pid_alive(pid)
        {
            tracing::info!(pid = pid, "sending SIGTERM to libkrun VM");
            unsafe {
                nix::libc::kill(pid as i32, nix::libc::SIGTERM);
            }

            let deadline =
                std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs as u64);
            while std::time::Instant::now() < deadline {
                unsafe {
                    nix::libc::waitpid(pid as i32, std::ptr::null_mut(), nix::libc::WNOHANG);
                }
                if !Self::pid_alive(pid) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            if Self::pid_alive(pid) {
                tracing::warn!(pid = pid, "VM did not exit after SIGTERM, sending SIGKILL");
                unsafe {
                    nix::libc::kill(pid as i32, nix::libc::SIGKILL);
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                unsafe {
                    nix::libc::waitpid(pid as i32, std::ptr::null_mut(), nix::libc::WNOHANG);
                }
            }
        }
        Ok(())
    }

    async fn status(&self, container_id: &str) -> Result<ContainerStatus> {
        let pid: u32 = container_id.parse().unwrap_or(0);
        let running = pid > 0 && Self::pid_alive(pid);

        Ok(ContainerStatus {
            name: String::new(),
            container_id: container_id.to_string(),
            running,
            exit_code: if running { None } else { Some(0) },
        })
    }

    async fn remove(&self, container_id: &str) -> Result<()> {
        self.stop(container_id, 0).await?;

        let managed_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".iii")
            .join("managed");

        if let Ok(entries) = std::fs::read_dir(&managed_dir) {
            for entry in entries.flatten() {
                let pid_file = entry.path().join("vm.pid");
                if let Ok(pid_str) = std::fs::read_to_string(&pid_file)
                    && pid_str.trim() == container_id
                {
                    let _ = std::fs::remove_dir_all(entry.path());
                    tracing::info!(container_id = %container_id, "removed libkrun worker directory");
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

pub fn k8s_mem_to_mib(value: &str) -> Option<String> {
    if let Some(n) = value.strip_suffix("Mi") {
        Some(n.to_string())
    } else if let Some(n) = value.strip_suffix("Gi") {
        n.parse::<u64>().ok().map(|v| (v * 1024).to_string())
    } else if let Some(n) = value.strip_suffix("Ki") {
        n.parse::<u64>().ok().map(|v| (v / 1024).to_string())
    } else {
        value
            .parse::<u64>()
            .ok()
            .map(|v| (v / (1024 * 1024)).to_string())
    }
}

fn fs_dir_size(path: &std::path::Path) -> Result<u64> {
    let mut total = 0u64;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_dir() {
                total += fs_dir_size(&entry.path()).unwrap_or(0);
            } else {
                total += meta.len();
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logs_dir_path() {
        let dir = LibkrunAdapter::logs_dir("test-worker");
        assert!(
            dir.to_string_lossy()
                .contains(".iii/managed/test-worker/logs")
        );
    }

    #[test]
    fn test_libkrun_available_returns_bool() {
        let result = libkrun_available();
        let _ = result;
    }

    #[test]
    fn build_vm_env_injects_isolation_marker_into_empty_input() {
        let merged = build_vm_env(HashMap::new());
        assert_eq!(merged.get("III_ISOLATION"), Some(&"libkrun".to_string()));
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn build_vm_env_preserves_caller_vars_and_adds_isolation() {
        let mut caller = HashMap::new();
        caller.insert("NODE_ENV".to_string(), "production".to_string());
        caller.insert("III_URL".to_string(), "ws://127.0.0.1:3111".to_string());
        let merged = build_vm_env(caller);
        assert_eq!(merged.get("III_ISOLATION"), Some(&"libkrun".to_string()));
        assert_eq!(merged.get("NODE_ENV"), Some(&"production".to_string()));
        assert_eq!(
            merged.get("III_URL"),
            Some(&"ws://127.0.0.1:3111".to_string())
        );
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn build_vm_env_launcher_overrides_caller_isolation() {
        let mut caller = HashMap::new();
        caller.insert("III_ISOLATION".to_string(), "docker".to_string());
        let merged = build_vm_env(caller);
        assert_eq!(merged.get("III_ISOLATION"), Some(&"libkrun".to_string()));
    }

    #[test]
    fn test_k8s_mem_to_mib_mi() {
        assert_eq!(k8s_mem_to_mib("512Mi"), Some("512".to_string()));
    }

    #[test]
    fn test_k8s_mem_to_mib_gi() {
        assert_eq!(k8s_mem_to_mib("2Gi"), Some("2048".to_string()));
    }

    #[test]
    fn test_k8s_mem_to_mib_ki() {
        assert_eq!(k8s_mem_to_mib("1048576Ki"), Some("1024".to_string()));
    }

    #[test]
    fn test_k8s_mem_to_mib_bytes() {
        assert_eq!(k8s_mem_to_mib("2147483648"), Some("2048".to_string()));
    }

    #[test]
    fn test_k8s_mem_to_mib_invalid() {
        assert_eq!(k8s_mem_to_mib("not-a-number"), None);
    }
}
