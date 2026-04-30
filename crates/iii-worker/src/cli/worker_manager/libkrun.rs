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
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::oci::{expected_oci_arch, read_cached_rootfs_arch, read_oci_entrypoint, read_oci_env};
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

    let env_pairs: Vec<(String, String)> =
        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let control_sock = rootfs.join("control.sock");
    let shell_sock = rootfs.join("shell.sock");

    let mut cmd = tokio::process::Command::new(&self_exe);
    cmd.arg("__vm-boot");
    for boot_arg in vm_boot_args_dev(
        &rootfs,
        exec_path,
        vcpus,
        ram_mib,
        &control_sock,
        &shell_sock,
        &env_pairs,
        mounts,
        args,
    ) {
        cmd.arg(boot_arg);
    }

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
            // Hardened writer: O_NOFOLLOW + 0o600 on Unix so a symlink
            // pre-planted at vm.pid can't redirect our write to a
            // sensitive file. Matches the watch.pid hardening.
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

use super::adapter::{ContainerSpec, ContainerStatus, ImageInfo, RuntimeAdapter};

const VM_BOOT_NETWORK_FLAG: &str = "--network";

fn vm_boot_args_oci(
    rootfs: &Path,
    exec_path: &str,
    workdir: &str,
    vcpus: u32,
    ram_mib: &str,
    pid_file: &Path,
    console_output: &Path,
    control_sock: &Path,
    shell_sock: &Path,
    env: &[(String, String)],
    exec_args: &[String],
) -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::new();
    out.push(OsString::from("--rootfs"));
    out.push(rootfs.as_os_str().to_owned());
    out.push(OsString::from("--exec"));
    out.push(OsString::from(exec_path));
    out.push(OsString::from("--workdir"));
    out.push(OsString::from(workdir));
    out.push(OsString::from("--vcpus"));
    out.push(OsString::from(vcpus.to_string()));
    out.push(OsString::from("--ram"));
    out.push(OsString::from(ram_mib));
    out.push(OsString::from("--pid-file"));
    out.push(pid_file.as_os_str().to_owned());
    out.push(OsString::from("--console-output"));
    out.push(console_output.as_os_str().to_owned());
    out.push(OsString::from("--control-sock"));
    out.push(control_sock.as_os_str().to_owned());
    out.push(OsString::from("--shell-sock"));
    out.push(shell_sock.as_os_str().to_owned());
    out.push(OsString::from(VM_BOOT_NETWORK_FLAG));
    for (k, v) in env {
        out.push(OsString::from("--env"));
        out.push(OsString::from(format!("{}={}", k, v)));
    }
    for arg in exec_args {
        out.push(OsString::from("--arg"));
        out.push(OsString::from(arg.as_str()));
    }
    out
}

