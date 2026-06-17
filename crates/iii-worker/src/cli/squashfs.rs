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

/// The cache path for a base rootfs dir's squashfs: `<dir>.sqfs` next to it.
pub fn base_squashfs_path(base_dir: &Path) -> std::path::PathBuf {
    let name = base_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("base");
    base_dir.with_file_name(format!("{name}.sqfs"))
}

/// Return the cached squashfs path for a base rootfs dir, building it
/// (host-side) on first use and rebuilding when stale. The cache file is
/// `<dir>.sqfs` next to the source.
///
/// Staleness: rebuild when the `.sqfs` is missing OR older than the source
/// dir's mtime. OCI/base rootfs cache dirs are immutable once extracted (a
/// re-pull replaces the dir, bumping its mtime), so a top-level mtime compare
/// catches re-extraction without walking the whole tree on every boot. The
/// build is atomic (temp + rename) so an interrupted build never leaves a
/// torn `.sqfs` that a later boot would attach as a corrupt overlay lower.
pub fn ensure_base_squashfs(base_dir: &Path) -> Result<std::path::PathBuf, String> {
    let out = base_squashfs_path(base_dir);

    let stale = match (fs::metadata(&out), fs::metadata(base_dir)) {
        // Both present: rebuild if the cache predates the source dir.
        (Ok(o), Ok(b)) => match (o.modified(), b.modified()) {
            (Ok(om), Ok(bm)) => om < bm,
            // mtime unavailable on this fs: trust the existing cache.
            _ => false,
        },
        // Cache exists, source dir stat failed: keep the cache.
        (Ok(_), Err(_)) => false,
        // No cache yet: build.
        (Err(_), _) => true,
    };

    if stale {
        eprintln!(
            "iii: building read-only base squashfs (host-side, image-independent) from {}...",
            base_dir.display()
        );
        let tmp = base_dir.with_file_name(format!(
            "{}.sqfs.partial",
            base_dir.file_name().and_then(|s| s.to_str()).unwrap_or("base")
        ));
        let _ = fs::remove_file(&tmp);
        build_squashfs(base_dir, &tmp).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })?;
        fs::rename(&tmp, &out)
            .map_err(|e| format!("finalize squashfs {}: {e}", out.display()))?;
    }
    Ok(out)
}

/// Remove the cached squashfs (and any leftover partial) for a base rootfs
/// dir. Call when the underlying image/rootfs cache is freed so the shared
/// `.sqfs` doesn't outlive its source. Best-effort; missing files are fine.
pub fn remove_base_squashfs(base_dir: &Path) {
    let out = base_squashfs_path(base_dir);
    let _ = fs::remove_file(&out);
    let name = base_dir.file_name().and_then(|s| s.to_str()).unwrap_or("base");
    let _ = fs::remove_file(base_dir.with_file_name(format!("{name}.sqfs.partial")));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::UNIX_EPOCH;

    fn make_src(dir: &Path) {
        fs::create_dir_all(dir.join("bin")).unwrap();
        let mut f = File::create(dir.join("bin/sh")).unwrap();
        f.write_all(b"#!/bin/sh\necho hi\n").unwrap();
        std::os::unix::fs::symlink("sh", dir.join("bin/ash")).unwrap();
    }

    #[test]
    fn builds_caches_rebuilds_on_stale_and_removes() {
        let tmp = std::env::temp_dir().join(format!("iii-sqfs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("rootfs");
        make_src(&src);

        // First call builds the cache next to the source.
        let out = ensure_base_squashfs(&src).unwrap();
        assert_eq!(out, base_squashfs_path(&src));
        assert!(out.exists());
        // Readable as a squashfs (magic "hsqs" for LE v4).
        let magic = fs::read(&out).unwrap()[..4].to_vec();
        assert_eq!(&magic, b"hsqs", "output is not a squashfs image");

        // Force the cache to look older than the source dir -> stale -> rebuild.
        filetime::set_file_mtime(&out, filetime::FileTime::from_unix_time(1_000_000, 0)).unwrap();
        ensure_base_squashfs(&src).unwrap();
        let m = fs::metadata(&out)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(m > 1_000_000, "stale squashfs should have been rebuilt");

        // GC removes the cache (and any partial).
        remove_base_squashfs(&src);
        assert!(!out.exists());

        let _ = fs::remove_dir_all(&tmp);
    }
}
