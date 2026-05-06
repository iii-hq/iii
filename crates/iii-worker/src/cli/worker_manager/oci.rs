// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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

/// OCI image to use as rootfs for each `runtime.kind` value.
///
/// Returns `(image_ref, cache_dir_name)`. Keep in sync with
/// `project::SUPPORTED_KINDS` and `project::infer_scripts`.
pub fn oci_image_for_kind(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "typescript" | "javascript" => ("docker.io/iiidev/node:latest", "node"),
        // `kind: bun` gets the oven image by default — ships bun
        // preinstalled so no bootstrap is needed and startup is fast.
        // Users can still override with `runtime.base_image` for a
        // pinned version (e.g. `oven/bun:1.3`).
        "bun" => ("docker.io/oven/bun:latest", "bun"),
        "python" => ("docker.io/iiidev/python:latest", "python"),
        "rust" => ("docker.io/library/rust:slim-bookworm", "rust"),
        _ => ("docker.io/iiidev/node:latest", "node"),
    }
}

/// Sanitize an OCI image reference into a cache-dir-safe name so two
/// workers using different base images don't collide on disk under
/// `~/.iii/cache/<slug>/`. Keeps it human-readable for easier
/// troubleshooting: `oven/bun:1` → `docker.io-oven-bun-1-<hash>`,
/// `ghcr.io/my-org/my-worker:latest` → `ghcr.io-my-org-my-worker-latest-<hash>`.
///
/// The input is first normalized through `oci_client::Reference` so
/// equivalent shorthand refs hit the same slug — `iiidev/node:latest`,
/// `docker.io/iiidev/node:latest`, and `docker.io/iiidev/node` (tag
/// inferred as `:latest`) all canonicalize to the same whole-form ref
/// and therefore share one cache dir. Refs that fail to parse (pure-
/// separator gibberish, malformed test inputs) fall through to the
/// raw string so the slug is still deterministic.
///
/// The 16-hex-digit FNV-1a suffix is derived from the canonical form.
/// It makes the slug injective: inputs that sanitize to the same
/// human-readable base (e.g. `ghcr.io/foo/bar:1` and `ghcr.io/foo-bar:1`
/// both collapse to `ghcr.io-foo-bar-1`) hash differently, so their
/// cache dirs never collide. Cost is 17 trailing characters on every
/// dir name.
pub fn rootfs_slug_for_image(image: &str) -> String {
    slug_from_text(&canonicalize_oci_ref(image.trim()))
}

/// Pre-canonicalization slug. Only used by `rootfs_cache` to locate
/// rootfses left on disk by the pre-fix hashing scheme so upgrading
/// users don't re-pull on their first post-upgrade run.
pub fn raw_rootfs_slug_for_image(image: &str) -> String {
    slug_from_text(image.trim())
}

/// Parse `image` as an OCI reference and return its canonical whole
/// form (registry/namespace/repo:tag with all defaults materialized).
/// Falls back to the raw string when the parser rejects it, so edge-
/// case inputs (test fixtures, malformed refs) still produce a stable
/// slug instead of panicking.
fn canonicalize_oci_ref(raw: &str) -> String {
    raw.parse::<oci_client::Reference>()
        .map(|r| r.whole())
        .unwrap_or_else(|_| raw.to_string())
}

/// Sanitize + hash. Kept private so the two public entry points
/// (`rootfs_slug_for_image`, `raw_rootfs_slug_for_image`) are the only
/// callers and the canonicalization boundary stays crisp.
fn slug_from_text(s: &str) -> String {
    let hash = fnv1a64(s.as_bytes());
    // Single-pass emit + coalesce runs of `-`; O(n) vs the previous
    // `while contains("--") { replace("--","-") }` O(n²) loop.
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let ch = match c {
            'A'..='Z' => c.to_ascii_lowercase(),
            'a'..='z' | '0'..='9' | '.' | '-' | '_' => c,
            _ => '-',
        };
        if ch == '-' && out.ends_with('-') {
            continue;
        }
        out.push(ch);
    }
    let base = out
        .trim_matches(|c: char| c == '-' || c == '.' || c == '_')
        .to_string();
    if base.is_empty() {
        format!("image-{hash:016x}")
    } else {
        format!("{base}-{hash:016x}")
    }
}

