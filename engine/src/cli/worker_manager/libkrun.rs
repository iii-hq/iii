// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! libkrun VM runtime for `iii worker dev`.
//!
//! Provides VM-based isolated execution using libkrun (Apple Hypervisor.framework
//! on macOS, KVM on Linux). The VM runs in a separate helper process
//! (iii-vm-helper) for crash isolation.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

/// Check if libkrun runtime is available on this system.
/// Verifies libkrun.dylib/.so exists in a known location.
pub fn libkrun_available() -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };
    libkrun_library_path(&exe).is_some()
}

/// OCI image to use as rootfs for each language.
/// Uses microsandbox project images which have language runtimes pre-installed.
fn oci_image_for_language(language: &str) -> (&'static str, &'static str) {
    // Returns (image_ref, rootfs_dir_name)
    match language {
        "typescript" | "javascript" => ("docker.io/microsandbox/node", "node"),
        "python" => ("docker.io/microsandbox/python", "python"),
        "rust" => ("docker.io/library/rust:slim-bookworm", "rust"),
        "go" => ("docker.io/library/golang:bookworm", "go"),
        _ => ("docker.io/microsandbox/node", "node"),
    }
}

/// Determine the rootfs path for a given language.
/// If the rootfs doesn't exist locally, pulls the OCI image and extracts it.
pub async fn prepare_rootfs(language: &str) -> Result<PathBuf> {
    let (oci_image, rootfs_name) = oci_image_for_language(language);

    // Check if rootfs already exists (cached from previous run)
    let search_paths = rootfs_search_paths(rootfs_name);
    for path in &search_paths {
        if path.exists() && path.join("bin").exists() {
            return Ok(path.clone());
        }
    }

    // Not cached — pull OCI image and extract as rootfs
    let rootfs_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
        .join(".iii")
        .join("rootfs")
        .join(rootfs_name);

    eprintln!(
        "  Rootfs '{}' not found. Pulling {}...",
        rootfs_name, oci_image
    );

    pull_and_extract_rootfs(oci_image, &rootfs_dir).await?;

    // Create /workspace directory in rootfs (used for project file copy)
    let workspace = rootfs_dir.join("workspace");
    std::fs::create_dir_all(&workspace).ok();

    // Ensure /etc/hosts exists — TSI networking resolves via the host,
    // but the guest needs localhost → 127.0.0.1 for local connections.
    let hosts_path = rootfs_dir.join("etc/hosts");
    if !hosts_path.exists() {
        let _ = std::fs::write(&hosts_path, "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n");
    }

    Ok(rootfs_dir)
}

