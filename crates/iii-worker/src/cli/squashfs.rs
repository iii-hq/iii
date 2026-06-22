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
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use std::sync::atomic::{AtomicU64, Ordering};

use backhand::compression::Compressor;
use backhand::kind::{self, Kind};
use backhand::{DEFAULT_BLOCK_SIZE, FilesystemCompressor, FilesystemWriter, NodeHeader};

/// Per-call sequence so concurrent builds (even within ONE process — the
/// engine starts workers as concurrent async tasks) never share a temp path.
static SQFS_BUILD_SEQ: AtomicU64 = AtomicU64::new(0);

/// Raise the soft fd limit before packing: backhand keeps an open `File` for
/// every regular file until `write()`, so a large image (the node base has ~15k
/// files) exceeds the default soft `RLIMIT_NOFILE` (256 on macOS) and the build
/// fails with EMFILE. Mirrors `vm_boot::raise_fd_limit`.
///
/// Best-effort: a syscall failure is warned, not fatal. Raising is an
/// optimization, not a correctness requirement — if the ambient limit is
/// already adequate the build still succeeds, and if it genuinely isn't the
/// open() in `add_dir` surfaces EMFILE with the offending path.
fn raise_fd_limit() {
    use nix::libc;
    let mut rlim: libc::rlimit = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) } != 0 {
        tracing::warn!(error = %std::io::Error::last_os_error(), "getrlimit(RLIMIT_NOFILE) failed; keeping current fd limit");
        return;
    }
    let target = rlim.rlim_max.min(1_048_576);
    if rlim.rlim_cur < target {
        rlim.rlim_cur = target;
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rlim) } != 0 {
            tracing::warn!(error = %std::io::Error::last_os_error(), "setrlimit(RLIMIT_NOFILE) failed; keeping current fd limit");
        }
    }
}

/// Build a read-only squashfs image at `out` from the rootfs tree at `src`.
///
/// Preserves regular files, directories, and symlinks with their permission
/// bits AND their ownership (uid/gid) and mtime, so a base image that ships
/// non-root-owned files (e.g. `/home/node` owned by uid 1000) boots with the
/// same ownership instead of everything flattened to root.
///
/// Special files (character/block devices, FIFOs, sockets) are skipped: a base
/// rootfs's `/dev` is populated by iii-init (devtmpfs) at boot, not carried in
/// the image.
///
/// Limitation: extended attributes are NOT carried (backhand's `NodeHeader`
/// has no xattr field), so `security.capability` file capabilities are dropped.
/// This is benign for iii because the worker runs as PID 1 / root, which holds
/// all capabilities regardless of per-file caps; revisit if a non-root worker
/// mode is ever added.
pub fn build_squashfs(src: &Path, out: &Path) -> Result<(), String> {
    raise_fd_limit();

    let mut w = FilesystemWriter::default();
    w.set_current_time();
    w.set_block_size(DEFAULT_BLOCK_SIZE);
    // Do NOT set_only_root_id(): we preserve per-node uid/gid below, so the id
    // table must hold the real owners, not just root.
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
            uid: meta.uid(),
            gid: meta.gid(),
            mtime: meta.mtime().clamp(0, u32::MAX as i64) as u32,
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
            let file = File::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
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
        // Per-CALL temp name: the `.sqfs` cache is SHARED across workers on the
        // same base image, so two concurrent first-boots (the engine starting
        // many same-image workers at once, whether as separate processes OR
        // concurrent async tasks in one process) would otherwise build to the
        // same `.partial` path and interleave into a corrupt squashfs. pid +
        // a process-local sequence makes every build's temp unique; the atomic
        // rename then makes it last-writer-wins, each a complete valid image.
        let tmp = base_dir.with_file_name(format!(
            "{}.sqfs.{}.{}.partial",
            base_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("base"),
            std::process::id(),
            SQFS_BUILD_SEQ.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = fs::remove_file(&tmp);
        build_squashfs(base_dir, &tmp).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })?;
        fs::rename(&tmp, &out).map_err(|e| format!("finalize squashfs {}: {e}", out.display()))?;
    }
    Ok(out)
}

/// Remove the cached squashfs (and any leftover partial) for a base rootfs
/// dir. Call when the underlying image/rootfs cache is freed so the shared
/// `.sqfs` doesn't outlive its source. Best-effort; missing files are fine.
pub fn remove_base_squashfs(base_dir: &Path) {
    let out = base_squashfs_path(base_dir);
    let _ = fs::remove_file(&out);
    // Sweep any leftover build partials (`<name>.sqfs.partial` and the
    // per-pid `<name>.sqfs.<pid>.partial`) orphaned by a crashed build.
    let name = base_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("base");
    let prefix = format!("{name}.sqfs");
    if let Some(parent) = out.parent()
        && let Ok(rd) = fs::read_dir(parent)
    {
        for entry in rd.flatten() {
            let fname = entry.file_name();
            let Some(f) = fname.to_str() else { continue };
            if f.starts_with(&prefix) && f.ends_with(".partial") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
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

    #[test]
    fn builds_image_with_many_files_without_fd_exhaustion() {
        let tmp = std::env::temp_dir().join(format!("iii-sqfs-manyfd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("rootfs");
        fs::create_dir_all(&src).unwrap();
        for i in 0..400 {
            let mut f = File::create(src.join(format!("f{i:04}"))).unwrap();
            f.write_all(format!("file {i}\n").as_bytes()).unwrap();
        }
        let out = tmp.join("out.sqfs");
        build_squashfs(&src, &out).expect("many-file build must not fail with EMFILE");
        assert_eq!(
            &fs::read(&out).unwrap()[..4],
            b"hsqs",
            "output is not a squashfs image"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn raise_fd_limit_never_lowers_and_reaches_target() {
        use nix::libc;
        let mut before: libc::rlimit = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut before) },
            0
        );
        raise_fd_limit();
        let mut after: libc::rlimit = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut after) },
            0
        );
        assert!(
            after.rlim_cur >= before.rlim_cur,
            "raise_fd_limit must not lower the limit"
        );
        let want = before.rlim_max.min(1_048_576);
        assert!(
            after.rlim_cur >= want || after.rlim_cur == after.rlim_max,
            "soft limit should reach the target or the hard cap"
        );
    }
}
