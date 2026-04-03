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
fn oci_image_for_language(language: &str) -> (&'static str, &'static str) {
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

    let search_paths = rootfs_search_paths(rootfs_name);
    for path in &search_paths {
        if path.exists() && path.join("bin").exists() {
            return Ok(path.clone());
        }
    }

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

    let workspace = rootfs_dir.join("workspace");
    std::fs::create_dir_all(&workspace).ok();

    let hosts_path = rootfs_dir.join("etc/hosts");
    if !hosts_path.exists() {
        let _ = std::fs::write(&hosts_path, "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n");
    }

    Ok(rootfs_dir)
}

/// Extract a single OCI layer with safety limits.
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

        if path.is_absolute() {
            anyhow::bail!(
                "Layer {}/{}: absolute path in tar entry: {}",
                layer_index + 1,
                layer_count,
                path.display()
            );
        }

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

        *total_size += entry_size;
        if *total_size > MAX_TOTAL_SIZE {
            anyhow::bail!(
                "Layer {}/{}: total extraction size exceeded {} bytes",
                layer_index + 1,
                layer_count,
                MAX_TOTAL_SIZE
            );
        }

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
async fn pull_and_extract_rootfs(image: &str, dest: &std::path::Path) -> Result<()> {
    use oci_client::client::ClientConfig;
    use oci_client::secrets::RegistryAuth;
    use oci_client::{Client, Reference};

    std::fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create rootfs directory: {}", dest.display()))?;

    let reference: Reference = image
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid image reference '{}': {}", image, e))?;

    let host_arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };

    let available_platforms = Arc::new(Mutex::new(Vec::<String>::new()));
    let platforms_capture = Arc::clone(&available_platforms);
    let target_arch_str = host_arch.to_string();

    let config = ClientConfig {
        platform_resolver: Some(Box::new(move |manifests| {
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
            None
        })),
        ..Default::default()
    };
    let client = Client::new(config);

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

    let mut total_size: u64 = 0;
    let layer_count = image_data.layers.len();
    for (i, layer) in image_data.layers.iter().enumerate() {
        eprintln!("  Extracting layer {}/{}...", i + 1, layer_count);
        extract_layer_with_limits(&layer.data, dest, i, layer_count, &mut total_size)?;
    }

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
#[cfg(target_os = "macos")]
fn ensure_macos_entitlements(binary: &std::path::Path) -> Result<()> {
    use std::process::Command;

    let output = Command::new("codesign")
        .args(["-d", "--entitlements", "-"])
        .arg(binary)
        .output()
        .context("failed to run codesign")?;

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    if combined.contains("com.apple.security.hypervisor")
        && combined.contains("com.apple.security.cs.disable-library-validation")
    {
        return Ok(());
    }

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

    let status = Command::new("codesign")
        .args(["--sign", "-", "--entitlements"])
        .arg(&plist_path)
        .arg("--force")
        .arg(binary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("failed to run codesign")?;

    if !status.success() {
        anyhow::bail!("codesign exited with {}", status);
    }

    Ok(())
}

/// Keep the terminal's `ISIG` flag enabled so Ctrl+C generates SIGINT.
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
/// Spawns `iii-worker __vm-boot` as a child process which boots the VM via libkrun FFI.
/// Uses a separate process for crash isolation.
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
    let self_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot locate iii-worker binary: {}", e);
            return 1;
        }
    };

    #[cfg(target_os = "macos")]
    {
        if let Err(e) = ensure_macos_entitlements(&self_exe) {
            eprintln!(
                "warning: failed to codesign for Hypervisor entitlement: {}",
                e
            );
        }
    }

    // Build command: iii-worker __vm-boot --rootfs ... --exec ...
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

    // libkrunfw is the only external dylib loaded at runtime.
    // msb_krun (the VMM) is compiled directly into the iii-worker binary.
    if let Some(fw_dir) = crate::cli::firmware::resolve::resolve_libkrunfw_dir() {
        cmd.env(
            crate::cli::firmware::resolve::lib_path_env_var(),
            fw_dir.to_string_lossy().as_ref(),
        );
    }

    // Detach child into its own session
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            Ok(())
        });
    }

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

    fn worker_dir(name: &str) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".iii")
            .join("managed")
            .join(name)
    }

    fn image_rootfs(image: &str) -> PathBuf {
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

    fn pid_file(name: &str) -> PathBuf {
        Self::worker_dir(name).join("vm.pid")
    }

    fn logs_dir(name: &str) -> PathBuf {
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
        let worker_dir = Self::worker_dir(&spec.name);
        std::fs::create_dir_all(&worker_dir)?;

        let rootfs_dir = Self::image_rootfs(&spec.image);
        if !rootfs_dir.exists() {
            tracing::info!(image = %spec.image, "rootfs not found, pulling automatically");
            eprintln!("  Rootfs not found. Pulling {}...", spec.image);
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

        // Ensure iii-init is available and copy to rootfs if not embedded
        if !iii_filesystem::init::has_init() {
            let init_path = crate::cli::firmware::download::ensure_init_binary().await?;
            let dest = worker_rootfs.join("init.krun");
            std::fs::copy(&init_path, &dest)
                .with_context(|| format!("failed to copy iii-init to rootfs: {}", dest.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
            }
        }

        let self_exe = std::env::current_exe().context("cannot locate iii-worker binary")?;
        #[cfg(target_os = "macos")]
        {
            let _ = ensure_macos_entitlements(&self_exe);
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

        let mut cmd = std::process::Command::new(&self_exe);
        cmd.arg("__vm-boot");
        cmd.arg("--rootfs").arg(&worker_rootfs);
        cmd.arg("--exec").arg(&exec_path);
        cmd.arg("--workdir").arg("/");
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

        let pid_file_path = Self::pid_file(&spec.name);
        cmd.arg("--pid-file").arg(&pid_file_path);

        cmd.arg("--console-output")
            .arg(Self::stdout_log(&spec.name));

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

        // msb_krun (the VMM) is compiled directly into the iii-worker binary.
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
        std::fs::write(Self::pid_file(&spec.name), pid.to_string())?;

        tracing::info!(name = %spec.name, pid = pid, "started libkrun VM");

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
        let exec = entrypoint[0].clone();
        let mut args: Vec<String> = entrypoint[1..].to_vec();
        args.extend(cmd);
        Some((exec, args))
    } else if !cmd.is_empty() {
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
    fn test_logs_dir_path() {
        let dir = LibkrunAdapter::logs_dir("test-worker");
        assert!(dir.to_string_lossy().contains(".iii/managed/test-worker/logs"));
    }

    #[test]
    fn test_libkrun_available_returns_bool() {
        let result = libkrun_available();
        let _ = result;
    }
}