/// 64-bit FNV-1a. Picked over SHA because a) we only need collision
/// resistance against accidental slug overlap, not cryptographic
/// strength, and b) no external dependency — the whole hash is 4 lines.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Determine the rootfs path for a given `runtime.kind`, optionally
/// overriding the default base image. If the rootfs doesn't exist
/// locally, pulls the OCI image and extracts it.
///
/// `base_image_override` comes from `runtime.base_image` in
/// `iii.worker.yaml`. When `Some`, the override's image ref is pulled
/// into a slug-derived cache dir so it doesn't clobber the kind-default
/// rootfs (e.g. a `base_image: oven/bun:1` worker gets its own
/// `~/.iii/rootfs/oven-bun-1/` instead of replacing the shared
/// `~/.iii/rootfs/bun/` that every default `kind: bun` worker uses).
pub async fn prepare_rootfs(kind: &str, base_image_override: Option<&str>) -> Result<PathBuf> {
    let (oci_image, legacy_slug): (String, String) = match base_image_override {
        Some(img) if !img.trim().is_empty() => {
            let slug = rootfs_slug_for_image(img.trim());
            if slug.is_empty() {
                anyhow::bail!(
                    "base_image {:?} produced an empty rootfs slug — use a normal image reference like `oven/bun:1`",
                    img
                );
            }
            (img.trim().to_string(), slug)
        }
        _ => {
            let (img, name) = oci_image_for_kind(kind);
            (img.to_string(), name.to_string())
        }
    };

    // Delegate cache lookup + pull to the unified rootfs cache. Legacy
    // `~/.iii/rootfs/<kind>/` and `~/.iii/images/<sha>/` layouts are
    // consulted as fallbacks so pre-unification users don't re-pull.
    let image_for_log = oci_image.clone();
    let slug_for_log = legacy_slug.clone();
    let hints = crate::cli::rootfs_cache::CacheHints {
        legacy_kind: Some(&legacy_slug),
        consult_images_cache: true,
        ..Default::default()
    };
    let rootfs_dir = crate::cli::rootfs_cache::ensure_rootfs(&oci_image, &hints, move || {
        eprintln!("  Pulling rootfs {} ({})...", slug_for_log, image_for_log);
    })
    .await?;

    // Post-populate runtime-expected dirs even on a cache hit; these are
    // idempotent and cheap, and covers the case where a legacy rootfs
    // predates the workspace/ convention.
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

/// Collect registries that should use HTTP instead of HTTPS.
/// `localhost`/`127.0.0.1` (any port) always; anything else only via
/// `III_INSECURE_REGISTRIES` — which emits a per-pull warning so a
/// forgotten env var can't silently enable LAN MITM.
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
        let extras: Vec<&str> = extra
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "localhost" && *s != "127.0.0.1")
            .collect();
        if !extras.is_empty() {
            eprintln!(
                "  {} III_INSECURE_REGISTRIES is active — plain-HTTP pulls permitted for: {}. \
                 MITM on the LAN can serve poisoned images. Unset the env var if this is not \
                 intentional.",
                "warning:".yellow(),
                extras.join(", ")
            );
        }
        for r in extras {
            if !registries.contains(&r.to_string()) {
                registries.push(r.to_string());
            }
        }
    }

    registries
}