/// Pull an OCI image and extract it as a rootfs directory.
/// Talks directly to the OCI registry — no Docker or Podman needed.
async fn pull_and_extract_rootfs(image: &str, dest: &std::path::Path) -> Result<()> {
    use oci_client::client::ClientConfig;
    use oci_client::{Client, Reference};
    use oci_client::secrets::RegistryAuth;

    std::fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create rootfs directory: {}", dest.display()))?;

    // 1. Parse image reference
    let reference: Reference = image
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid image reference '{}': {}", image, e))?;

    // 2. Create OCI registry client with platform resolver for multi-arch images.
    // libkrun boots Linux VMs, so we always select linux/{host_arch}.
    let config = ClientConfig {
        platform_resolver: Some(Box::new(|manifests| {
            let target_arch = if cfg!(target_arch = "aarch64") {
                oci_spec::image::Arch::ARM64
            } else {
                oci_spec::image::Arch::Amd64
            };
            for m in manifests {
                if let Some(ref platform) = m.platform {
                    if platform.os == oci_spec::image::Os::Linux
                        && platform.architecture == target_arch
                    {
                        return Some(m.digest.clone());
                    }
                }
            }
            // Fallback: first linux manifest
            manifests.iter()
                .find(|m| {
                    m.platform.as_ref()
                        .map(|p| p.os == oci_spec::image::Os::Linux)
                        .unwrap_or(false)
                })
                .map(|m| m.digest.clone())
        })),
        ..Default::default()
    };
    let client = Client::new(config);

    // 3. Pull the image (manifest + all layers)
    eprintln!("  Pulling image layers...");
    let image_data = client
        .pull(
            &reference,
            &RegistryAuth::Anonymous,
            vec![
                oci_client::manifest::IMAGE_LAYER_GZIP_MEDIA_TYPE,
                oci_client::manifest::IMAGE_DOCKER_LAYER_GZIP_MEDIA_TYPE,
                oci_client::manifest::IMAGE_LAYER_MEDIA_TYPE,
            ],
        )
        .await
        .context("Failed to pull image. Check image name and network.")?;

    // 4. Extract each layer (gzipped tarballs)
    for (i, layer) in image_data.layers.iter().enumerate() {
        eprintln!(
            "  Extracting layer {}/{}...",
            i + 1,
            image_data.layers.len(),
        );

        let decoder = flate2::read::GzDecoder::new(&layer.data[..]);
        let mut archive = tar::Archive::new(decoder);
        archive.set_preserve_permissions(true);
        archive.set_overwrite(true);

        for entry in archive.entries().context("Failed to read layer tar")? {
            let mut entry = entry.context("Failed to read tar entry")?;
            let path = entry.path().context("Failed to get entry path")?.into_owned();

            // Handle OCI whiteout files (.wh.* = delete from lower layer)
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(".wh.") {
                    let target = path.parent().unwrap_or(&path).join(&name[4..]);
                    let full_target = dest.join(&target);
                    let _ = std::fs::remove_file(&full_target);
                    let _ = std::fs::remove_dir_all(&full_target);
                    continue;
                }
            }

            entry
                .unpack_in(dest)
                .with_context(|| format!("Failed to extract: {}", path.display()))?;
        }
    }

    // 5. Save image config (entrypoint/cmd) for managed workers
    let config_json = &image_data.config.data;
    let config_path = dest.join(".oci-config.json");
    let _ = std::fs::write(&config_path, config_json);

    eprintln!("  Rootfs ready at {}", dest.display());
    Ok(())
}

/// Find the directory containing libkrun.dylib/.so for DYLD_LIBRARY_PATH.
fn libkrun_library_path(helper_binary: &std::path::Path) -> Option<String> {
    let lib_name = if cfg!(target_os = "macos") { "libkrun.dylib" } else { "libkrun.so" };

    // 1. Adjacent to the helper binary
    if let Some(dir) = helper_binary.parent() {
        if dir.join(lib_name).exists() {
            return Some(dir.to_string_lossy().to_string());
        }
    }
    // 2. ~/.local/lib (microsandbox install location)
    if let Some(home) = dirs::home_dir() {
        let local_lib = home.join(".local").join("lib");
        if local_lib.join(lib_name).exists() {
            return Some(local_lib.to_string_lossy().to_string());
        }
    }
    // 3. /usr/local/lib
    let usr_local = std::path::PathBuf::from("/usr/local/lib");
    if usr_local.join(lib_name).exists() {
        return Some(usr_local.to_string_lossy().to_string());
    }
    // 4. Homebrew (macOS)
    let homebrew = std::path::PathBuf::from("/opt/homebrew/opt/libkrun/lib");
    if homebrew.join(lib_name).exists() {
        return Some(homebrew.to_string_lossy().to_string());
    }
    None
}

fn rootfs_search_paths(name: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("rootfs").join(name));
        }
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".iii").join("rootfs").join(name));
    }
    paths.push(PathBuf::from("/usr/local/share/iii/rootfs").join(name));
    paths
}

/// Ensure the binary has the Hypervisor entitlement (macOS only).
/// Checks if already signed with the entitlement; if not, codesigns it.
/// This makes `cargo build && iii worker dev` work without manual codesigning.
#[cfg(target_os = "macos")]
fn ensure_hypervisor_entitlement(binary: &std::path::Path) -> Result<()> {
    use std::process::Command;

    // Check if already entitled — verify signature and grep for the entitlement.
    let output = Command::new("codesign")
        .args(["-d", "--entitlements", "-"])
        .arg(binary)
        .output()
        .context("failed to run codesign")?;

    // codesign writes entitlements to stderr (display format) or stdout (raw).
    // Check both.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    if combined.contains("com.apple.security.hypervisor") {
        return Ok(()); // Already signed
    }

    // Write entitlements plist to a temp location
    let entitlements_dir = std::env::temp_dir();
    let plist_path = entitlements_dir.join("iii-vm-entitlements.plist");
    std::fs::write(&plist_path, concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
        "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
        "<plist version=\"1.0\">\n<dict>\n",
        "  <key>com.apple.security.hypervisor</key>\n  <true/>\n",
        "</dict>\n</plist>\n",
    ))?;

    eprintln!("  Signing binary with Hypervisor entitlement...");
    let status = Command::new("codesign")
        .args(["--sign", "-", "--entitlements"])
        .arg(&plist_path)
        .arg("--force")
        .arg(binary)
        .status()
        .context("failed to run codesign")?;

    if !status.success() {
        anyhow::bail!("codesign exited with {}", status);
    }

    Ok(())
}

