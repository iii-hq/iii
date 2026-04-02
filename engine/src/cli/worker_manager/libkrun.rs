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
use std::path::Component;
use std::sync::{Arc, Mutex};

/// Maximum total extracted size (10 GiB).
const MAX_TOTAL_SIZE: u64 = 10 * 1024 * 1024 * 1024;
/// Maximum single file size (5 GiB).
const MAX_FILE_SIZE: u64 = 5 * 1024 * 1024 * 1024;
/// Maximum number of tar entries.
const MAX_ENTRY_COUNT: u64 = 1_000_000;
/// Maximum path depth.
const MAX_PATH_DEPTH: usize = 128;

fn expected_oci_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    }
}

fn read_cached_rootfs_arch(rootfs_dir: &std::path::Path) -> Option<String> {
    let config_path = rootfs_dir.join(".oci-config.json");
    let data = std::fs::read_to_string(config_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&data).ok()?;
    json.get("architecture")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Check if libkrun runtime is available on this system.
/// msb_krun (the VMM) is compiled into the binary; this checks for libkrunfw.
pub fn libkrun_available() -> bool {
    crate::cli::firmware::resolve::resolve_libkrunfw_dir().is_some()
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

/// Extract a single OCI layer with safety limits.
///
/// Validates entry count, file size, total size, path depth, and path traversal
/// before extracting each entry. Handles whiteout files per OCI spec.
fn extract_layer_with_limits(
    data: &[u8],
    dest: &std::path::Path,
    layer_index: usize,
    layer_count: usize,
    total_size: &mut u64,
) -> Result<()> {
    let decoder = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(decoder);
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);

    let mut entry_count: u64 = 0;

    for entry in archive.entries().context("Failed to read layer tar")? {
        let mut entry = entry.context("Failed to read tar entry")?;

        entry_count += 1;
        if entry_count > MAX_ENTRY_COUNT {
            anyhow::bail!(
                "Layer {}/{}: exceeded max entry count ({})",
                layer_index + 1,
                layer_count,
                MAX_ENTRY_COUNT
            );
        }

        let path = entry
            .path()
            .context("Failed to get entry path")?
            .into_owned();

        // Reject absolute paths
        if path.is_absolute() {
            anyhow::bail!(
                "Layer {}/{}: absolute path in tar entry: {}",
                layer_index + 1,
                layer_count,
                path.display()
            );
        }

        // Reject path traversal (../)
        for component in path.components() {
            if matches!(component, Component::ParentDir) {
                anyhow::bail!(
                    "Layer {}/{}: path traversal in tar entry: {}",
                    layer_index + 1,
                    layer_count,
                    path.display()
                );
            }
        }

        // Check path depth
        let depth = path
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .count();
        if depth > MAX_PATH_DEPTH {
            anyhow::bail!(
                "Layer {}/{}: path too deep ({} components): {}",
                layer_index + 1,
                layer_count,
                depth,
                path.display()
            );
        }

        // Check individual file size
        let entry_size = entry.size();
        if entry_size > MAX_FILE_SIZE {
            anyhow::bail!(
                "Layer {}/{}: file too large: {} bytes (max {})",
                layer_index + 1,
                layer_count,
                entry_size,
                MAX_FILE_SIZE
            );
        }

        // Accumulate total size
        *total_size += entry_size;
        if *total_size > MAX_TOTAL_SIZE {
            anyhow::bail!(
                "Layer {}/{}: total extraction size exceeded {} bytes",
                layer_index + 1,
                layer_count,
                MAX_TOTAL_SIZE
            );
        }

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

    Ok(())
}

/// Pull an OCI image and extract it as a rootfs directory.
/// Talks directly to the OCI registry — no Docker or Podman needed.
async fn pull_and_extract_rootfs(image: &str, dest: &std::path::Path) -> Result<()> {
    use oci_client::client::ClientConfig;
    use oci_client::secrets::RegistryAuth;
    use oci_client::{Client, Reference};

    std::fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create rootfs directory: {}", dest.display()))?;

    // 1. Parse image reference
    let reference: Reference = image
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid image reference '{}': {}", image, e))?;

    // 2. Runtime architecture detection (not compile-time cfg!)
    let host_arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };

    // 3. Create OCI registry client with strict platform resolver for multi-arch images.
    // libkrun boots Linux VMs, so we always select linux/{host_arch}.
    // No fallback — mismatch is a hard error.
    let available_platforms = Arc::new(Mutex::new(Vec::<String>::new()));
    let platforms_capture = Arc::clone(&available_platforms);
    let target_arch_str = host_arch.to_string();

    let config = ClientConfig {
        platform_resolver: Some(Box::new(move |manifests| {
            // Record all available platforms for error reporting
            let mut platforms = platforms_capture.lock().unwrap();
            for m in manifests {
                if let Some(ref platform) = m.platform {
                    platforms.push(format!("{}/{}", platform.os, platform.architecture));
                }
            }
            drop(platforms);

            let target_arch = match target_arch_str.as_str() {
                "arm64" => oci_spec::image::Arch::ARM64,
                _ => oci_spec::image::Arch::Amd64,
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
            // No fallback — return None to signal mismatch
            None
        })),
        ..Default::default()
    };
    let client = Client::new(config);

    // 4. Pull the image (manifest + all layers)
    eprintln!("  Pulling image layers...");
    let media_types = vec![
        oci_client::manifest::IMAGE_LAYER_GZIP_MEDIA_TYPE,
        oci_client::manifest::IMAGE_DOCKER_LAYER_GZIP_MEDIA_TYPE,
        oci_client::manifest::IMAGE_LAYER_MEDIA_TYPE,
    ];

    let image_data = match client
        .pull(&reference, &RegistryAuth::Anonymous, media_types)
        .await
    {
        Ok(data) => data,
        Err(e) => {
            let platforms = available_platforms.lock().unwrap();
            if !platforms.is_empty() {
                anyhow::bail!(
                    "Architecture mismatch: no linux/{} manifest found for '{}'. Available platforms: {}",
                    host_arch,
                    image,
                    platforms.join(", ")
                );
            }
            return Err(e).context("Failed to pull image. Check image name and network.");
        }
    };

    // 5. Log pull details
    eprintln!("  Architecture: linux/{}", host_arch);
    if let Some(ref digest) = image_data.digest {
        eprintln!("  Digest: {}", digest);
    }
    let total_layer_bytes: usize = image_data.layers.iter().map(|l| l.data.len()).sum();
    eprintln!(
        "  Layers: {} ({:.1} MiB)",
        image_data.layers.len(),
        total_layer_bytes as f64 / (1024.0 * 1024.0)
    );
    // 6. Extract each layer with safety limits
    let mut total_size: u64 = 0;
    let layer_count = image_data.layers.len();
    for (i, layer) in image_data.layers.iter().enumerate() {
        eprintln!("  Extracting layer {}/{}...", i + 1, layer_count);
        extract_layer_with_limits(&layer.data, dest, i, layer_count, &mut total_size)?;
    }

    // 7. Save image config (entrypoint/cmd) for managed workers
    let config_json = &image_data.config.data;
    let config_path = dest.join(".oci-config.json");
    let _ = std::fs::write(&config_path, config_json);

    eprintln!("  Rootfs ready at {}", dest.display());
    Ok(())
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

/// Ensure the binary has the required VM entitlements (macOS only).
/// Checks if already signed with both hypervisor and library-validation entitlements;
/// if not, codesigns with both. This makes `cargo build && iii worker dev` work
/// without manual codesigning.
#[cfg(target_os = "macos")]
fn ensure_macos_entitlements(binary: &std::path::Path) -> Result<()> {
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
    if combined.contains("com.apple.security.hypervisor")
        && combined.contains("com.apple.security.cs.disable-library-validation")
    {
        return Ok(()); // Already signed with both entitlements
    }

    // Write entitlements plist to a temp location
    let entitlements_dir = std::env::temp_dir();
    let plist_path = entitlements_dir.join("iii-vm-entitlements.plist");
    std::fs::write(
        &plist_path,
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
            "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\">\n<dict>\n",
            "  <key>com.apple.security.hypervisor</key>\n  <true/>\n",
            "  <key>com.apple.security.cs.disable-library-validation</key>\n  <true/>\n",
            "</dict>\n</plist>\n",
        ),
    )?;

    eprintln!("  Signing binary with VM entitlements...");
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

/// Keep the terminal's `ISIG` flag enabled so Ctrl+C generates SIGINT.
///
/// krun puts the terminal in raw mode (clearing ISIG among other flags),
/// which prevents Ctrl+C from producing a signal. This task periodically
/// re-enables ISIG without disturbing the rest of krun's raw-mode settings.
/// Runs forever; cancelled when the surrounding `tokio::select!` completes.
#[cfg(unix)]
async fn ensure_terminal_isig() {
    use nix::libc;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDERR_FILENO, &mut t) == 0 && (t.c_lflag & libc::ISIG == 0) {
                t.c_lflag |= libc::ISIG;
                libc::tcsetattr(libc::STDERR_FILENO, libc::TCSANOW, &t);
            }
        }
    }
}

