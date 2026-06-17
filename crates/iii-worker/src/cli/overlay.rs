//! Shared read-only rootfs overlay: feature flag, init-capability handshake,
//! per-worker layout marker, and GC of orphaned legacy-layout artifacts.
//!
//! The overlay model (shared read-only squashfs base + per-worker writable
//! upper, assembled by iii-init) is ON BY DEFAULT. It is gated by two checks:
//!
//! 1. Feature flag — `III_ROOTFS_MODE`. Anything other than `legacy`/`off`/
//!    `0`/`false` (including unset) means overlay. Operators opt back into the
//!    legacy per-worker-clone model with `III_ROOTFS_MODE=legacy`.
//!
//! 2. Capability handshake — overlay requires an EMBEDDED iii-init. An
//!    embedded init is compiled into *this* iii-worker binary, so it is
//!    guaranteed version-matched and overlay-capable. A downloaded/cached
//!    init (resolved from `~/.iii/lib/iii-init`) can be STALE — older than
//!    this binary, predating overlay support — and would ignore the
//!    `III_BLOCK_ROOT_*` env and boot the legacy pivot against a layout that
//!    no longer exists, producing a broken VM. Embedded init can never be
//!    stale, so its presence is the safe capability signal. This is what
//!    makes an `iii` version update safe even with independently-cached
//!    init binaries.

use std::path::{Path, PathBuf};

const LAYOUT_MARKER: &str = ".iii-layout";
/// Legacy per-worker full-clone layout.
pub const LAYOUT_LEGACY: &str = "legacy";
/// Shared read-only base + per-worker overlay upper layout.
pub const LAYOUT_OVERLAY: &str = "overlay";

/// Overlay is on by default; `legacy|off|0|false|no` disables it.
///
/// Precedence: `III_ROOTFS_MODE` env var > config.yaml `rootfs.mode` >
/// default (overlay). The config.yaml fallback exists because the engine's
/// worker-spawn chain doesn't forward arbitrary env to the start process, so
/// an on-disk setting is how an operator preference reliably reaches the boot
/// path. Unset everywhere = overlay.
pub fn overlay_enabled() -> bool {
    let val = std::env::var("III_ROOTFS_MODE")
        .ok()
        .or_else(crate::cli::config_file::rootfs_mode);
    match val {
        Some(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "legacy" | "off" | "0" | "false" | "no"
        ),
        None => true,
    }
}

/// True only when an overlay-capable (embedded) iii-init is available — see
/// the capability-handshake rationale in the module docs.
pub fn init_overlay_capable() -> bool {
    iii_filesystem::init::has_init()
}

/// Decide whether to boot in overlay mode: flag on AND init capable. Emits a
/// one-line reason when the flag is on but we must fall back to legacy, so a
/// non-embedded build (or a deliberate `III_ROOTFS_MODE=legacy`) is visible in
/// the worker log rather than silent.
pub fn overlay_active() -> bool {
    if !overlay_enabled() {
        return false;
    }
    if !init_overlay_capable() {
        // Common+expected in non-embed (dev) builds, so keep it quiet — a
        // per-start stderr warning would spam every worker boot. Operators
        // who expect overlay can see this at debug level.
        tracing::debug!(
            "overlay rootfs requested but iii-init is not embedded; using legacy per-worker rootfs"
        );
        return false;
    }
    true
}

fn marker_path(managed_dir: &Path) -> PathBuf {
    managed_dir.join(LAYOUT_MARKER)
}

/// The worker's recorded layout, or `None` when unmarked (a pre-overlay
/// install, or a fresh worker).
pub fn read_layout(managed_dir: &Path) -> Option<String> {
    std::fs::read_to_string(marker_path(managed_dir))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Stamp the worker's layout marker.
pub fn write_layout(managed_dir: &Path, layout: &str) {
    let _ = std::fs::create_dir_all(managed_dir);
    if let Err(e) = std::fs::write(marker_path(managed_dir), layout) {
        tracing::debug!("write layout marker for {}: {e}", managed_dir.display());
    }
}

/// Migrate a worker's on-disk layout to overlay, reclaiming orphaned
/// legacy-layout artifacts. The legacy per-worker-clone model kept the
/// dependency cache at `var/iii/deps` and a `var/.iii-prepared` marker inside
/// the clone; under overlay those live in the per-worker writable upper and
/// iii-init's own state, so the legacy copies are dead weight. Removing them
/// reclaims the bulk of a stale clone (the dep caches) while leaving the dir
/// bootable as the overlay trampoline.
///
/// Idempotent: a no-op once the worker is marked `overlay`.
pub fn migrate_to_overlay(managed_dir: &Path) {
    if read_layout(managed_dir).as_deref() == Some(LAYOUT_OVERLAY) {
        return;
    }
    let deps = managed_dir.join("var/iii/deps");
    if deps.is_dir() {
        let freed = dir_size(&deps);
        match std::fs::remove_dir_all(&deps) {
            Ok(()) if freed > 0 => eprintln!(
                "iii: reclaimed {:.1} MB of orphaned legacy dep cache (overlay migration)",
                freed as f64 / 1_048_576.0
            ),
            Ok(()) => {}
            Err(e) => tracing::debug!("gc legacy deps {}: {e}", deps.display()),
        }
    }
    let _ = std::fs::remove_file(managed_dir.join("var/.iii-prepared"));
    write_layout(managed_dir, LAYOUT_OVERLAY);
}

/// Total bytes under a directory (best-effort; symlinks not followed).
fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                total += dir_size(&entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}
