// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Unified rootfs cache.
//!
//! Historically three different rootfs caches coexisted, each with its
//! own layout key and write path, so a single OCI image could be pulled
//! up to three times (once per caller) and none of the caches shared:
//!
//! | caller | wrote to | keyed by |
//! |----|----|----|
//! | managed worker `prepare_rootfs` | `~/.iii/rootfs/<kind>/` | runtime kind |
//! | sandbox `auto_install_image` | `~/.iii/managed/<image>/rootfs/` | preset name |
//! | `LibkrunAdapter::pull` | `~/.iii/images/<sha256[..16]>/` | sha256(oci_ref) |
//!
//! This module collapses them behind a single content-addressable cache
//! at `~/.iii/cache/<slug>/` keyed on the OCI reference itself. All
//! three flows go through `ensure_rootfs`, so pulling `docker.io/foo:1`
//! as a sandbox preset later satisfies a managed worker's request for
//! the same image without re-pulling.
//!
//! `resolve_cached` also consults the three legacy paths (opt-in per
//! `CacheHints`) so upgrading users don't have to re-download every
//! image the first time they run the unified code.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli::oci_ref::canonical_cache_key;
use crate::cli::worker_manager::oci::{
    pull_and_extract_rootfs, raw_rootfs_slug_for_image, rootfs_slug_for_image,
};

/// Canonical cache root. All new pulls land under here, keyed by
/// `rootfs_slug_for_image(oci_ref)`.
fn cache_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".iii")
        .join("cache")
}

/// Canonical on-disk location for `oci_ref`'s rootfs. Returned path is
/// stable across calls and callers — two different flows referencing
/// the same image resolve to the same directory. Digest-pinned refs
/// (`foo:tag@sha256:...`) collapse to the same dir as the unpinned
/// form via `canonical_cache_key`, so engine-resolved pins never
/// trigger a re-pull of an image the user already added.
pub fn canonical_path(oci_ref: &str) -> PathBuf {
    cache_root().join(rootfs_slug_for_image(canonical_cache_key(oci_ref)))
}

/// True if `path` looks like a populated OCI rootfs. The `bin/` check
/// catches stub directories left behind by a failed pull — every sane
/// Linux image ships `/bin/sh`, so a rootfs with no `bin/` is either
/// mid-extract or arch-mismatched.
pub fn is_populated(path: &Path) -> bool {
    path.exists() && path.join("bin").exists()
}

/// Legacy cache locations the caller wants consulted on a canonical
/// miss. Supplying the relevant hint for your flow lets users' existing
/// rootfses stay hot across the refactor instead of forcing a re-pull.
#[derive(Default, Debug, Clone)]
pub struct CacheHints<'a> {
    /// `~/.iii/rootfs/<kind>/` — managed-worker `prepare_rootfs` history.
    pub legacy_kind: Option<&'a str>,
    /// `~/.iii/managed/<preset>/rootfs/` — sandbox `auto_install_image` history.
    pub legacy_preset: Option<&'a str>,
    /// `~/.iii/images/<sha256(oci_ref)[..16]>/` — old LibkrunAdapter cache.
    /// Cheap to always set true on the libkrun path, no-op otherwise.
    pub consult_images_cache: bool,
}

fn legacy_candidates<'a>(oci_ref: &str, hints: &CacheHints<'a>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return out;
    };
    // Pre-canonicalization slug. Before OCI refs were normalized through
    // `oci_client::Reference`, `rootfs_slug_for_image` hashed the raw
    // string — so `iiidev/foo:latest` (sandbox preset) and
    // `docker.io/iiidev/foo:latest` (managed worker) wrote to distinct
    // dirs under `~/.iii/cache/`. Check the raw-form slug too so users
    // who already have a populated rootfs from the pre-fix code stay hot.
    let raw_slug = raw_rootfs_slug_for_image(oci_ref);
    let canonical_slug = rootfs_slug_for_image(oci_ref);
    if raw_slug != canonical_slug {
        out.push(home.join(".iii").join("cache").join(&raw_slug));
    }
    if let Some(kind) = hints.legacy_kind {
        out.push(home.join(".iii").join("rootfs").join(kind));
    }
    if let Some(preset) = hints.legacy_preset {
        out.push(
            home.join(".iii")
                .join("managed")
                .join(preset)
                .join("rootfs"),
        );
    }
    if hints.consult_images_cache {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(oci_ref.as_bytes());
        let hash = hex::encode(&hasher.finalize()[..8]);
        out.push(home.join(".iii").join("images").join(hash));
    }
    out
}

/// Return the first populated cache path for `oci_ref`, or `None` on
/// total miss. Canonical is checked before any legacy path.
pub fn resolve_cached(oci_ref: &str, hints: &CacheHints<'_>) -> Option<PathBuf> {
    let canonical = canonical_path(oci_ref);
    if is_populated(&canonical) {
        return Some(canonical);
    }
    for legacy in legacy_candidates(oci_ref, hints) {
        if is_populated(&legacy) {
            return Some(legacy);
        }
    }
    None
}

/// Ensure a populated rootfs exists for `oci_ref`; return its on-disk
/// path. `on_pull_start` fires exactly once, only on a cache miss, right
/// before the network pull begins — use it to log "Pulling..." or
/// surface a progress event to the user.
pub async fn ensure_rootfs(
    oci_ref: &str,
    hints: &CacheHints<'_>,
    on_pull_start: impl FnOnce(),
) -> Result<PathBuf> {
    if let Some(cached) = resolve_cached(oci_ref, hints) {
        return Ok(cached);
    }
    on_pull_start();
    let dest = canonical_path(oci_ref);
    // Clean any partial-state stub from a prior failed attempt: a
    // dir-exists-but-empty rootfs can confound tar's overwrite logic
    // and fool `is_populated` on the next call.
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&dest);
    }
    pull_and_extract_rootfs(oci_ref, &dest).await?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn populate(dir: &Path) {
        std::fs::create_dir_all(dir.join("bin")).unwrap();
    }

    #[test]
    fn canonical_path_is_slug_based_and_stable() {
        let a = canonical_path("docker.io/library/python:3.13");
        let b = canonical_path("docker.io/library/python:3.13");
        assert_eq!(a, b);
        assert_ne!(a, canonical_path("docker.io/library/python:3.12"));
    }

    #[test]
    fn is_populated_requires_bin_subdir() {
        let td = tempdir().unwrap();
        assert!(!is_populated(td.path()));
        populate(td.path());
        assert!(is_populated(td.path()));
    }

    #[test]
    fn resolve_cached_returns_none_on_total_miss() {
        // Use a slug nothing on disk will have. Canonical_path respects
        // $HOME, so we don't need to redirect that — just pick an image
        // reference that will hash to a unique slug per test run.
        let uniq = format!(
            "nonexistent-{}:{}",
            std::process::id(),
            std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
        );
        let hints = CacheHints::default();
        assert!(resolve_cached(&uniq, &hints).is_none());
    }
}
