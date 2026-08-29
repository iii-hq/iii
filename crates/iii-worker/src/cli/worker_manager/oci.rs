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

    // Pre-fetch the manifest so we can tell the user UP FRONT what they're
    // about to wait for (layer count + total compressed bytes). The
    // `client.pull(...)` call below is one blocking network operation
    // with no per-byte progress hook, so without this summary the only
    // signal the wait UI has is the 3s heartbeat (elapsed time only).
    // Knowing "1247 MiB across 5 layers" lets the user gauge how long
    // the pull will realistically take. Best-effort: if the registry
    // returns an Image Index (multi-platform manifest) we skip the
    // summary rather than do the full platform resolution dance — the
    // pull below handles that path natively.
    let manifest_summary: String = match client
        .pull_manifest(&reference, &RegistryAuth::Anonymous)
        .await
    {
        Ok((oci_client::manifest::OciManifest::Image(m), _)) => {
            let bytes: i64 = m.layers.iter().map(|l| l.size).sum();
            format!(
                " ({} layer{}, {:.1} MiB)",
                m.layers.len(),
                if m.layers.len() == 1 { "" } else { "s" },
                bytes as f64 / (1024.0 * 1024.0),
            )
        }
        _ => String::new(),
    };

    eprintln!("  Pulling {}{}...", image, manifest_summary);
    let _ = manifest_summary; // consumed inside pull_with_progress now

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

        // Streaming pull: fetch manifest, then pull each layer via
        // `pull_blob_stream`, counting bytes into an Arc<AtomicU64>.
        // A background task reads the counter every 2s and emits a
        // real progress line:
        //   "Pulling <image>: 412.6/1247.3 MiB (33%, 41.2 MiB/s, 10s)"
        // The wait UI's `tail:` row picks up the latest one and shows
        // genuine download progress instead of just elapsed time.
        let pull_result = pull_with_progress(&client, &reference, image, host_arch).await;

        match pull_result {
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
    use std::io::IsTerminal;
    let mirror_layer_lines = !std::io::stderr().is_terminal();
    let extract_result: Result<()> = (|| {
        for (i, layer) in image_data.layers.iter().enumerate() {
            extract_layer_with_limits(&layer.data, &extract_root, i, layer_count, &mut total_size)?;
            pb.inc(1);
            // On TTY, indicatif's progress bar handles the live row.
            // On non-TTY (engine-captured log file), the bar produces
            // nothing — so we mirror each completed layer as a plain
            // eprintln to keep the `tail:` row in `iii worker add
            // --wait` refreshing with concrete progress. The TTY
            // branch SKIPS this print so it doesn't scroll the bar's
            // row off as the bar is also being redrawn.
            if mirror_layer_lines {
                eprintln!(
                    "  Extracted layer {}/{} ({:.1} MiB total)",
                    i + 1,
                    layer_count,
                    total_size as f64 / (1024.0 * 1024.0),
                );
            }
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

// ─────────────────────────────────────────────────────────────────────────────
// Streaming pull with byte-level progress
//
// `oci_client::Client::pull` is one blocking call with no per-byte hook,
// which forced our wait UI to fall back on an elapsed-time-only heart-
// beat. Re-implement the pull at the layer level: fetch the manifest
// (resolving multi-platform), pull the config blob, then stream each
// layer via `pull_blob_stream`, counting bytes into a shared
// `AtomicU64`. A background task reads the counter every 2s and emits
// a real progress line that the wait UI's `tail:` row surfaces.
// ─────────────────────────────────────────────────────────────────────────────

async fn pull_with_progress(
    client: &oci_client::Client,
    reference: &oci_client::Reference,
    image: &str,
    host_arch: &str,
) -> Result<oci_client::client::ImageData> {
    use futures::StreamExt;
    use oci_client::client::{Config, ImageData, ImageLayer};
    use sha2::Digest;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    // 1. Manifest (resolving multi-platform if the registry returned an
    //    ImageIndex). Tracks the resolved manifest digest so we can
    //    return it to the caller alongside the data.
    let (image_manifest, manifest_digest) =
        pull_image_manifest_resolved(client, reference, host_arch).await?;

    let total_bytes: u64 = image_manifest
        .layers
        .iter()
        .map(|l| l.size.max(0) as u64)
        .sum();
    let downloaded = Arc::new(AtomicU64::new(0));

    // 2. Heartbeat task using the byte counter. 2-second cadence
    //    rather than 3 so the percentage feels live without burning
    //    the file with chatter.
    let downloaded_for_heartbeat = Arc::clone(&downloaded);
    let total_for_heartbeat = total_bytes;
    let image_for_heartbeat = image.to_string();
    let attempt_started = std::time::Instant::now();
    // Skip the per-second heartbeat eprintlns when stderr is a TTY: the
    // caller (`handle_oci_pull_and_add`) wraps this whole call in a
    // `Spinner` whose 100ms tick redraws the same row. Heartbeat
    // eprintlns from here would scroll the spinner row off and produce
    // visual chaos. The non-TTY path (engine-spawned subprocess →
    // `~/.iii/logs/<name>/stderr.log`) NEEDS these lines because the
    // wait UI tails that file for the `tail:` row, and there's no
    // spinner there.
    use std::io::IsTerminal;
    let emit_heartbeat = !std::io::stderr().is_terminal();
    // Scope guard: aborts the heartbeat task on Drop. Without this, every
    // `?` operator between here and the explicit `drop(heartbeat)` at the
    // end of this function (config-blob fetch, every layer's
    // `pull_blob_stream`, every chunk read) would early-return and leak
    // the task — it would keep `eprintln!`-ing progress lines for the
    // rest of the process lifetime, racing against the retry loop's
    // next attempt's fresh heartbeat. With this guard the abort fires
    // unconditionally on any return path.
    struct AbortOnDrop(tokio::task::JoinHandle<()>);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let heartbeat = AbortOnDrop(tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        // Default `Burst` would replay missed ticks back-to-back if
        // the runtime worker thread was held off (e.g., a blocking
        // syscall elsewhere). The wait UI would suddenly see N
        // identical "Pulling X.X/Y.Y MiB" lines in milliseconds, which
        // looks like a glitch rather than honest progress.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await; // first tick is instant; consume it
        loop {
            interval.tick().await;
            let dl = downloaded_for_heartbeat.load(Ordering::Relaxed);
            let elapsed = attempt_started.elapsed().as_secs().max(1);
            let mb = dl as f64 / (1024.0 * 1024.0);
            if total_for_heartbeat > 0 {
                let total_mb = total_for_heartbeat as f64 / (1024.0 * 1024.0);
                let pct = (dl as u128 * 100 / total_for_heartbeat.max(1) as u128).min(100) as u64;
                let rate = mb / elapsed as f64;
                if emit_heartbeat {
                    eprintln!(
                        "  Pulling {}: {:.1}/{:.1} MiB ({}%, {:.1} MiB/s, {}s)",
                        image_for_heartbeat, mb, total_mb, pct, rate, elapsed
                    );
                }
            } else if emit_heartbeat {
                // Registry didn't report layer sizes — show what we've
                // got so the user at least sees activity.
                eprintln!(
                    "  Pulling {}: {:.1} MiB ({}s)",
                    image_for_heartbeat, mb, elapsed
                );
            }
        }
    }));

    // 3. Pull the config blob. Small (typically <10 KiB), doesn't
    //    contribute to the layer-progress denominator. Verify the
    //    descriptor's SHA256 digest while streaming so a corrupted CDN
    //    response or MITM doesn't slip through — the upstream
    //    `client.pull(...)` we replaced did this internally; the
    //    streaming path needs to do it explicitly.
    let mut config_bytes: Vec<u8> = Vec::with_capacity(image_manifest.config.size.max(0) as usize);
    let mut config_hasher = sha2::Sha256::new();
    let mut config_stream = client
        .pull_blob_stream(reference, &image_manifest.config)
        .await
        .context("failed to pull image config blob")?;
    while let Some(chunk) = config_stream.next().await {
        let chunk = chunk.context("config blob stream chunk failed")?;
        config_hasher.update(&chunk);
        config_bytes.extend_from_slice(&chunk);
    }
    verify_sha256_digest(&image_manifest.config.digest, config_hasher, "image config")?;
    let config = Config {
        data: bytes::Bytes::from(config_bytes),
        media_type: image_manifest.config.media_type.clone(),
        annotations: None,
    };

    // 4. Pull each layer with byte-progress counter updates. Sequential
    //    rather than concurrent: registries throttle per-connection, so
    //    parallel pulls rarely speed things up and they make the
    //    progress display incoherent ("layer 1 at 80%, layer 3 at 40%").
    //    Each layer's bytes are hashed as they stream and verified
    //    against the manifest descriptor's digest before the layer is
    //    accepted into `layers` — content integrity match before the
    //    extract step touches the rootfs.
    let mut layers: Vec<ImageLayer> = Vec::with_capacity(image_manifest.layers.len());
    for (idx, layer_desc) in image_manifest.layers.iter().enumerate() {
        let mut layer_bytes: Vec<u8> = Vec::with_capacity(layer_desc.size.max(0) as usize);
        let mut layer_hasher = sha2::Sha256::new();
        let mut stream = client
            .pull_blob_stream(reference, layer_desc)
            .await
            .with_context(|| format!("failed to pull layer {} blob", idx + 1))?;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.with_context(|| format!("layer {} stream chunk failed", idx + 1))?;
            layer_hasher.update(&chunk);
            downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            layer_bytes.extend_from_slice(&chunk);
        }
        verify_sha256_digest(
            &layer_desc.digest,
            layer_hasher,
            &format!("layer {}", idx + 1),
        )?;
        layers.push(ImageLayer {
            data: bytes::Bytes::from(layer_bytes),
            media_type: layer_desc.media_type.clone(),
            annotations: None,
        });
    }

    drop(heartbeat); // explicit: AbortOnDrop fires here

    // Final 100% line so the user sees the pull cleanly conclude
    // before the extract eprintlns start. Gated on non-TTY for the same
    // reason as the heartbeat: on TTY the outer Spinner provides the
    // signal; here this line would just collide with it.
    let final_mb = downloaded.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0);
    let total_mb = total_bytes as f64 / (1024.0 * 1024.0);
    let final_elapsed = attempt_started.elapsed().as_secs().max(1);
    if emit_heartbeat {
        eprintln!(
            "  Pulling {}: {:.1}/{:.1} MiB (100%, {:.1}s total)",
            image, final_mb, total_mb, final_elapsed as f64
        );
    }

    Ok(ImageData {
        layers,
        digest: manifest_digest,
        config,
        manifest: Some(image_manifest),
    })
}

