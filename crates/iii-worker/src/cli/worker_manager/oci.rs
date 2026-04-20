// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! OCI image pulling, extraction, and rootfs search.

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Component;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Maximum total extracted size (10 GiB).
const MAX_TOTAL_SIZE: u64 = 10 * 1024 * 1024 * 1024;
/// Maximum single file size (5 GiB).
const MAX_FILE_SIZE: u64 = 5 * 1024 * 1024 * 1024;
/// Maximum number of tar entries.
const MAX_ENTRY_COUNT: u64 = 1_000_000;
/// Maximum path depth.
const MAX_PATH_DEPTH: usize = 128;

pub fn expected_oci_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    }
}

pub fn read_cached_rootfs_arch(rootfs_dir: &std::path::Path) -> Option<String> {
    let config_path = rootfs_dir.join(".oci-config.json");
    let data = std::fs::read_to_string(config_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&data).ok()?;
    json.get("architecture")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// OCI image to use as rootfs for each language.
pub fn oci_image_for_language(language: &str) -> (&'static str, &'static str) {
    match language {
        "typescript" | "javascript" => ("docker.io/iiidev/node:latest", "node"),
        "python" => ("docker.io/iiidev/python:latest", "python"),
        "rust" => ("docker.io/library/rust:slim-bookworm", "rust"),
        _ => ("docker.io/iiidev/node:latest", "node"),
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

    eprintln!("  Pulling rootfs {} ({})...", rootfs_name, oci_image);

    pull_and_extract_rootfs(oci_image, &rootfs_dir).await?;

    let workspace = rootfs_dir.join("workspace");
    std::fs::create_dir_all(&workspace).ok();

    let hosts_path = rootfs_dir.join("etc/hosts");
    if !hosts_path.exists() {
        let _ = std::fs::write(&hosts_path, "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n");
    }

    Ok(rootfs_dir)
}

/// Setuid + setgid bit mask.
///
/// Both bits combined (0o4000 | 0o2000). Applied with a bitwise-NOT to
/// clear them: `mode & !SETID_BITS`. Stripped during OCI extraction — see
/// [`extract_layer_with_limits`] for the rationale.
const SETID_BITS: u32 = 0o6000;

/// Extract a single OCI layer with safety limits.
///
/// Setuid and setgid bits are stripped from every regular file as it lands.
/// The microVM rootfs is served read-only through PassthroughFs with no UID
/// translation: host ownership surfaces verbatim inside the guest. When the
/// extracting user is not root (the common case), setuid binaries carry the
/// host user's UID + setuid bit, and the guest kernel's setuid semantics
/// *drop* the caller from euid=0 to that non-zero UID on exec — the classic
/// example being `/bin/mount` refusing to run with "must be superuser".
/// Stripping setuid/setgid at extraction time lets these binaries inherit
/// the PID-1 euid (root) on exec, which is what a single-tenant microVM
/// guest actually wants. There is no privilege boundary *inside* the VM
/// for setuid to defend, so removing the bit is strictly a fix.
pub fn extract_layer_with_limits(
    data: &[u8],
    dest: &std::path::Path,
    layer_index: usize,
    layer_count: usize,
    total_size: &mut u64,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

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

        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.starts_with(".wh.")
        {
            let target = path.parent().unwrap_or(&path).join(&name[4..]);
            let full_target = dest.join(&target);
            let _ = std::fs::remove_file(&full_target);
            let _ = std::fs::remove_dir_all(&full_target);
            continue;
        }

        let entry_type = entry.header().entry_type();
        let header_mode = entry.header().mode().unwrap_or(0);

        let unpacked = entry
            .unpack_in(dest)
            .with_context(|| format!("Failed to extract: {}", path.display()))?;

        // `unpack_in` returns Ok(false) when it intentionally skips an entry
        // (e.g. path traversal, no parent). Don't touch the dest path in
        // that case — the file wasn't written and the resolved path could
        // point outside the rootfs.
        if !unpacked {
            continue;
        }

        // Strip setuid/setgid from regular files. See function doc for why.
        // Symlinks are skipped: chmod on a symlink path follows to the
        // target, which may live outside the rootfs.
        if matches!(
            entry_type,
            tar::EntryType::Regular | tar::EntryType::Continuous
        ) && header_mode & SETID_BITS != 0
        {
            let target = dest.join(&path);
            if let Ok(meta) = std::fs::metadata(&target) {
                let current = meta.permissions().mode();
                if current & SETID_BITS != 0 {
                    let mut perms = meta.permissions();
                    perms.set_mode(current & !SETID_BITS);
                    if let Err(e) = std::fs::set_permissions(&target, perms) {
                        tracing::warn!(
                            path = %target.display(),
                            error = %e,
                            "failed to strip setuid/setgid bits; setuid binaries will drop PID-1 privileges inside the guest",
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/// Collect registry hosts that should use plain HTTP instead of HTTPS.
/// Reads from the `III_INSECURE_REGISTRIES` env var (comma-separated).
/// `localhost` and `127.0.0.1` (any port) are always treated as insecure.
fn insecure_registries(reference: &oci_client::Reference) -> Vec<String> {
    let mut registries: Vec<String> = vec!["localhost".to_string(), "127.0.0.1".to_string()];

    let host = reference.registry();
    if let Some(hostname) = host.split(':').next()
        && (hostname == "localhost" || hostname == "127.0.0.1")
        && !registries.contains(&host.to_string())
    {
        registries.push(host.to_string());
    }

    if let Ok(extra) = std::env::var("III_INSECURE_REGISTRIES") {
        for r in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if !registries.contains(&r.to_string()) {
                registries.push(r.to_string());
            }
        }
    }

    registries
}

/// Pull an OCI image and extract it as a rootfs directory.
pub async fn pull_and_extract_rootfs(image: &str, dest: &std::path::Path) -> Result<()> {
    use oci_client::client::{ClientConfig, ClientProtocol};
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

    let http_exceptions = insecure_registries(&reference);
    let protocol = if http_exceptions.is_empty() {
        ClientProtocol::Https
    } else {
        ClientProtocol::HttpsExcept(http_exceptions)
    };

    let config = ClientConfig {
        protocol,
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
                if let Some(ref platform) = m.platform
                    && platform.os == oci_spec::image::Os::Linux
                    && platform.architecture == target_arch
                {
                    return Some(m.digest.clone());
                }
            }
            None
        })),
        ..Default::default()
    };
    let client = Client::new(config);

    eprintln!("  Pulling image layers...");
    let media_types: Vec<&str> = vec![
        oci_client::manifest::IMAGE_LAYER_GZIP_MEDIA_TYPE,
        oci_client::manifest::IMAGE_DOCKER_LAYER_GZIP_MEDIA_TYPE,
        oci_client::manifest::IMAGE_LAYER_MEDIA_TYPE,
    ];

    const MAX_PULL_ATTEMPTS: u32 = 3;
    let mut image_data = None;
    let mut last_err = None;

    for attempt in 0..MAX_PULL_ATTEMPTS {
        if attempt > 0 {
            let delay = std::time::Duration::from_secs(3u64.pow(attempt - 1));
            eprintln!(
                "  Retrying in {}s (attempt {}/{})...",
                delay.as_secs(),
                attempt + 1,
                MAX_PULL_ATTEMPTS
            );
            tokio::time::sleep(delay).await;
        }

        match client
            .pull(&reference, &RegistryAuth::Anonymous, media_types.clone())
            .await
        {
            Ok(data) => {
                image_data = Some(data);
                break;
            }
            Err(e) => {
                eprintln!("  Pull attempt {} failed: {}", attempt + 1, e);
                last_err = Some(e);
            }
        }
    }

    let image_data = match image_data {
        Some(data) => data,
        None => {
            let e = last_err.unwrap();
            let platforms = available_platforms.lock().unwrap();
            if !platforms.is_empty() {
                anyhow::bail!(
                    "Architecture mismatch: no linux/{} manifest found for '{}'. Available platforms: {}",
                    host_arch,
                    image,
                    platforms.join(", ")
                );
            }
            return Err(e).context(format!(
                "Failed to pull image '{}'. Check image name and network connectivity.",
                image
            ));
        }
    };

    if let Some(ref digest) = image_data.digest {
        tracing::debug!(%digest, "image digest");
    }
    let total_layer_bytes: usize = image_data.layers.iter().map(|l| l.data.len()).sum();
    eprintln!(
        "  linux/{} | {} layers | {:.1} MiB",
        host_arch,
        image_data.layers.len(),
        total_layer_bytes as f64 / (1024.0 * 1024.0)
    );

    let layer_count = image_data.layers.len();
    let pb = indicatif::ProgressBar::new(layer_count as u64);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "  [{bar:40.cyan/blue}] {pos}/{len} layers extracted",
        )
        .unwrap()
        .progress_chars("=> "),
    );

    let mut total_size: u64 = 0;
    for (i, layer) in image_data.layers.iter().enumerate() {
        extract_layer_with_limits(&layer.data, dest, i, layer_count, &mut total_size)?;
        pb.inc(1);
    }
    pb.finish();

    let config_json = &image_data.config.data;
    let config_path = dest.join(".oci-config.json");
    let _ = std::fs::write(&config_path, config_json);

    tracing::info!(path = %dest.display(), "rootfs ready");
    eprintln!("  {} Rootfs ready", "\u{2713}".green());
    Ok(())
}

pub fn rootfs_search_paths(name: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        paths.push(dir.join("rootfs").join(name));
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".iii").join("rootfs").join(name));
    }
    paths.push(PathBuf::from("/usr/local/share/iii/rootfs").join(name));
    paths
}