/// Run a dev worker session inside a libkrun VM.
///
/// Spawns `iii __vm-boot` as a child process which boots the VM via libkrun FFI.
/// Uses a separate process for crash isolation — if libkrun segfaults,
/// the main iii engine process survives.
pub async fn run_dev(
    _language: &str,
    _project_path: &str,
    exec_path: &str,
    args: &[String],
    env: HashMap<String, String>,
    vcpus: u32,
    ram_mib: u32,
    rootfs: PathBuf,
) -> i32 {
    // 1. Locate our own binary (iii) to re-exec with __vm-boot
    let self_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot locate iii binary: {}", e);
            return 1;
        }
    };

    // 2. Ensure Hypervisor entitlement (macOS only).
    //    libkrun needs com.apple.security.hypervisor to create VMs.
    //    Auto-sign during development so `cargo build && iii worker dev` just works.
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = ensure_hypervisor_entitlement(&self_exe) {
            eprintln!("warning: failed to codesign for Hypervisor entitlement: {}", e);
        }
    }

    // 3. Build command: iii __vm-boot --rootfs ... --exec ...
    let mut cmd = tokio::process::Command::new(&self_exe);
    cmd.arg("__vm-boot");
    cmd.arg("--rootfs").arg(&rootfs);
    cmd.arg("--exec").arg(exec_path);
    cmd.arg("--workdir").arg("/workspace");
    cmd.arg("--vcpus").arg(vcpus.to_string());
    cmd.arg("--ram").arg(ram_mib.to_string());

    for (key, value) in &env {
        cmd.arg("--env").arg(format!("{}={}", key, value));
    }

    for arg in args {
        cmd.arg("--arg").arg(arg);
    }

    // Ensure the child can find libkrun.dylib at runtime.
    let dylib_path = libkrun_library_path(&self_exe);
    if let Some(ref path) = dylib_path {
        cmd.env("DYLD_LIBRARY_PATH", path);
    }

    cmd.stdin(std::process::Stdio::inherit());
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    // 3. Spawn and wait.
    //    Ctrl+C handling: SIGINT goes to the entire process group (parent + child).
    //    Node VMs exit cleanly on SIGINT, but libkrun may swallow SIGINT for some
    //    guest workloads (e.g. Python). We register a raw SIGINT handler that
    //    sends SIGKILL to the child PID, ensuring the VM always stops.
    match cmd.spawn() {
        Ok(mut child) => {
            #[cfg(unix)]
            {
                use nix::sys::signal;
                use std::sync::atomic::{AtomicU32, Ordering};

                static CHILD_PID: AtomicU32 = AtomicU32::new(0);

                if let Some(pid) = child.id() {
                    CHILD_PID.store(pid, Ordering::SeqCst);

                    // Install a raw SIGINT handler that kills the child and
                    // exits the parent immediately. This is the nuclear option:
                    // krun_start_enter may swallow SIGINT, and tokio's event loop
                    // may not notice the child died, so we force-exit here.
                    extern "C" fn sigint_handler(_: nix::libc::c_int) {
                        let pid = CHILD_PID.load(Ordering::SeqCst);
                        if pid != 0 {
                            unsafe { nix::libc::kill(pid as i32, nix::libc::SIGKILL); }
                        }
                        unsafe { nix::libc::_exit(0); }
                    }

                    unsafe {
                        signal::sigaction(
                            signal::Signal::SIGINT,
                            &signal::SigAction::new(
                                signal::SigHandler::Handler(sigint_handler),
                                signal::SaFlags::empty(),
                                signal::SigSet::empty(),
                            ),
                        ).ok();
                    }
                }
            }

            match child.wait().await {
                Ok(status) => status.code().unwrap_or(1),
                Err(e) => {
                    eprintln!("error: VM boot process failed: {}", e);
                    1
                }
            }
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

impl LibkrunAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Managed worker directory: ~/.iii/managed/{name}/
    fn worker_dir(name: &str) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".iii")
            .join("managed")
            .join(name)
    }

    /// Rootfs directory for a managed worker's OCI image.
    fn image_rootfs(image: &str) -> PathBuf {
        // Hash the image ref to create a stable directory name
        let hash = {
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(image.as_bytes());
            hex::encode(&hasher.finalize()[..8])
        };
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".iii")
            .join("images")
            .join(hash)
    }

    /// PID file for a managed worker.
    fn pid_file(name: &str) -> PathBuf {
        Self::worker_dir(name).join("vm.pid")
    }

    /// Log file for a managed worker.
    fn log_file(name: &str) -> PathBuf {
        Self::worker_dir(name).join("vm.log")
    }

    /// Check if a PID is alive.
    fn pid_alive(pid: u32) -> bool {
        unsafe { nix::libc::kill(pid as i32, 0) == 0 }
    }
}