fn vm_boot_args_dev(
    rootfs: &Path,
    exec_path: &str,
    vcpus: u32,
    ram_mib: u32,
    control_sock: &Path,
    shell_sock: &Path,
    env: &[(String, String)],
    mounts: &[(String, String)],
    exec_args: &[String],
) -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::new();
    out.push(OsString::from("--rootfs"));
    out.push(rootfs.as_os_str().to_owned());
    out.push(OsString::from("--exec"));
    out.push(OsString::from(exec_path));
    out.push(OsString::from("--workdir"));
    out.push(OsString::from("/workspace"));
    out.push(OsString::from("--vcpus"));
    out.push(OsString::from(vcpus.to_string()));
    out.push(OsString::from("--ram"));
    out.push(OsString::from(ram_mib.to_string()));
    out.push(OsString::from("--control-sock"));
    out.push(control_sock.as_os_str().to_owned());
    out.push(OsString::from("--shell-sock"));
    out.push(shell_sock.as_os_str().to_owned());
    out.push(OsString::from(VM_BOOT_NETWORK_FLAG));
    for (k, v) in env {
        out.push(OsString::from("--env"));
        out.push(OsString::from(format!("{}={}", k, v)));
    }
    for (host, guest) in mounts {
        out.push(OsString::from("--mount"));
        out.push(OsString::from(format!("{}:{}", host, guest)));
    }
    for arg in exec_args {
        out.push(OsString::from("--arg"));
        out.push(OsString::from(arg.as_str()));
    }
    out
}

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

    /// Canonical on-disk location for the image's rootfs. Delegates to
    /// the unified `rootfs_cache` so libkrun shares its cache with the
    /// sandbox daemon and `prepare_rootfs` — pulling the same OCI ref
    /// twice across those flows now hits one directory, not three.
    /// Pinned (`foo:tag@sha256:...`) and unpinned variants of the same
    /// image collapse to one dir inside `rootfs_cache`, preserving the
    /// #1540 cache-reuse invariant across the unified path.
    pub fn image_rootfs(image: &str) -> PathBuf {
        crate::cli::rootfs_cache::canonical_path(image)
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
        let expected_arch = expected_oci_arch().to_string();
        let hints = crate::cli::rootfs_cache::CacheHints {
            consult_images_cache: true,
            ..Default::default()
        };

        // Cache hit: verify arch before short-circuiting. Legacy paths
        // can be arch-mismatched if the user switched host architectures.
        if let Some(cached) = crate::cli::rootfs_cache::resolve_cached(image, &hints) {
            let cached_arch = read_cached_rootfs_arch(&cached);
            let arch_match = cached_arch
                .as_deref()
                .map(|a| a == expected_arch)
                .unwrap_or(false);
            if arch_match {
                tracing::info!(image = %image, path = %cached.display(), "image rootfs cached, skipping pull");
                let size_bytes = fs_dir_size(&cached).ok();
                return Ok(ImageInfo {
                    image: image.to_string(),
                    size_bytes,
                });
            }
            tracing::warn!(
                image = %image,
                expected_arch = %expected_arch,
                cached_arch = ?cached_arch,
                path = %cached.display(),
                "cached rootfs architecture mismatch, rebuilding cache"
            );
            // Wipe the mismatched copy (could be canonical or legacy —
            // either way its bytes are wrong for this host) and fall
            // through to pull.
            let _ = std::fs::remove_dir_all(&cached);
        }

        tracing::info!(image = %image, "pulling OCI image via libkrun");
        let image_for_log = image.to_string();
        let rootfs_dir = crate::cli::rootfs_cache::ensure_rootfs(
            image,
            &crate::cli::rootfs_cache::CacheHints::default(),
            move || {
                tracing::info!(image = %image_for_log, "pulling OCI image via libkrun");
            },
        )
        .await?;

        let hosts_path = rootfs_dir.join("etc/hosts");
        if !hosts_path.exists() {
            let _ = std::fs::write(&hosts_path, "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n");
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
        // Resolve through the unified cache so we read from wherever
        // the rootfs actually lives (canonical or legacy).
        let hints = crate::cli::rootfs_cache::CacheHints {
            consult_images_cache: true,
            ..Default::default()
        };
        let rootfs_dir = crate::cli::rootfs_cache::resolve_cached(image, &hints)
            .unwrap_or_else(|| Self::image_rootfs(image));
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

        // Prefer a populated cache hit (canonical or legacy); only pull
        // if neither location has the image.
        let rootfs_hints = crate::cli::rootfs_cache::CacheHints {
            consult_images_cache: true,
            ..Default::default()
        };
        let rootfs_dir = match crate::cli::rootfs_cache::resolve_cached(&spec.image, &rootfs_hints)
        {
            Some(p) => p,
            None => {
                tracing::info!(image = %spec.image, "rootfs not found, pulling automatically");
                eprintln!("  Pulling rootfs ({})...", spec.image);
                self.pull(&spec.image).await?;
                // pull() wrote to canonical_path; resolve_cached will find it.
                crate::cli::rootfs_cache::resolve_cached(&spec.image, &rootfs_hints)
                    .unwrap_or_else(|| Self::image_rootfs(&spec.image))
            }
        };

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

        let vcpus = spec
            .cpu_limit
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|v| v.ceil().max(1.0) as u32)
            .unwrap_or(2);
        let ram_mib = spec
            .memory_limit
            .as_deref()
            .and_then(k8s_mem_to_mib)
            .unwrap_or_else(|| "2048".to_string());

        let pid_file_path = Self::pid_file(&spec.name);
        let console_output_path = Self::stdout_log(&spec.name);
        // Control channel for host-driven fast restarts. The socket is
        // colocated with the pid file under ~/.iii/managed/<name>/ so
        // supervisor_ctl::control_socket_path resolves to the same place
        // the watcher and stop handler use. Without this, iii-init's
        // supervisor mode stays dormant and every source edit falls back
        // to a full VM restart.
        let control_sock_path = worker_dir.join("control.sock");
        // Shell-exec channel alongside the control channel. `iii worker
        // exec` connects to shell.sock; the in-VM dispatcher thread
        // handles requests. Absent => exec refuses with a clear error.
        let shell_sock_path = worker_dir.join("shell.sock");

        let image_env = read_oci_env(&worker_rootfs);
        let mut caller_env: HashMap<String, String> = image_env.into_iter().collect();
        for (key, value) in &spec.env {
            caller_env.insert(key.clone(), value.clone());
        }
        let merged_env = build_vm_env(caller_env);
        let env_pairs: Vec<(String, String)> = merged_env.into_iter().collect();

        let mut cmd = std::process::Command::new(&self_exe);
        cmd.arg("__vm-boot");
        for boot_arg in vm_boot_args_oci(
            &worker_rootfs,
            &exec_path,
            &workdir,
            vcpus,
            &ram_mib,
            &pid_file_path,
            &console_output_path,
            &control_sock_path,
            &shell_sock_path,
            &env_pairs,
            &exec_args,
        ) {
            cmd.arg(boot_arg);
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

    fn parse_vm_boot_args(boot_args: Vec<OsString>) -> crate::cli::vm_boot::VmBootArgs {
        use clap::Parser;

        #[derive(Parser)]
        struct Wrapper {
            #[command(flatten)]
            args: crate::cli::vm_boot::VmBootArgs,
        }

        let mut argv: Vec<OsString> = vec![OsString::from("test")];
        argv.extend(boot_args);
        Wrapper::parse_from(argv).args
    }

    #[test]
    fn vm_boot_args_oci_enables_network_and_roundtrips() {
        let env = vec![("III_URL".to_string(), "ws://localhost:3111".to_string())];
        let exec_args = vec!["--url".to_string(), "ws://localhost:3111".to_string()];

        let parsed = parse_vm_boot_args(vm_boot_args_oci(
            Path::new("/tmp/rootfs"),
            "/bin/sh",
            "/",
            4,
            "2048",
            Path::new("/tmp/vm.pid"),
            Path::new("/tmp/console.log"),
            Path::new("/tmp/control.sock"),
            Path::new("/tmp/shell.sock"),
            &env,
            &exec_args,
        ));

        assert!(parsed.network);
        assert_eq!(parsed.rootfs, "/tmp/rootfs");
        assert_eq!(parsed.exec, "/bin/sh");
        assert_eq!(parsed.workdir, "/");
        assert_eq!(parsed.vcpus, 4);
        assert_eq!(parsed.ram, 2048);
        assert_eq!(parsed.pid_file.as_deref(), Some("/tmp/vm.pid"));
        assert_eq!(parsed.console_output.as_deref(), Some("/tmp/console.log"));
        assert_eq!(parsed.control_sock.as_deref(), Some("/tmp/control.sock"));
        assert_eq!(parsed.shell_sock.as_deref(), Some("/tmp/shell.sock"));
        assert_eq!(parsed.env, vec!["III_URL=ws://localhost:3111".to_string()]);
        assert_eq!(parsed.arg, exec_args);
    }

    #[test]
    fn vm_boot_args_dev_enables_network_and_roundtrips() {
        let env = vec![("III_URL".to_string(), "ws://localhost:3111".to_string())];
        let mounts = vec![("/host/src".to_string(), "/guest/src".to_string())];
        let exec_args = vec!["-c".to_string(), "exec bash /run.sh".to_string()];

        let parsed = parse_vm_boot_args(vm_boot_args_dev(
            Path::new("/tmp/rootfs"),
            "/bin/sh",
            2,
            2048,
            Path::new("/tmp/control.sock"),
            Path::new("/tmp/shell.sock"),
            &env,
            &mounts,
            &exec_args,
        ));

        assert!(parsed.network);
        assert_eq!(parsed.rootfs, "/tmp/rootfs");
        assert_eq!(parsed.exec, "/bin/sh");
        assert_eq!(parsed.workdir, "/workspace");
        assert_eq!(parsed.vcpus, 2);
        assert_eq!(parsed.ram, 2048);
        assert_eq!(parsed.control_sock.as_deref(), Some("/tmp/control.sock"));
        assert_eq!(parsed.shell_sock.as_deref(), Some("/tmp/shell.sock"));
        assert_eq!(parsed.env, vec!["III_URL=ws://localhost:3111".to_string()]);
        assert_eq!(parsed.mount, vec!["/host/src:/guest/src".to_string()]);
        assert_eq!(parsed.arg, exec_args);
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