/// Read entrypoint and cmd from the saved OCI image config.
pub fn read_oci_entrypoint(rootfs: &std::path::Path) -> Option<(String, Vec<String>)> {
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

/// Read WorkingDir from the saved OCI image config.
pub fn read_oci_workdir(rootfs: &std::path::Path) -> Option<String> {
    let config_path = rootfs.join(".oci-config.json");
    let data = std::fs::read_to_string(&config_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&data).ok()?;
    json.get("config")?
        .get("WorkingDir")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Read environment variables from the saved OCI image config.
pub fn read_oci_env(rootfs: &std::path::Path) -> Vec<(String, String)> {
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
        assert_eq!(image, "docker.io/iiidev/node:latest");
        assert_eq!(name, "node");
    }

    #[test]
    fn test_oci_image_for_typescript() {
        let (image, name) = oci_image_for_language("typescript");
        assert_eq!(image, "docker.io/iiidev/node:latest");
        assert_eq!(name, "node");
    }

    #[test]
    fn test_oci_image_for_python() {
        let (image, name) = oci_image_for_language("python");
        assert_eq!(image, "docker.io/iiidev/python:latest");
        assert_eq!(name, "python");
    }

    #[test]
    fn test_oci_image_for_rust() {
        let (image, name) = oci_image_for_language("rust");
        assert_eq!(image, "docker.io/library/rust:slim-bookworm");
        assert_eq!(name, "rust");
    }

    #[test]
    fn test_oci_image_for_go() {
        let (image, name) = oci_image_for_language("go");
        assert_eq!(image, "docker.io/iiidev/node:latest");
        assert_eq!(name, "node");
    }

    #[test]
    fn test_expected_oci_arch() {
        let arch = expected_oci_arch();
        if cfg!(target_arch = "aarch64") {
            assert_eq!(arch, "arm64");
        } else if cfg!(target_arch = "x86_64") {
            assert_eq!(arch, "amd64");
        }
    }

    #[test]
    fn test_read_oci_entrypoint_with_entrypoint_and_cmd() {
        let dir = tempfile::tempdir().unwrap();
        let config = r#"{"config": {"Entrypoint": ["/usr/bin/node"], "Cmd": ["server.js"]}}"#;
        std::fs::write(dir.path().join(".oci-config.json"), config).unwrap();
        let result = read_oci_entrypoint(dir.path()).unwrap();
        assert_eq!(result.0, "/usr/bin/node");
        assert_eq!(result.1, vec!["server.js"]);
    }

    #[test]
    fn test_read_oci_entrypoint_cmd_only() {
        let dir = tempfile::tempdir().unwrap();
        let config = r#"{"config": {"Cmd": ["/bin/sh", "-c", "echo hello"]}}"#;
        std::fs::write(dir.path().join(".oci-config.json"), config).unwrap();
        let result = read_oci_entrypoint(dir.path()).unwrap();
        assert_eq!(result.0, "/bin/sh");
        assert_eq!(result.1, vec!["-c", "echo hello"]);
    }

    #[test]
    fn test_read_oci_entrypoint_none() {
        let dir = tempfile::tempdir().unwrap();
        let config = r#"{"config": {}}"#;
        std::fs::write(dir.path().join(".oci-config.json"), config).unwrap();
        assert!(read_oci_entrypoint(dir.path()).is_none());
    }

    #[test]
    fn test_read_oci_env() {
        let dir = tempfile::tempdir().unwrap();
        let config = r#"{"config": {"Env": ["PATH=/usr/bin", "HOME=/root"]}}"#;
        std::fs::write(dir.path().join(".oci-config.json"), config).unwrap();
        let env = read_oci_env(dir.path());
        assert_eq!(
            env,
            vec![
                ("PATH".to_string(), "/usr/bin".to_string()),
                ("HOME".to_string(), "/root".to_string()),
            ]
        );
    }

    #[test]
    fn test_read_oci_env_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config = r#"{"config": {}}"#;
        std::fs::write(dir.path().join(".oci-config.json"), config).unwrap();
        let env = read_oci_env(dir.path());
        assert!(env.is_empty());
    }

    #[test]
    fn test_extract_layer_rejects_path_traversal() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();

        // Build tar bytes manually to bypass tar::Builder path validation
        let mut raw_tar = Vec::new();
        let data = b"malicious content";
        let path_bytes = b"../escape.txt";

        // 512-byte GNU tar header
        let mut header_block = [0u8; 512];
        header_block[..path_bytes.len()].copy_from_slice(path_bytes);
        // mode (offset 100, 8 bytes)
        header_block[100..107].copy_from_slice(b"0000644");
        // size (offset 124, 12 bytes) — octal
        let size_str = format!("{:011o}", data.len());
        header_block[124..135].copy_from_slice(size_str.as_bytes());
        // typeflag (offset 156) — '0' regular file
        header_block[156] = b'0';
        // magic (offset 257, 6 bytes) + version (offset 263, 2 bytes)
        header_block[257..263].copy_from_slice(b"ustar\0");
        header_block[263..265].copy_from_slice(b"00");
        // checksum (offset 148, 8 bytes): fill with spaces, compute, then write
        header_block[148..156].copy_from_slice(b"        ");
        let cksum: u32 = header_block.iter().map(|&b| b as u32).sum();
        let cksum_str = format!("{:06o}\0 ", cksum);
        header_block[148..156].copy_from_slice(cksum_str.as_bytes());

        raw_tar.extend_from_slice(&header_block);
        raw_tar.extend_from_slice(data);
        // Pad to 512-byte boundary
        let padding = 512 - (data.len() % 512);
        if padding < 512 {
            raw_tar.extend(std::iter::repeat(0u8).take(padding));
        }
        // Two zero blocks to end the archive
        raw_tar.extend(std::iter::repeat(0u8).take(1024));

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw_tar).unwrap();
        let gz_data = gz.finish().unwrap();

        let mut total_size = 0u64;
        let result = extract_layer_with_limits(&gz_data, dir.path(), 0, 1, &mut total_size);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("path traversal"), "got: {}", err_msg);
    }

    #[test]
    fn test_extract_layer_extracts_valid_tar() {
        let dir = tempfile::tempdir().unwrap();
        let mut builder = tar::Builder::new(Vec::new());

        let data = b"hello world";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "test.txt", &data[..])
            .unwrap();
        let tar_data = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_data).unwrap();
        let gz_data = gz.finish().unwrap();

        let mut total_size = 0u64;
        extract_layer_with_limits(&gz_data, dir.path(), 0, 1, &mut total_size).unwrap();

        let content = std::fs::read_to_string(dir.path().join("test.txt")).unwrap();
        assert_eq!(content, "hello world");
        assert_eq!(total_size, 11);
    }
}