/// Compare the SHA256 digest a streaming hasher produced against the OCI
/// descriptor's expected digest string ("sha256:<hex>"). Returns Err on
/// mismatch, unsupported algorithm, or malformed digest string — caller
/// propagates so the corrupted blob never reaches the extract step or
/// the rootfs.
///
/// This is the only content-integrity check on the streaming pull path.
/// The original `oci_client::Client::pull(...)` did this internally; we
/// have to do it explicitly now that we walk blobs ourselves for
/// byte-level progress reporting.
fn verify_sha256_digest(expected: &str, hasher: sha2::Sha256, label: &str) -> Result<()> {
    use sha2::Digest;
    // Match the algorithm prefix case-insensitively. The OCI spec
    // mandates lowercase `sha256:` but defensively accepting `Sha256:`
    // / `SHA256:` keeps a registry-side typo from surfacing as the
    // wrong error ("unsupported algorithm" when the algorithm is fine
    // and only the case is wrong).
    let Some(expected_hex) = expected
        .split_once(':')
        .filter(|(alg, _)| alg.eq_ignore_ascii_case("sha256"))
        .map(|(_, hex)| hex)
    else {
        // OCI distribution spec allows other algorithms, but in practice
        // every registry we target uses SHA256. Fail loudly on anything
        // else so a future spec drift doesn't silently degrade to
        // "verified" when nothing was checked.
        anyhow::bail!(
            "{} digest uses unsupported algorithm: {} (expected sha256:...)",
            label,
            expected
        );
    };
    let computed = hasher.finalize();
    let computed_hex = computed
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    if !computed_hex.eq_ignore_ascii_case(expected_hex) {
        anyhow::bail!(
            "{} digest mismatch: expected sha256:{}, got sha256:{}",
            label,
            expected_hex,
            computed_hex
        );
    }
    Ok(())
}

