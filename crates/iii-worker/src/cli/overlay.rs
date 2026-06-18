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
/// Shared read-only base + per-worker overlay upper layout. (Legacy workers
/// carry no marker — `read_layout` returns `None` — so there is no constant
/// for it.)
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
        Some(v) => mode_value_enables_overlay(&v),
        None => true,
    }
}

/// Pure mapping of a mode string to "overlay on?". Anything other than the
/// recognized disable words (case-insensitive) enables overlay.
fn mode_value_enables_overlay(v: &str) -> bool {
    !matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "legacy" | "off" | "0" | "false" | "no"
    )
}

/// Decide whether to boot in overlay mode: flag on AND an overlay-capable
/// (embedded) iii-init is available — see the capability-handshake rationale in
/// the module docs. Emits a one-line reason when the flag is on but we must
/// fall back to legacy, so a non-embedded build (or a deliberate
/// `III_ROOTFS_MODE=legacy`) is visible in the worker log rather than silent.
pub fn overlay_active() -> bool {
    if !overlay_enabled() {
        return false;
    }
    if !iii_filesystem::init::has_init() {
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
        match std::fs::remove_dir_all(&deps) {
            Ok(()) => eprintln!("iii: reclaimed orphaned legacy dep cache (overlay migration)"),
            Err(e) => tracing::debug!("gc legacy deps {}: {e}", deps.display()),
        }
    }
    let _ = std::fs::remove_file(managed_dir.join("var/.iii-prepared"));
    write_layout(managed_dir, LAYOUT_OVERLAY);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("iii-overlay-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn layout_marker_roundtrips_and_is_none_when_absent() {
        let d = tmp("marker");
        assert_eq!(read_layout(&d), None);
        write_layout(&d, LAYOUT_OVERLAY);
        assert_eq!(read_layout(&d).as_deref(), Some(LAYOUT_OVERLAY));
        write_layout(&d, "legacy");
        assert_eq!(read_layout(&d).as_deref(), Some("legacy"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn migrate_gcs_legacy_artifacts_and_marks_overlay() {
        let d = tmp("migrate");
        // Simulate a legacy clone's orphaned artifacts.
        std::fs::create_dir_all(d.join("var/iii/deps/node_modules")).unwrap();
        std::fs::write(d.join("var/iii/deps/node_modules/x"), b"payload").unwrap();
        std::fs::create_dir_all(d.join("var")).unwrap();
        std::fs::write(d.join("var/.iii-prepared"), b"").unwrap();

        migrate_to_overlay(&d);

        assert!(!d.join("var/iii/deps").exists(), "legacy dep cache not GC'd");
        assert!(
            !d.join("var/.iii-prepared").exists(),
            "stale prepared marker not removed"
        );
        assert_eq!(read_layout(&d).as_deref(), Some(LAYOUT_OVERLAY));

        // Idempotent: a second call is a no-op (already overlay) and must not
        // error or change the marker.
        migrate_to_overlay(&d);
        assert_eq!(read_layout(&d).as_deref(), Some(LAYOUT_OVERLAY));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn mode_value_mapping_disables_on_legacy_words_only() {
        for v in ["legacy", "off", "0", "false", "no", "LEGACY", " Off "] {
            assert!(!mode_value_enables_overlay(v), "{v} should disable overlay");
        }
        for v in ["overlay", "on", "1", "true", "yes", "anything-else"] {
            assert!(mode_value_enables_overlay(v), "{v} should enable overlay");
        }
    }
}