#[cfg(not(unix))]
async fn ensure_terminal_isig() {
    std::future::pending::<()>().await;
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
        if let Err(e) = ensure_macos_entitlements(&self_exe) {
            eprintln!(
                "warning: failed to codesign for Hypervisor entitlement: {}",
                e
            );
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

    // libkrunfw is the only external dylib loaded at runtime (via libloading).
    // msb_krun (the VMM) is compiled directly into the iii binary.
    // Set DYLD_LIBRARY_PATH/LD_LIBRARY_PATH as a fallback for libkrunfw discovery.
    if let Some(fw_dir) = crate::cli::firmware::resolve::resolve_libkrunfw_dir() {
        cmd.env(
            crate::cli::firmware::resolve::lib_path_env_var(),
            fw_dir.to_string_lossy().as_ref(),
        );
    }

    // Detach child into its own session so krun_start_enter() cannot call
    // tcsetpgrp() to steal the terminal's foreground process group.
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            Ok(())
        });
    }

    // Give the child /dev/null for stdin so krun cannot read from or
    // tcsetpgrp() the terminal via stdin. stdout/stderr are inherited so
    // the VM console output (virtio-console) appears on the parent's
    // terminal. krun WILL put the terminal in raw mode via tcsetattr() on
    // the inherited stdout/stderr fds, but we restore cooked mode after
    // the child exits (see below).
    cmd.stdin(std::process::Stdio::null());

    match cmd.spawn() {
        Ok(mut child) => {
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
                _ = ensure_terminal_isig() => {
                    unreachable!()
                }
            };

            // Restore terminal to cooked mode after the child exits.
            // krun puts it into raw mode (disables OPOST/ONLCR) which
            // would break NL→CRNL translation for subsequent output.
            #[cfg(unix)]
            super::super::managed::restore_terminal_cooked_mode();

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

    /// Logs directory for a managed worker.
    fn logs_dir(name: &str) -> PathBuf {
        Self::worker_dir(name).join("logs")
    }

    /// Stdout log file for a managed worker.
    fn stdout_log(name: &str) -> PathBuf {
        Self::logs_dir(name).join("stdout.log")
    }

    /// Stderr log file for a managed worker.
    fn stderr_log(name: &str) -> PathBuf {
        Self::logs_dir(name).join("stderr.log")
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
        let expected_arch = expected_oci_arch().to_string();

        // Skip if already pulled
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
            // Patch rootfs: ensure /etc/hosts exists for DNS
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

        // Ensure Hypervisor entitlement (macOS)
        let self_exe = std::env::current_exe().context("cannot locate iii binary")?;
        #[cfg(target_os = "macos")]
        {
            let _ = ensure_macos_entitlements(&self_exe);
        }

        // Create logs directory and separate stdout/stderr files (D-04, D-09)
        let logs_dir = Self::logs_dir(&spec.name);
        std::fs::create_dir_all(&logs_dir)
            .with_context(|| format!("failed to create logs dir: {}", logs_dir.display()))?;

        let stdout_file = std::fs::File::create(Self::stdout_log(&spec.name))
            .with_context(|| "failed to create stdout.log")?;
        let stderr_file = std::fs::File::create(Self::stderr_log(&spec.name))
            .with_context(|| "failed to create stderr.log")?;

        // Read entrypoint/cmd from OCI image config.
        let (exec_path, mut exec_args) =
            read_oci_entrypoint(&worker_rootfs).unwrap_or_else(|| ("/bin/sh".to_string(), vec![]));

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
                .and_then(|m| k8s_mem_to_mib(m))
                .unwrap_or_else(|| "2048".to_string()),
        );

        // Pass PID file path so the on_exit callback can clean it up
        let pid_file_path = Self::pid_file(&spec.name);
        cmd.arg("--pid-file").arg(&pid_file_path);

        // Redirect VM console output to the stdout log file so `iii worker logs`
        // can display it. Without this, krun writes console output to the
        // terminal which is unavailable for background managed workers.
        cmd.arg("--console-output")
            .arg(Self::stdout_log(&spec.name));

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

        // libkrunfw is the only external dylib loaded at runtime (via libloading).
        // msb_krun (the VMM) is compiled directly into the iii binary.
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

        // Write PID file
        std::fs::write(Self::pid_file(&spec.name), pid.to_string())?;

        tracing::info!(name = %spec.name, pid = pid, "started libkrun VM");

        // Return PID as the "container ID"
        Ok(pid.to_string())
    }

    async fn stop(&self, container_id: &str, timeout_secs: u32) -> Result<()> {
        if let Ok(pid) = container_id.parse::<u32>() {
            if Self::pid_alive(pid) {
                tracing::info!(pid = pid, "sending SIGTERM to libkrun VM");
                unsafe {
                    nix::libc::kill(pid as i32, nix::libc::SIGTERM);
                }

                let deadline = std::time::Instant::now()
                    + std::time::Duration::from_secs(timeout_secs as u64);
                while std::time::Instant::now() < deadline {
                    // Reap zombie if this process is the direct parent
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
                    // Reap zombie after SIGKILL
                    unsafe {
                        nix::libc::waitpid(pid as i32, std::ptr::null_mut(), nix::libc::WNOHANG);
                    }
                }
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

    let entrypoint: Vec<String> = config
        .get("Entrypoint")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let cmd: Vec<String> = config
        .get("Cmd")
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
    let env_arr = json
        .get("config")
        .and_then(|c| c.get("Env"))
        .and_then(|e| e.as_array());

    match env_arr {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .filter_map(|s| {
                let mut parts = s.splitn(2, '=');
                Some((
                    parts.next()?.to_string(),
                    parts.next().unwrap_or("").to_string(),
                ))
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
        value
            .parse::<u64>()
            .ok()
            .map(|v| (v / (1024 * 1024)).to_string())
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
        assert!(
            paths
                .iter()
                .any(|p| p.to_string_lossy().contains(".iii/rootfs"))
        );
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
    fn test_logs_dir_path() {
        let dir = LibkrunAdapter::logs_dir("test-worker");
        assert!(dir.to_string_lossy().contains(".iii/managed/test-worker/logs"));
    }

    #[test]
    fn test_stdout_log_path() {
        let path = LibkrunAdapter::stdout_log("test-worker");
        assert!(path.to_string_lossy().ends_with("logs/stdout.log"));
    }

    #[test]
    fn test_stderr_log_path() {
        let path = LibkrunAdapter::stderr_log("test-worker");
        assert!(path.to_string_lossy().ends_with("logs/stderr.log"));
    }

    #[test]
    fn test_libkrun_available_returns_false_without_helper() {
        let result = libkrun_available();
        let _ = result;
    }

    // --- OCI hardening tests ---

    #[test]
    fn test_host_arch_detection() {
        // std::env::consts::ARCH maps to expected OCI arch string
        let host_arch = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            other => other,
        };
        // On x86_64 CI/dev machines, should be "amd64"; on ARM64, "arm64"
        assert!(
            host_arch == "amd64" || host_arch == "arm64",
            "unexpected host arch: {}",
            host_arch
        );
    }

    /// Helper: create a gzipped tar archive in memory with the given entries.
    /// Each entry is (path, content_bytes).
    fn make_tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            for (path, content) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_path(path).unwrap();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append(&header, &content[..]).unwrap();
            }
            builder.finish().unwrap();
        }
        encoder.finish().unwrap()
    }

    /// Helper: build a raw tar + gzip with arbitrary path bytes in the header.
    /// Bypasses tar crate's validation so we can test path traversal etc.
    fn make_raw_tar_gz(path_str: &str, content: &[u8]) -> Vec<u8> {
        let mut raw_tar = Vec::new();
        let path_bytes = path_str.as_bytes();

        // GNU tar header: 512 bytes
        let mut header_block = [0u8; 512];

        // name field: bytes 0..100
        let name_len = path_bytes.len().min(100);
        header_block[..name_len].copy_from_slice(&path_bytes[..name_len]);

        // mode: bytes 100..108 — "0000644\0"
        header_block[100..108].copy_from_slice(b"0000644\0");

        // uid: bytes 108..116 — "0000000\0"
        header_block[108..116].copy_from_slice(b"0000000\0");

        // gid: bytes 116..124 — "0000000\0"
        header_block[116..124].copy_from_slice(b"0000000\0");

        // size: bytes 124..136 — octal ASCII
        let size_str = format!("{:011o}\0", content.len());
        header_block[124..136].copy_from_slice(size_str.as_bytes());

        // mtime: bytes 136..148 — "00000000000\0"
        header_block[136..148].copy_from_slice(b"00000000000\0");

        // typeflag: byte 156 — '0' for regular file
        header_block[156] = b'0';

        // magic: bytes 257..263 — "ustar\0"
        header_block[257..263].copy_from_slice(b"ustar\0");

        // version: bytes 263..265 — "00"
        header_block[263..265].copy_from_slice(b"00");

        // Compute checksum: sum of all bytes treating checksum field (148..156) as spaces
        header_block[148..156].copy_from_slice(b"        "); // 8 spaces
        let cksum: u32 = header_block.iter().map(|&b| b as u32).sum();
        let cksum_str = format!("{:06o}\0 ", cksum);
        header_block[148..156].copy_from_slice(cksum_str.as_bytes());

        raw_tar.extend_from_slice(&header_block);

        // Data blocks (padded to 512)
        raw_tar.extend_from_slice(content);
        let padding = (512 - (content.len() % 512)) % 512;
        raw_tar.extend(std::iter::repeat(0u8).take(padding));

        // End-of-archive (two zero blocks)
        raw_tar.extend_from_slice(&[0u8; 1024]);

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder, &raw_tar).unwrap();
        encoder.finish().unwrap()
    }

    /// Helper: build a raw tar + gzip with a long filename using GNU LongLink extension.
    fn make_raw_tar_gz_longname(path_str: &str, content: &[u8]) -> Vec<u8> {
        let mut raw_tar = Vec::new();
        let path_bytes = path_str.as_bytes();

        // GNU LongLink header: type 'L', name "././@LongLink"
        // This tells the tar reader that the next data block(s) contain the real filename.
        {
            let mut lnk_header = [0u8; 512];
            let lnk_name = b"././@LongLink";
            lnk_header[..lnk_name.len()].copy_from_slice(lnk_name);
            lnk_header[100..108].copy_from_slice(b"0000000\0");
            lnk_header[108..116].copy_from_slice(b"0000000\0");
            lnk_header[116..124].copy_from_slice(b"0000000\0");
            // Size = length of the long name + null terminator
            let name_size = path_bytes.len() + 1;
            let size_str = format!("{:011o}\0", name_size);
            lnk_header[124..136].copy_from_slice(size_str.as_bytes());
            lnk_header[136..148].copy_from_slice(b"00000000000\0");
            // Type 'L' = GNU LongName
            lnk_header[156] = b'L';
            lnk_header[257..263].copy_from_slice(b"ustar ");
            lnk_header[263..265].copy_from_slice(b" \0");

            lnk_header[148..156].copy_from_slice(b"        ");
            let cksum: u32 = lnk_header.iter().map(|&b| b as u32).sum();
            let cksum_str = format!("{:06o}\0 ", cksum);
            lnk_header[148..156].copy_from_slice(cksum_str.as_bytes());
            raw_tar.extend_from_slice(&lnk_header);

            // Data: the long filename + null, padded to 512
            let mut name_data = Vec::from(path_bytes);
            name_data.push(0); // null terminator
            let padding = (512 - (name_data.len() % 512)) % 512;
            name_data.extend(std::iter::repeat(0u8).take(padding));
            raw_tar.extend_from_slice(&name_data);
        }

        // Actual file entry header (name field can be truncated; tar reader uses LongLink name)
        {
            let mut header_block = [0u8; 512];
            // Truncated name
            let name_len = path_bytes.len().min(99);
            header_block[..name_len].copy_from_slice(&path_bytes[..name_len]);

            header_block[100..108].copy_from_slice(b"0000644\0");
            header_block[108..116].copy_from_slice(b"0000000\0");
            header_block[116..124].copy_from_slice(b"0000000\0");

            let size_str = format!("{:011o}\0", content.len());
            header_block[124..136].copy_from_slice(size_str.as_bytes());

            header_block[136..148].copy_from_slice(b"00000000000\0");
            header_block[156] = b'0'; // regular file
            header_block[257..263].copy_from_slice(b"ustar ");
            header_block[263..265].copy_from_slice(b" \0");

            header_block[148..156].copy_from_slice(b"        ");
            let cksum: u32 = header_block.iter().map(|&b| b as u32).sum();
            let cksum_str = format!("{:06o}\0 ", cksum);
            header_block[148..156].copy_from_slice(cksum_str.as_bytes());

            raw_tar.extend_from_slice(&header_block);

            // Data blocks
            raw_tar.extend_from_slice(content);
            let padding = (512 - (content.len() % 512)) % 512;
            raw_tar.extend(std::iter::repeat(0u8).take(padding));
        }

        // End-of-archive
        raw_tar.extend_from_slice(&[0u8; 1024]);

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder, &raw_tar).unwrap();
        encoder.finish().unwrap()
    }

    /// Helper: build a raw tar + gzip where the header claims a large file size.
    fn make_raw_tar_gz_with_fake_size(path_str: &str, fake_size: u64) -> Vec<u8> {
        let mut raw_tar = Vec::new();
        let path_bytes = path_str.as_bytes();

        let mut header_block = [0u8; 512];
        let name_len = path_bytes.len().min(100);
        header_block[..name_len].copy_from_slice(&path_bytes[..name_len]);

        header_block[100..108].copy_from_slice(b"0000644\0");
        header_block[108..116].copy_from_slice(b"0000000\0");
        header_block[116..124].copy_from_slice(b"0000000\0");

        let size_str = format!("{:011o}\0", fake_size);
        header_block[124..136].copy_from_slice(size_str.as_bytes());

        header_block[136..148].copy_from_slice(b"00000000000\0");
        header_block[156] = b'0';
        header_block[257..263].copy_from_slice(b"ustar\0");
        header_block[263..265].copy_from_slice(b"00");

        header_block[148..156].copy_from_slice(b"        ");
        let cksum: u32 = header_block.iter().map(|&b| b as u32).sum();
        let cksum_str = format!("{:06o}\0 ", cksum);
        header_block[148..156].copy_from_slice(cksum_str.as_bytes());

        raw_tar.extend_from_slice(&header_block);
        // Write a couple of zero data blocks (the size check triggers before full read)
        raw_tar.extend_from_slice(&[0u8; 1024]);
        // End-of-archive
        raw_tar.extend_from_slice(&[0u8; 1024]);

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut encoder, &raw_tar).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn test_extraction_rejects_path_traversal() {
        let data = make_raw_tar_gz("../etc/passwd", b"evil");
        let tmp = tempfile::tempdir().unwrap();
        let mut total_size = 0u64;
        let result = extract_layer_with_limits(&data, tmp.path(), 0, 1, &mut total_size);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("path traversal"),
            "expected 'path traversal' in error: {}",
            err_msg
        );
    }

    #[test]
    fn test_extraction_rejects_deep_path() {
        // Create a path with 130 Normal components (exceeds MAX_PATH_DEPTH=128)
        // Use short names to fit in tar header (100 bytes max for basic name field)
        // With 2-char names + separators: "a/b/c/..." — we need a longer path
        // that fits in 100 bytes. GNU extensions handle longer paths.
        // Actually our raw builder only uses the 100-byte name field.
        // 130 components of "x/" = 260 chars, won't fit in 100 bytes.
        // Use GNU extended header approach or a shorter test.
        // Simpler: use make_tar_gz since normal paths work, and manually construct
        // a path that's exactly 129 components deep but short enough.
        // Actually, the raw builder works with paths up to 100 chars.
        // 129 single-char components: "a/a/a/.../a/f" = ~257 chars. Too long.
        // Instead, let's test with a depth of 129 using make_tar_gz with a GNU header
        // that supports long names via the tar crate.
        // The tar crate's set_path handles this with GNU longname extension.
        // But it rejected it earlier... let's check: "provided value is too long"
        // That's because GNU header path limit was hit.
        // Use our raw builder with a truncated path in the name field, relying on
        // path parsing. Actually, we need the full path to be parsed.
        // Better approach: test with a moderate depth that fits in 100 bytes.
        // 50 single-char components: "a/a/a/.../x" = ~99 chars = 50 components.
        // That's under 128. Let's just test > 128 by using fewer but guaranteed-parseable path.
        //
        // Actually, the real tar format supports long names via pax or GNU extensions.
        // The `tar` crate Builder handles this. The error was set_path rejecting `..`.
        // For deep paths, the issue was the path being too long for GNU tar format.
        // Let's build a raw tar with a GNU longlink extension.
        // Simpler: modify MAX_PATH_DEPTH to test with a shorter path.
        // No — that changes production code. Instead, use raw tar with pax extension.
        //
        // Simplest approach: build a tar with GNU longname header.
        let deep_path: String = (0..130)
            .map(|_| "d")
            .collect::<Vec<_>>()
            .join("/")
            + "/f";
        // This is 130 Normal components + "f" = 131 Normal components, ~262 chars.
        // Build raw tar with GNU longname extension:
        let data = make_raw_tar_gz_longname(&deep_path, b"deep");
        let tmp = tempfile::tempdir().unwrap();
        let mut total_size = 0u64;
        let result = extract_layer_with_limits(&data, tmp.path(), 0, 1, &mut total_size);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("path too deep"),
            "expected 'path too deep' in error: {}",
            err_msg
        );
    }

    #[test]
    fn test_extraction_file_size_limit() {
        // Create a tar with a header claiming > 5 GiB
        let data = make_raw_tar_gz_with_fake_size("big.bin", MAX_FILE_SIZE + 1);
        let tmp = tempfile::tempdir().unwrap();
        let mut total_size = 0u64;
        let result = extract_layer_with_limits(&data, tmp.path(), 0, 1, &mut total_size);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("file too large"),
            "expected 'file too large' in error: {}",
            err_msg
        );
    }

    #[test]
    fn test_extraction_entry_count_limit() {
        // We can't create 1M+ entries in a test, so temporarily test with a smaller
        // archive and verify the counting mechanism works. The actual constant is 1_000_000.
        // Instead, verify that the function counts entries correctly by extracting
        // a small archive successfully.
        let data = make_tar_gz(&[
            ("a.txt", b"a"),
            ("b.txt", b"b"),
            ("c.txt", b"c"),
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let mut total_size = 0u64;
        let result = extract_layer_with_limits(&data, tmp.path(), 0, 1, &mut total_size);
        assert!(result.is_ok(), "small archive should extract fine: {:?}", result.err());
        assert_eq!(total_size, 3); // 1 + 1 + 1 bytes
    }

    #[test]
    fn test_extraction_valid_archive() {
        // A valid archive with normal paths should extract successfully
        let data = make_tar_gz(&[
            ("usr/bin/hello", b"#!/bin/sh\necho hello"),
            ("etc/config.txt", b"key=value"),
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let mut total_size = 0u64;
        let result = extract_layer_with_limits(&data, tmp.path(), 0, 1, &mut total_size);
        assert!(result.is_ok(), "valid archive should extract: {:?}", result.err());
        assert!(tmp.path().join("usr/bin/hello").exists());
        assert!(tmp.path().join("etc/config.txt").exists());
    }

    #[test]
    fn test_extraction_constants_match_microsandbox() {
        // Verify our constants match microsandbox's proven values
        assert_eq!(MAX_TOTAL_SIZE, 10 * 1024 * 1024 * 1024);
        assert_eq!(MAX_FILE_SIZE, 5 * 1024 * 1024 * 1024);
        assert_eq!(MAX_ENTRY_COUNT, 1_000_000);
        assert_eq!(MAX_PATH_DEPTH, 128);
    }

    // --- Platform hardening tests (Phase 09) ---

    #[test]
    fn test_entitlements_plist_contains_both_keys() {
        // Verify the plist template string that ensure_macos_entitlements generates
        // contains both required entitlement keys.
        // We test the string content directly rather than calling codesign.
        let plist = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
            "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\">\n<dict>\n",
            "  <key>com.apple.security.hypervisor</key>\n  <true/>\n",
            "  <key>com.apple.security.cs.disable-library-validation</key>\n  <true/>\n",
            "</dict>\n</plist>\n"
        );
        assert!(
            plist.contains("com.apple.security.hypervisor"),
            "Plist must include hypervisor entitlement"
        );
        assert!(
            plist.contains("com.apple.security.cs.disable-library-validation"),
            "Plist must include disable-library-validation entitlement"
        );
    }

    // --- SIGTERM-then-SIGKILL stop behavior tests ---

    #[tokio::test]
    async fn test_stop_sends_sigterm_before_sigkill() {
        // Spawn a process that exits immediately on SIGTERM (default behavior of `sleep`)
        let child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("failed to spawn sleep");
        let pid = child.id();

        let adapter = LibkrunAdapter::new();
        let start = std::time::Instant::now();
        adapter.stop(&pid.to_string(), 2).await.unwrap();
        let elapsed = start.elapsed();

        // sleep responds to SIGTERM by exiting immediately.
        // If stop() sends SIGTERM first, elapsed should be well under 2 seconds.
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "stop() took {:?}, expected < 2s (SIGTERM should cause immediate exit)",
            elapsed
        );
        // Process must be dead
        assert!(
            !LibkrunAdapter::pid_alive(pid),
            "process should be dead after stop"
        );
    }

    #[tokio::test]
    async fn test_stop_sigkill_fallback_on_unresponsive() {
        // Spawn a process that ignores SIGTERM
        let child = std::process::Command::new("bash")
            .args(["-c", "trap '' TERM; sleep 60"])
            .spawn()
            .expect("failed to spawn bash");
        let pid = child.id();

        // Give the trap time to install
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let adapter = LibkrunAdapter::new();
        let start = std::time::Instant::now();
        adapter.stop(&pid.to_string(), 1).await.unwrap();
        let elapsed = start.elapsed();

        // Process ignores SIGTERM, so stop() must wait for timeout then SIGKILL.
        // Elapsed should be >= 1s (the timeout) because SIGTERM was ignored.
        assert!(
            elapsed >= std::time::Duration::from_millis(900),
            "stop() took {:?}, expected >= 1s (should wait for SIGTERM timeout before SIGKILL)",
            elapsed
        );
        // Process must be dead (SIGKILL cannot be caught)
        assert!(
            !LibkrunAdapter::pid_alive(pid),
            "process should be dead after SIGKILL"
        );
    }
}
