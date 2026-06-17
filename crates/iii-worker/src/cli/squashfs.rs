//! Host-side squashfs base-image builder (pure-Rust `backhand`).
//!
//! Packs an extracted OCI rootfs DIRECTORY into a read-only squashfs image
//! ENTIRELY on the host. It never executes or depends on any tool inside the
//! image (`mke2fs`, `mksquashfs`, …), so it works for ANY base image —
//! debian, alpine, distroless, scratch, or a custom one. The resulting image
//! is the read-only overlay *lower* for the shared-rootfs sandbox model
//! (a per-worker writable upper is layered on top via overlayfs in iii-init).
//!
//! gzip compression is used because the libkrunfw guest kernel's squashfs is
//! built with zlib; it also keeps the dependency free of the xz/zstd C
//! toolchains.

use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use backhand::compression::Compressor;
use backhand::kind::{self, Kind};
use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader, DEFAULT_BLOCK_SIZE};

/// Build a read-only squashfs image at `out` from the rootfs tree at `src`.
///
/// Preserves regular files, directories, and symlinks with their permission
/// bits. Special files (character/block devices, FIFOs, sockets) are skipped:
/// a base rootfs's `/dev` is populated by iii-init (devtmpfs) at boot, not
/// carried in the image.
pub fn build_squashfs(src: &Path, out: &Path) -> Result<(), String> {
    let mut w = FilesystemWriter::default();
    w.set_current_time();
    w.set_block_size(DEFAULT_BLOCK_SIZE);
    w.set_only_root_id();
    w.set_kind(Kind::from_const(kind::LE_V4_0).map_err(|e| format!("squashfs kind: {e}"))?);
    w.set_compressor(
        FilesystemCompressor::new(Compressor::Gzip, None)
            .map_err(|e| format!("squashfs compressor: {e}"))?,
    );
    if let Ok(m) = fs::symlink_metadata(src) {
        w.set_root_mode((m.permissions().mode() & 0o7777) as u16);
    }

    add_dir(&mut w, src, src)?;

    let mut f = File::create(out).map_err(|e| format!("create {}: {e}", out.display()))?;
    w.write(&mut f)
        .map_err(|e| format!("write squashfs {}: {e}", out.display()))?;
    Ok(())
}

/// Recursively push the contents of `dir` (parents before children, so the
/// squashfs writer always has the parent directory before its entries).
fn add_dir(w: &mut FilesystemWriter, root: &Path, dir: &Path) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("read_dir {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .collect();
    // Deterministic order; also keeps parent-before-child within a level.
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let rel = match path.strip_prefix(root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let sqfs_path = Path::new("/").join(rel);
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!("squashfs: skip {} (stat: {e})", path.display());
                continue;
            }
        };
        let header = NodeHeader {
            permissions: (meta.permissions().mode() & 0o7777) as u16,
            uid: 0,
            gid: 0,
            mtime: 0,
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            let target =
                fs::read_link(&path).map_err(|e| format!("readlink {}: {e}", path.display()))?;
            w.push_symlink(target, &sqfs_path, header)
                .map_err(|e| format!("push_symlink {}: {e}", sqfs_path.display()))?;
        } else if ft.is_dir() {
            w.push_dir(&sqfs_path, header)
                .map_err(|e| format!("push_dir {}: {e}", sqfs_path.display()))?;
            add_dir(w, root, &path)?;
        } else if ft.is_file() {
            let file =
                File::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
            w.push_file(file, &sqfs_path, header)
                .map_err(|e| format!("push_file {}: {e}", sqfs_path.display()))?;
        }
        // device / fifo / socket: intentionally skipped (see module doc).
    }
    Ok(())
}

/// Return the cached squashfs path for a base rootfs dir, building it
/// (host-side) on first use. The cache file is `<dir>.sqfs` next to the
/// source. Rebuilds only when missing.
pub fn ensure_base_squashfs(base_dir: &Path) -> Result<std::path::PathBuf, String> {
    let name = base_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("base");
    let out = base_dir.with_file_name(format!("{name}.sqfs"));
    if !out.exists() {
        eprintln!(
            "iii: building read-only base squashfs (host-side, image-independent) from {}...",
            base_dir.display()
        );
        build_squashfs(base_dir, &out)?;
    }
    Ok(out)
}