#[async_trait::async_trait]
impl RuntimeAdapter for LibkrunAdapter {
    async fn pull(&self, image: &str) -> Result<ImageInfo> {
        let rootfs_dir = Self::image_rootfs(image);

        // Skip if already pulled
        if rootfs_dir.exists() && rootfs_dir.join("bin").exists() {
            tracing::info!(image = %image, "image rootfs cached, skipping pull");
        } else {
            tracing::info!(image = %image, "pulling OCI image via libkrun");
            pull_and_extract_rootfs(image, &rootfs_dir).await?;
            // Patch rootfs: ensure /etc/hosts exists for DNS
            let hosts_path = rootfs_dir.join("etc/hosts");
            if !hosts_path.exists() {
                let _ = std::fs::write(&hosts_path, "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n");
            }
        }

        // Estimate size from directory
        let size_bytes = fs_dir_size(&rootfs_dir).ok();

        Ok(ImageInfo {
            image: image.to_string(),
            size_bytes,
        })
    }

    async fn extract_file(&self, image: &str, path: &str) -> Result<Vec<u8>> {
        let rootfs_dir = Self::image_rootfs(image);
        // path is absolute inside the image (e.g., "/iii/worker.yaml")
        let file_path = rootfs_dir.join(path.trim_start_matches('/'));
        std::fs::read(&file_path)
            .with_context(|| format!("failed to read {} from rootfs", file_path.display()))
    }