/// Pull an OCI image and extract it as a rootfs directory.
///
/// **Atomicity contract:** `dest` is produced via rename(2) from a sibling
/// temp directory, so observers either see the fully-extracted rootfs or
/// nothing at all. A SIGTERM / network failure / disk-full partway through
/// extraction leaves an orphaned `.tmp-*` sibling that the next pull
/// cleans up. The cache-hit check at
/// `crates/iii-worker/src/cli/worker_manager/libkrun.rs:393` can therefore
/// trust that `dest` existing with `bin/` means the rootfs is complete.
///
/// Atomicity scope is `dest` ONLY — callers can rely on
/// `dest` either existing with the full rootfs or not existing. Sibling
/// metadata (e.g. a digest sidecar in a later task) is written by the
/// caller AFTER this function returns and is not covered by the same
/// rename; a missing sidecar beside a valid rootfs is treated as a
/// cache miss upstream, which forces a re-pull that rewrites both.
pub async fn pull_and_extract_rootfs(image: &str, dest: &std::path::Path) -> Result<()> {
    use oci_client::client::{ClientConfig, ClientProtocol};
    use oci_client::secrets::RegistryAuth;
    use oci_client::{Client, Reference};

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

    // Set up the atomic-extract staging dir AFTER pull succeeds. Doing this
    // before pull would leak the temp dir on network/auth failure.
    //
    // The atomic unit is `dest` itself, NOT its parent. The staging dir
    // lives as a SIBLING of `dest` so the final rename(temp, dest) touches
    // only the one dir the caller asked for — it must never clobber the
    // parent, which may house other unrelated cache entries (e.g.
    // `~/.iii/rootfs/` is shared across base images like `node`, `bun`,
    // `python`; similarly `~/.iii/images/<hash>/` may have a sibling
    // `digest.txt` added in a later task).
    let dest_parent = dest.parent();
    let dest_basename = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("rootfs");

    let (extract_root, temp_to_promote) = match dest_parent {
        Some(parent) => {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let tmp_name = format!(".{}.tmp-{}-{}", dest_basename, std::process::id(), nanos);
            let tmp_dir = parent.join(tmp_name);
            let _ = std::fs::remove_dir_all(&tmp_dir);
            std::fs::create_dir_all(&tmp_dir).with_context(|| {
                format!(
                    "Failed to create temp extract directory: {}",
                    tmp_dir.display()
                )
            })?;
            (tmp_dir.clone(), Some((tmp_dir, dest.to_path_buf())))
        }
        None => {
            // Degenerate: dest has no parent. Skip atomicity.
            std::fs::create_dir_all(dest).with_context(|| {
                format!("Failed to create rootfs directory: {}", dest.display())
            })?;
            (dest.to_path_buf(), None)
        }
    };

    let mut total_size: u64 = 0;
    let extract_result: Result<()> = (|| {
        for (i, layer) in image_data.layers.iter().enumerate() {
            extract_layer_with_limits(&layer.data, &extract_root, i, layer_count, &mut total_size)?;
            pb.inc(1);
        }
        pb.finish();

        let config_json = &image_data.config.data;
        let config_path = extract_root.join(".oci-config.json");
        let _ = std::fs::write(&config_path, config_json);
        Ok(())
    })();

    // Extraction done. If we were in the atomic path, either promote the
    // temp dir into place OR clean it up.
    if let Some((tmp_dir, final_dir)) = temp_to_promote {
        match extract_result {
            Ok(()) => {
                // If `final_dir` already exists (leftover from a prior
                // interrupted run, or a race with another `iii worker add`),
                // remove it so the rename can succeed. We're claiming this
                // dir atomically — stale contents are replaced wholesale.
                if final_dir.exists() {
                    std::fs::remove_dir_all(&final_dir).with_context(|| {
                        format!(
                            "Failed to remove stale rootfs before atomic promote: {}",
                            final_dir.display()
                        )
                    })?;
                }
                std::fs::rename(&tmp_dir, &final_dir).with_context(|| {
                    format!(
                        "Failed to atomically promote rootfs {} -> {}",
                        tmp_dir.display(),
                        final_dir.display()
                    )
                })?;
                tracing::info!(path = %dest.display(), "rootfs ready (atomic promote)");
                eprintln!("  {} Rootfs ready", "\u{2713}".green());
                Ok(())
            }
            Err(e) => {
                // Extraction failed mid-way. Clean up the temp dir so the
                // next pull doesn't see a half-filled sibling. Ignore errors
                // on cleanup — original extraction error is what matters.
                let _ = std::fs::remove_dir_all(&tmp_dir);
                Err(e)
            }
        }
    } else {
        // Degenerate path (no parent): extraction wrote directly to dest.
        extract_result?;
        tracing::info!(path = %dest.display(), "rootfs ready");
        eprintln!("  {} Rootfs ready", "\u{2713}".green());
        Ok(())
    }
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
    fn slug_preserves_dots_and_dashes() {
        let slug = rootfs_slug_for_image("ghcr.io/my-org/my-worker:latest");
        assert!(
            slug.starts_with("ghcr.io-my-org-my-worker-latest-"),
            "unexpected slug: {slug}"
        );
    }

    #[test]
    fn slug_converts_simple_image_ref() {
        // `oven/bun:1` is a Docker Hub shorthand; canonicalizer inserts
        // the default registry, so the slug is keyed on the canonical
        // form. The hash is still computed from the canonical text.
        let slug = rootfs_slug_for_image("oven/bun:1");
        assert!(
            slug.starts_with("docker.io-oven-bun-1-"),
            "unexpected slug: {slug}"
        );
    }

    #[test]
    fn slug_lowercases() {
        // Uppercase paths fail the Docker reference regex, so
        // `canonicalize_oci_ref` falls back to the raw string and the
        // sanitizer lowercases it. Behavior unchanged from pre-canon.
        let slug = rootfs_slug_for_image("GHCR.io/Foo/Bar:1.0");
        assert!(
            slug.starts_with("ghcr.io-foo-bar-1.0-"),
            "unexpected slug: {slug}"
        );
    }

    #[test]
    fn slug_collapses_consecutive_separators() {
        // Weird but defensive: double colons and leading/trailing junk
        // shouldn't produce runs of dashes or empty segments. These
        // inputs fail Reference::try_from and hit the raw-string
        // fallback, so sanitizer behavior is what's under test.
        let a = rootfs_slug_for_image("::foo::");
        assert!(a.starts_with("foo-"), "unexpected slug: {a}");
        let b = rootfs_slug_for_image("a//b:c");
        assert!(b.starts_with("a-b-c-"), "unexpected slug: {b}");
    }

    #[test]
    fn slug_different_images_produce_different_slugs() {
        // Guards the "two workers with different base images share a
        // cache dir" collision — the whole point of slugging the full
        // image ref instead of just the name.
        assert_ne!(
            rootfs_slug_for_image("docker.io/iiidev/node:latest"),
            rootfs_slug_for_image("oven/bun:1")
        );
    }

    /// The sanitizer can still collapse distinct canonical refs into
    /// the same human-readable base (`ghcr.io/foo/bar:1` and
    /// `ghcr.io/foo-bar:1` both become `ghcr.io-foo-bar-1`). The hash
    /// suffix keeps their cache dirs from colliding on disk.
    #[test]
    fn slug_hash_prevents_prefix_collision() {
        let a = rootfs_slug_for_image("ghcr.io/foo/bar:1");
        let b = rootfs_slug_for_image("ghcr.io/foo-bar:1");
        // Both share the human prefix after sanitization.
        assert!(a.starts_with("ghcr.io-foo-bar-1-"), "unexpected slug: {a}");
        assert!(b.starts_with("ghcr.io-foo-bar-1-"), "unexpected slug: {b}");
        // But the full slugs differ because the hash is computed over
        // the canonical ref, which still distinguishes them.
        assert_ne!(a, b);
    }

    #[test]
    fn slug_falls_back_to_image_prefix_when_sanitization_empties() {
        // Pure-separator input produces an empty human base; we still
        // want a stable deterministic directory, so fall back to
        // "image-<hash>" instead of the empty string that would make
        // `prepare_rootfs` bail.
        let slug = rootfs_slug_for_image(":::");
        assert!(slug.starts_with("image-"), "unexpected slug: {slug}");
    }

    #[test]
    fn slug_is_deterministic() {
        // Hash must be stable across calls on the same input so cache
        // dirs survive restarts. FNV-1a is deterministic by definition
        // but this locks in the guarantee for the combined slug too.
        assert_eq!(
            rootfs_slug_for_image("oven/bun:1"),
            rootfs_slug_for_image("oven/bun:1")
        );
    }

    /// Regression for the ~/.iii/cache/ duplication bug: sandbox
    /// presets pass `iiidev/node:latest` (no registry prefix) while
    /// managed workers pass `docker.io/iiidev/node:latest`. Without
    /// canonicalization, both wrote to distinct slug dirs and pulled
    /// the same image twice.
    #[test]
    fn slug_normalizes_docker_hub_default_registry() {
        assert_eq!(
            rootfs_slug_for_image("iiidev/node:latest"),
            rootfs_slug_for_image("docker.io/iiidev/node:latest"),
        );
    }

    /// Docker Hub shorthand `node` expands to
    /// `docker.io/library/node:latest` — the `library/` namespace is
    /// the canonical location for official images.
    #[test]
    fn slug_normalizes_library_shortform() {
        assert_eq!(
            rootfs_slug_for_image("node"),
            rootfs_slug_for_image("docker.io/library/node:latest"),
        );
    }

    /// Omitting the tag is equivalent to `:latest`. Both forms must
    /// land in the same cache dir or the first pull won't satisfy the
    /// second call.
    #[test]
    fn slug_normalizes_missing_tag_to_latest() {
        assert_eq!(
            rootfs_slug_for_image("iiidev/node"),
            rootfs_slug_for_image("iiidev/node:latest"),
        );
    }

    /// Guard the pre-canonicalization escape hatch used by
    /// `rootfs_cache::legacy_candidates`. When the raw form differs
    /// from the canonical form, the two slugs must differ so upgrading
    /// users' existing rootfses remain findable through the legacy
    /// path.
    #[test]
    fn raw_slug_differs_from_canonical_when_shorthand() {
        let raw = raw_rootfs_slug_for_image("iiidev/node:latest");
        let canonical = rootfs_slug_for_image("iiidev/node:latest");
        assert_ne!(raw, canonical);
        assert!(raw.starts_with("iiidev-node-latest-"));
        assert!(canonical.starts_with("docker.io-iiidev-node-latest-"));
    }

    #[test]
    fn test_oci_image_for_kind_defaults_to_node() {
        let (image, name) = oci_image_for_kind("unknown_kind");
        assert_eq!(image, "docker.io/iiidev/node:latest");
        assert_eq!(name, "node");
    }

    #[test]
    fn test_oci_image_for_typescript() {
        let (image, name) = oci_image_for_kind("typescript");
        assert_eq!(image, "docker.io/iiidev/node:latest");
        assert_eq!(name, "node");
    }

    #[test]
    fn test_oci_image_for_bun() {
        let (image, name) = oci_image_for_kind("bun");
        assert_eq!(image, "docker.io/oven/bun:latest");
        assert_eq!(name, "bun");
    }

    #[test]
    fn test_oci_image_for_python() {
        let (image, name) = oci_image_for_kind("python");
        assert_eq!(image, "docker.io/iiidev/python:latest");
        assert_eq!(name, "python");
    }

    #[test]
    fn test_oci_image_for_rust() {
        let (image, name) = oci_image_for_kind("rust");
        assert_eq!(image, "docker.io/library/rust:slim-bookworm");
        assert_eq!(name, "rust");
    }

    #[test]
    fn test_oci_image_for_go() {
        let (image, name) = oci_image_for_kind("go");
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
