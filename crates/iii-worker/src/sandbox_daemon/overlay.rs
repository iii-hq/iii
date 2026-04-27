//! Per-sandbox overlayfs layout. A sandbox mounts tmpfs upper over the
//! shared read-only rootfs lower, giving each VM its own ephemeral FS view.
//!
//! Directory layout per sandbox (ID = UUID):
//!   /tmp/iii-sandbox/<uuid>/upper/     (writable, tmpfs-backed in practice)
//!   /tmp/iii-sandbox/<uuid>/work/      (overlayfs work dir)
//!   /tmp/iii-sandbox/<uuid>/merged/    (the unified view libkrun mounts)

use std::path::PathBuf;
use uuid::Uuid;

pub struct OverlayLayout {
    pub upper: PathBuf,
    pub work: PathBuf,
    pub merged: PathBuf,
}

impl OverlayLayout {
    pub fn for_sandbox(id: Uuid) -> Self {
        let base = PathBuf::from("/tmp/iii-sandbox").join(id.to_string());
        Self {
            upper: base.join("upper"),
            work: base.join("work"),
            merged: base.join("merged"),
        }
    }

    pub fn base(&self) -> PathBuf {
        self.upper.parent().expect("upper has parent").to_path_buf()
    }

    /// Create the directory structure; does NOT mount. On macOS we skip the
    /// real overlay mount (libkrun handles the VM's root FS differently),
    /// and on Linux the actual mount() happens elsewhere if we need it —
    /// for v1 we hand libkrun the lower rootfs directly plus a tmpfs
    /// workspace. This helper exists so the layout is deterministic and
    /// easily reaped.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.upper)?;
        std::fs::create_dir_all(&self.work)?;
        std::fs::create_dir_all(&self.merged)?;
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
}