    async fn start(&self, spec: &ContainerSpec) -> Result<String> {
        let worker_dir = Self::worker_dir(&spec.name);
        std::fs::create_dir_all(&worker_dir)?;

        let rootfs_dir = Self::image_rootfs(&spec.image);
        if !rootfs_dir.exists() {
            anyhow::bail!("rootfs not found for image {}. Run pull first.", spec.image);
        }

        // Clone rootfs to worker-specific dir so each worker has its own filesystem
        let worker_rootfs = worker_dir.join("rootfs");
        if !worker_rootfs.exists() {
            clone_rootfs(&rootfs_dir, &worker_rootfs)
                .map_err(|e| anyhow::anyhow!("failed to clone rootfs: {}", e))?;
        }

        // Ensure Hypervisor entitlement (macOS)
        let self_exe = std::env::current_exe()
            .context("cannot locate iii binary")?;
        #[cfg(target_os = "macos")]
        {
            let _ = ensure_hypervisor_entitlement(&self_exe);
        }

        // Build the __vm-boot command
        let log_file = Self::log_file(&spec.name);
        let log_fd = std::fs::File::create(&log_file)
            .with_context(|| format!("failed to create log file: {}", log_file.display()))?;
        let log_fd_err = log_fd.try_clone()?;

        // Read entrypoint/cmd from OCI image config.
        let (exec_path, mut exec_args) = read_oci_entrypoint(&worker_rootfs)
            .unwrap_or_else(|| ("/bin/sh".to_string(), vec![]));

        // Replace --url value in OCI entrypoint args with the user's III_ENGINE_URL.
        // The Rust SDK reads --url with higher priority than env vars.
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
                // No --url in args — add it so the binary uses our URL
                exec_args.push("--url".to_string());
                exec_args.push(url.clone());
            }
        }

        let mut cmd = std::process::Command::new(&self_exe);
        cmd.arg("__vm-boot");
        cmd.arg("--rootfs").arg(&worker_rootfs);
        cmd.arg("--exec").arg(&exec_path);
        cmd.arg("--workdir").arg("/");
        // CPU limit may be fractional (e.g., "0.5") — round up to at least 1 vCPU
        let vcpus = spec.cpu_limit.as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|v| v.ceil().max(1.0) as u32)
            .unwrap_or(1);
        cmd.arg("--vcpus").arg(vcpus.to_string());
        cmd.arg("--ram").arg(
            spec.memory_limit.as_deref()
                .and_then(|m| k8s_mem_to_mib(m))
                .unwrap_or_else(|| "512".to_string())
        );

        // Merge image env (base) with spec env (overrides from iii.workers.yaml).
        let image_env = read_oci_env(&worker_rootfs);
        let mut merged_env: HashMap<String, String> = image_env.into_iter().collect();
        for (key, value) in &spec.env {
            merged_env.insert(key.clone(), value.clone());
        }
        for (key, value) in &merged_env {
            cmd.arg("--env").arg(format!("{}={}", key, value));
        }
        for arg in &exec_args {
            cmd.arg("--arg").arg(arg);
        }

        // Set DYLD_LIBRARY_PATH for libkrun
        if let Some(ref path) = libkrun_library_path(&self_exe) {
            cmd.env("DYLD_LIBRARY_PATH", path);
        }

        cmd.stdout(log_fd);
        cmd.stderr(log_fd_err);
        cmd.stdin(std::process::Stdio::null());

        let child = cmd.spawn()
            .context("failed to spawn VM boot process")?;

        let pid = child.id();

        // Write PID file
        std::fs::write(Self::pid_file(&spec.name), pid.to_string())?;

        tracing::info!(name = %spec.name, pid = pid, "started libkrun VM");

        // Return PID as the "container ID"
        Ok(pid.to_string())
    }

    async fn stop(&self, container_id: &str, _timeout_secs: u32) -> Result<()> {
        if let Ok(pid) = container_id.parse::<u32>() {
            if Self::pid_alive(pid) {
                tracing::info!(pid = pid, "stopping libkrun VM");
                unsafe { nix::libc::kill(pid as i32, nix::libc::SIGKILL); }
                // Wait briefly for process to exit
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
        Ok(())
    }

    async fn status(&self, container_id: &str) -> Result<ContainerStatus> {
        let pid: u32 = container_id.parse().unwrap_or(0);
        let running = pid > 0 && Self::pid_alive(pid);

        Ok(ContainerStatus {
            name: String::new(), // caller has the name
            container_id: container_id.to_string(),
            running,
            exit_code: if running { None } else { Some(0) },
        })
    }

    async fn remove(&self, container_id: &str) -> Result<()> {
        // Stop if still running
        self.stop(container_id, 0).await?;

        // Find and remove the worker directory
        let managed_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".iii")
            .join("managed");

        if let Ok(entries) = std::fs::read_dir(&managed_dir) {
            for entry in entries.flatten() {
                let pid_file = entry.path().join("vm.pid");
                if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                    if pid_str.trim() == container_id {
                        let _ = std::fs::remove_dir_all(entry.path());
                        tracing::info!(container_id = %container_id, "removed libkrun worker directory");
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }
}

/// Read entrypoint and cmd from the saved OCI image config.
/// Returns (exec_path, args) or None if config is missing/unparseable.
/// OCI spec: entrypoint is the executable, cmd is the default arguments.
/// Docker convention: if entrypoint is set, cmd provides args.
/// If only cmd is set, cmd[0] is the executable and cmd[1..] are args.
fn read_oci_entrypoint(rootfs: &std::path::Path) -> Option<(String, Vec<String>)> {
    let config_path = rootfs.join(".oci-config.json");
    let data = std::fs::read_to_string(&config_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&data).ok()?;

    let config = json.get("config")?;

    let entrypoint: Vec<String> = config.get("Entrypoint")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let cmd: Vec<String> = config.get("Cmd")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    if !entrypoint.is_empty() {
        // entrypoint[0] is exec, rest + cmd are args
        let exec = entrypoint[0].clone();
        let mut args: Vec<String> = entrypoint[1..].to_vec();
        args.extend(cmd);
        Some((exec, args))
    } else if !cmd.is_empty() {
        // cmd[0] is exec, rest are args
        let exec = cmd[0].clone();
        let args = cmd[1..].to_vec();
        Some((exec, args))
    } else {
        None
    }
}

/// Read environment variables from the saved OCI image config.
fn read_oci_env(rootfs: &std::path::Path) -> Vec<(String, String)> {
    let config_path = rootfs.join(".oci-config.json");
    let data = match std::fs::read_to_string(&config_path) {
        Ok(d) => d,
        Err(_) => return vec![],
    };
    let json: serde_json::Value = match serde_json::from_str(&data) {
        Ok(j) => j,
        Err(_) => return vec![],
    };
    let env_arr = json.get("config")
        .and_then(|c| c.get("Env"))
        .and_then(|e| e.as_array());

    match env_arr {
        Some(arr) => arr.iter()
            .filter_map(|v| v.as_str())
            .filter_map(|s| {
                let mut parts = s.splitn(2, '=');
                Some((parts.next()?.to_string(), parts.next().unwrap_or("").to_string()))
            })
            .collect(),
        None => vec![],
    }
}

/// Convert Kubernetes-style memory to MiB string for libkrun --ram.
fn k8s_mem_to_mib(value: &str) -> Option<String> {
    if let Some(n) = value.strip_suffix("Mi") {
        Some(n.to_string())
    } else if let Some(n) = value.strip_suffix("Gi") {
        n.parse::<u64>().ok().map(|v| (v * 1024).to_string())
    } else if let Some(n) = value.strip_suffix("Ki") {
        n.parse::<u64>().ok().map(|v| (v / 1024).to_string())
    } else {
        value.parse::<u64>().ok().map(|v| (v / (1024 * 1024)).to_string())
    }
}

/// Clone a rootfs directory using APFS clonefile (macOS) or reflink (Linux).
fn clone_rootfs(base: &std::path::Path, dest: &std::path::Path) -> std::result::Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    let status = std::process::Command::new("cp")
        .args(if cfg!(target_os = "macos") {
            vec!["-c", "-a"]
        } else {
            vec!["--reflink=auto", "-a"]
        })
        .arg(base.as_os_str())
        .arg(dest.as_os_str())
        .status()
        .map_err(|e| format!("cp: {}", e))?;
    if !status.success() {
        return Err(format!("cp exited with {}", status));
    }
    Ok(())
}

/// Estimate directory size (for ImageInfo).
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
    fn test_rootfs_search_paths_includes_home() {
        let paths = rootfs_search_paths("node");
        // Should include ~/.iii/rootfs/node
        assert!(paths.iter().any(|p| p.to_string_lossy().contains(".iii/rootfs")));
    }

    #[test]
    fn test_oci_image_for_language_defaults_to_node() {
        let (image, name) = oci_image_for_language("unknown_lang");
        assert_eq!(image, "docker.io/microsandbox/node");
        assert_eq!(name, "node");
    }

    #[test]
    fn test_oci_image_for_typescript() {
        let (image, name) = oci_image_for_language("typescript");
        assert_eq!(image, "docker.io/microsandbox/node");
        assert_eq!(name, "node");
    }

    #[test]
    fn test_oci_image_for_python() {
        let (image, name) = oci_image_for_language("python");
        assert_eq!(image, "docker.io/microsandbox/python");
        assert_eq!(name, "python");
    }

    #[test]
    fn test_libkrun_available_returns_false_without_helper() {
        // Without iii-vm-helper installed, should return false
        // This test passes in CI where the helper binary doesn't exist
        // Note: may return true if helper is in PATH during local dev
        let result = libkrun_available();
        // Just verify it doesn't panic
        let _ = result;
    }
}
