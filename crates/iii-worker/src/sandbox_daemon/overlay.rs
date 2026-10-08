//! Per-sandbox host directory layout.
//!
//! Isolation model: a sandbox never boots on the shared image
//! cache. `__vm-boot` gets this sandbox's `root/` trampoline, served
//! read-only (`--rootfs-readonly`) so no guest write can reach the host, plus
//! the cached image as a read-only erofs lower. iii-init assembles `/` as
//! overlayfs with an in-guest tmpfs upper, so every write is private to the
//! VM and vanishes with it.
//!
//! Directory layout per sandbox (ID = UUID):
//!   /tmp/iii-sandbox/<uuid>/root/      (read-only virtiofs trampoline: `/dev/root`)
//!   /tmp/iii-sandbox/<uuid>/upper/     (reserved; the live upper is an in-guest tmpfs)
//!   /tmp/iii-sandbox/<uuid>/work/      (reserved)
//!   /tmp/iii-sandbox/<uuid>/merged/    (host-side workdir placeholder)

use std::path::PathBuf;
use uuid::Uuid;

/// Directories iii-init's `overlay_root` creates on the virtiofs root before
/// it pivots (`/dev`, `/overlay-lower`, `/overlay-upperfs`, `/new-root`; see
/// `crates/iii-init/src/root_pivot.rs`). The sandbox trampoline is served
/// read-only, so they are pre-created host-side: the guest `mkdir` then gets
/// `EEXIST`, which iii-init tolerates, instead of `EROFS`.
pub const TRAMPOLINE_STAGING_DIRS: &[&str] =
    &["dev", "overlay-lower", "overlay-upperfs", "new-root"];

pub struct OverlayLayout {
    pub upper: PathBuf,
    pub work: PathBuf,
    pub merged: PathBuf,
    pub root: PathBuf,
}

impl OverlayLayout {
    pub fn for_sandbox(id: Uuid) -> Self {
        let base = PathBuf::from("/tmp/iii-sandbox").join(id.to_string());
        Self {
            upper: base.join("upper"),
            work: base.join("work"),
            merged: base.join("merged"),
            root: base.join("root"),
        }
    }

    pub fn base(&self) -> PathBuf {
        self.upper.parent().expect("upper has parent").to_path_buf()
    }

    /// Create the directory structure, including the trampoline's
    /// [`TRAMPOLINE_STAGING_DIRS`]; does NOT mount anything. This helper
    /// exists so the layout is deterministic and easily reaped.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.upper)?;
        std::fs::create_dir_all(&self.work)?;
        std::fs::create_dir_all(&self.merged)?;
        for dir in TRAMPOLINE_STAGING_DIRS {
            std::fs::create_dir_all(self.root.join(dir))?;
        }
        Ok(())
    }

    pub fn cleanup(&self) -> std::io::Result<()> {
        let _ = std::fs::remove_dir_all(self.base());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_paths_are_deterministic() {
        let id = Uuid::nil();
        let l = OverlayLayout::for_sandbox(id);
        assert!(l.upper.ends_with("upper"));
        assert!(l.work.ends_with("work"));
        assert!(l.merged.ends_with("merged"));
    }

    #[test]
    fn ensure_dirs_and_cleanup_roundtrip() {
        let id = Uuid::new_v4();
        let l = OverlayLayout::for_sandbox(id);
        l.ensure_dirs().unwrap();
        assert!(l.upper.exists());
        l.cleanup().unwrap();
        assert!(!l.base().exists());
    }

    #[test]
    fn ensure_dirs_prepares_read_only_trampoline() {
        let l = OverlayLayout::for_sandbox(Uuid::new_v4());
        l.ensure_dirs().unwrap();
        assert!(l.root.starts_with(l.base()));
        for dir in TRAMPOLINE_STAGING_DIRS {
            assert!(l.root.join(dir).is_dir(), "missing trampoline dir {dir}");
        }
        // Nothing else: the trampoline must not carry image content.
        assert_eq!(
            std::fs::read_dir(&l.root).unwrap().count(),
            TRAMPOLINE_STAGING_DIRS.len()
        );
        l.cleanup().unwrap();
    }
}