/// Fetch the image manifest, resolving multi-platform `ImageIndex` to
/// the appropriate `linux/<host_arch>` `Image` manifest. Returns the
/// resolved Image manifest and its digest.
async fn pull_image_manifest_resolved(
    client: &oci_client::Client,
    reference: &oci_client::Reference,
    host_arch: &str,
) -> Result<(oci_client::manifest::OciImageManifest, Option<String>)> {
    use oci_client::secrets::RegistryAuth;

    let target_arch = match host_arch {
        "arm64" => oci_spec::image::Arch::ARM64,
        _ => oci_spec::image::Arch::Amd64,
    };

    let (manifest, digest) = client
        .pull_manifest(reference, &RegistryAuth::Anonymous)
        .await
        .context("failed to pull image manifest")?;

    match manifest {
        oci_client::manifest::OciManifest::Image(m) => Ok((m, Some(digest))),
        oci_client::manifest::OciManifest::ImageIndex(idx) => {
            let chosen = idx.manifests.iter().find(|m| {
                m.platform.as_ref().is_some_and(|p| {
                    p.os == oci_spec::image::Os::Linux && p.architecture == target_arch
                })
            });
            let chosen_digest = chosen
                .ok_or_else(|| {
                    let available: Vec<String> = idx
                        .manifests
                        .iter()
                        .filter_map(|m| {
                            m.platform
                                .as_ref()
                                .map(|p| format!("{}/{}", p.os, p.architecture))
                        })
                        .collect();
                    anyhow::anyhow!(
                        "no linux/{} manifest in image index. Available platforms: {}",
                        host_arch,
                        available.join(", ")
                    )
                })?
                .digest
                .clone();
            let resolved_ref = reference.clone_with_digest(chosen_digest.clone());
            let (m, d) = client
                .pull_manifest(&resolved_ref, &RegistryAuth::Anonymous)
                .await
                .context("failed to pull resolved platform manifest")?;
            match m {
                oci_client::manifest::OciManifest::Image(im) => Ok((im, Some(d))),
                oci_client::manifest::OciManifest::ImageIndex(_) => {
                    Err(anyhow::anyhow!("nested ImageIndex not supported"))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: the streaming pull's `verify_sha256_digest` must
    /// accept a hasher whose finalize matches the descriptor's hex
    /// suffix (case-insensitively).
    #[test]
    fn verify_sha256_digest_accepts_matching_hash() {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(b"hello world");
        // sha256("hello world") = b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
        let expected = "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        verify_sha256_digest(expected, h, "test blob").expect("digest must match");
    }

    /// Regression: a one-byte difference in streamed content must fail
    /// loudly. Without this check, a MITM-corrupted CDN response would
    /// silently land in the rootfs.
    #[test]
    fn verify_sha256_digest_rejects_mismatched_hash() {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(b"goodbye world");
        let expected = "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        let err = verify_sha256_digest(expected, h, "tampered blob")
            .expect_err("mismatched digest must fail");
        assert!(err.to_string().contains("tampered blob"));
        assert!(err.to_string().contains("digest mismatch"));
    }

    /// Future spec drift safeguard: an unsupported digest algorithm
    /// (e.g. "sha512:...") must error instead of silently passing.
    #[test]
    fn verify_sha256_digest_rejects_non_sha256_algorithm() {
        use sha2::Digest;
        let h = sha2::Sha256::new();
        let err = verify_sha256_digest("sha512:deadbeef", h, "future blob")
            .expect_err("non-sha256 algorithm must fail");
        assert!(err.to_string().contains("unsupported algorithm"));
    }

    /// `verify_sha256_digest` accepts uppercase hex in the descriptor
    /// (registries vary). Same content, same digest, just letter case.
    #[test]
    fn verify_sha256_digest_is_case_insensitive() {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(b"hello world");
        let expected = "sha256:B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
        verify_sha256_digest(expected, h, "uppercase").expect("uppercase hex must match");
    }

    /// Regression: an out-of-spec `Sha256:` / `SHA256:` prefix from a
    /// buggy registry must still be recognized as the supported
    /// algorithm — without this, the operator chases an "unsupported
    /// algorithm" error when the algorithm is fine and only the case
    /// is off.
    #[test]
    fn verify_sha256_digest_accepts_uppercase_algorithm_prefix() {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(b"hello world");
        let expected = "Sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        verify_sha256_digest(expected, h, "Sha256-prefix").expect("uppercase prefix must match");

        let mut h2 = sha2::Sha256::new();
        h2.update(b"hello world");
        let expected2 = "SHA256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        verify_sha256_digest(expected2, h2, "SHA256-prefix").expect("ALL-CAPS prefix must match");
    }

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
