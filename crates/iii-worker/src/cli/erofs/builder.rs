// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Pack an extracted OCI rootfs DIRECTORY into a read-only erofs image, entirely
//! on the host. Mirrors the old squashfs builder's contract: it never executes
//! or depends on any tool inside the image, so it works for ANY base image
//! (debian, alpine, distroless, scratch, custom). The result is the read-only
//! overlay *lower* for the shared-rootfs sandbox model; a per-worker writable
//! upper is layered on top via overlayfs in iii-init.
//!
//! Bounded memory: regular-file bytes are streamed into a single on-disk spool
//! (one fd, chunked reads) rather than held in RAM, so a multi-hundred-MB base
//! never inflates the host process. The erofs writer reads file data back from
//! the spool at image-write time.
//!
//! Carried: regular files, directories, symlinks, with permission bits,
//! ownership (uid/gid), and mtime; hardlinks (same dev+ino) share one inode and
//! data extent. NOT carried: device/fifo/socket nodes (the base rootfs's /dev is
//! populated by iii-init's devtmpfs at boot, not shipped in the image) and
//! xattrs (matching the prior squashfs builder; revisit if file capabilities or
//! SELinux labels ever matter for a non-root worker mode).

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::tree::{
    DataSpool, DirectoryNode, FileData, FileTree, InodeMetadata, RegularFileId, RegularFileNode,
    SymlinkNode, TreeNode,
};
use super::write_erofs;

/// Per-call sequence so concurrent builds (even within ONE process — the engine
/// starts workers as concurrent async tasks) never share a temp path.
static EROFS_BUILD_SEQ: AtomicU64 = AtomicU64::new(0);

/// Read-buffer size for streaming file bytes into the spool.
const SPOOL_CHUNK: usize = 64 * 1024;

fn inode_metadata(m: &fs::Metadata) -> InodeMetadata {
    InodeMetadata {
        uid: m.uid(),
        gid: m.gid(),
        // Permission + setuid/setgid/sticky bits only; the erofs writer ORs the
        // S_IFMT type bits from the node kind.
        mode: (m.mode() & 0o7777) as u16,
        mtime: m.mtime().max(0) as u64,
        mtime_nsec: m.mtime_nsec().clamp(0, u32::MAX as i64) as u32,
    }
}

/// Build a read-only erofs image at `out` from the rootfs tree at `src`.
pub fn build_erofs(src: &Path, out: &Path) -> Result<(), String> {
    // Spool lives next to `out` (which the caller already makes unique per
    // build), so concurrent builds never collide. Removed unconditionally below.
    let spool_path = PathBuf::from(format!("{}.spool", out.display()));
    let _ = fs::remove_file(&spool_path);
    let mut spool = DataSpool::new(&spool_path)
        .map_err(|e| format!("create erofs spool {}: {e}", spool_path.display()))?;

    let mut tree = FileTree::new();
    if let Ok(m) = fs::symlink_metadata(src) {
        tree.root.metadata = inode_metadata(&m);
    }

    let mut hardlinks: HashMap<(u64, u64), (RegularFileId, FileData)> = HashMap::new();

    let result = add_dir(&mut tree, &mut spool, &mut hardlinks, src, src).and_then(|()| {
        write_erofs(&tree, out).map_err(|e| format!("write erofs {}: {e}", out.display()))?;
        Ok(())
    });

    drop(spool);
    let _ = fs::remove_file(&spool_path);
    result
}

/// Recursively push the contents of `dir` into the tree (deterministic order so
/// the image is reproducible for a given source).
fn add_dir(
    tree: &mut FileTree,
    spool: &mut DataSpool,
    hardlinks: &mut HashMap<(u64, u64), (RegularFileId, FileData)>,
    root: &Path,
    dir: &Path,
) -> Result<(), String> {
    // Propagate per-entry read errors rather than dropping them: a silently
    // skipped entry would produce a complete-looking but incomplete base image.
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))? {
        entries.push(entry.map_err(|e| format!("read_dir entry in {}: {e}", dir.display()))?);
    }
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let rel = match path.strip_prefix(root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rel_bytes = rel.as_os_str().as_bytes();
        if rel_bytes.is_empty() {
            continue;
        }

        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!("erofs: skip {} (stat: {e})", path.display());
                continue;
            }
        };
        let imeta = inode_metadata(&meta);
        let ft = meta.file_type();

        if ft.is_symlink() {
            let target =
                fs::read_link(&path).map_err(|e| format!("readlink {}: {e}", path.display()))?;
            let node = TreeNode::Symlink(SymlinkNode {
                metadata: imeta,
                target: target.as_os_str().as_bytes().to_vec(),
            });
            tree.insert(rel_bytes, node)
                .map_err(|e| format!("insert symlink {}: {e}", path.display()))?;
        } else if ft.is_dir() {
            tree.insert(rel_bytes, TreeNode::Directory(DirectoryNode::new(imeta)))
                .map_err(|e| format!("insert dir {}: {e}", path.display()))?;
            add_dir(tree, spool, hardlinks, root, &path)?;
        } else if ft.is_file() {
            let key = (meta.dev(), meta.ino());
            let (id, data) = if meta.nlink() > 1 {
                if let Some((eid, edata)) = hardlinks.get(&key) {
                    // Another link to a file already spooled: reuse its inode id
                    // and data extent so the writer emits one inode + one copy.
                    (*eid, edata.clone())
                } else {
                    let data = spool_file(spool, &path)?;
                    let id = RegularFileId::new();
                    hardlinks.insert(key, (id, data.clone()));
                    (id, data)
                }
            } else {
                (RegularFileId::new(), spool_file(spool, &path)?)
            };
            let node = TreeNode::RegularFile(RegularFileNode {
                id,
                metadata: imeta,
                xattrs: Vec::new(),
                data,
                // nlink is recomputed by the writer from id occurrences; the
                // value here is a placeholder.
                nlink: 1,
            });
            tree.insert(rel_bytes, node)
                .map_err(|e| format!("insert file {}: {e}", path.display()))?;
        }
        // device / fifo / socket: intentionally skipped (see module doc).
    }
    Ok(())
}

/// Stream a regular file's bytes into the spool and return a `FileData` handle
/// pointing at the written extent. Reads in fixed chunks so a large file never
/// lands in memory in full.
fn spool_file(spool: &mut DataSpool, path: &Path) -> Result<FileData, String> {
    let start = spool.current_offset();
    let mut f = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut buf = [0u8; SPOOL_CHUNK];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        spool
            .write_chunk(&buf[..n])
            .map_err(|e| format!("spool write {}: {e}", path.display()))?;
    }
    let len = spool.current_offset() - start;
    Ok(spool.data_ref(start, len))
}

/// The cache path for a base rootfs dir's erofs image: `<dir>.erofs` next to it.
pub fn base_erofs_path(base_dir: &Path) -> PathBuf {
    let name = base_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("base");
    base_dir.with_file_name(format!("{name}.erofs"))
}

/// Return the cached erofs path for a base rootfs dir, building it (host-side)
/// on first use and rebuilding when stale. The cache file is `<dir>.erofs` next
/// to the source.
///
/// Staleness: rebuild when the `.erofs` is missing OR older than the source
/// dir's mtime. OCI/base rootfs cache dirs are immutable once extracted (a
/// re-pull replaces the dir, bumping its mtime), so a top-level mtime compare
/// catches re-extraction without walking the whole tree on every boot. The
/// build is atomic (temp + rename) so an interrupted build never leaves a torn
/// `.erofs` that a later boot would attach as a corrupt overlay lower.
pub fn ensure_base_erofs(base_dir: &Path) -> Result<PathBuf, String> {
    let out = base_erofs_path(base_dir);

    let stale = match (fs::metadata(&out), fs::metadata(base_dir)) {
        (Ok(o), Ok(b)) => match (o.modified(), b.modified()) {
            (Ok(om), Ok(bm)) => om < bm,
            _ => false,
        },
        (Ok(_), Err(_)) => false,
        (Err(_), _) => true,
    };

    if stale {
        eprintln!(
            "iii: building read-only base erofs (host-side, image-independent) from {}...",
            base_dir.display()
        );
        // Per-CALL temp name: the `.erofs` cache is SHARED across workers on the
        // same base image, so two concurrent first-boots would otherwise build
        // to the same path and interleave into a corrupt image. pid + a
        // process-local sequence makes every build's temp unique; the atomic
        // rename then makes it last-writer-wins, each a complete valid image.
        let tmp = base_dir.with_file_name(format!(
            "{}.erofs.{}.{}.partial",
            base_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("base"),
            std::process::id(),
            EROFS_BUILD_SEQ.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = fs::remove_file(&tmp);
        build_erofs(base_dir, &tmp).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })?;
        fs::rename(&tmp, &out).map_err(|e| format!("finalize erofs {}: {e}", out.display()))?;
    }
    Ok(out)
}

/// Remove the cached erofs (and any leftover partial/spool) for a base rootfs
/// dir. Call when the underlying image/rootfs cache is freed so the shared
/// `.erofs` doesn't outlive its source. Best-effort; missing files are fine.
pub fn remove_base_erofs(base_dir: &Path) {
    let out = base_erofs_path(base_dir);
    let _ = fs::remove_file(&out);
    // Migration hygiene: a base cached before the squashfs->erofs pivot has an
    // orphaned `<name>.sqfs` sibling that nothing reads anymore. Remove it at the
    // same re-extract boundary so upgraded hosts don't accumulate dead images.
    let _ = fs::remove_file(out.with_extension("sqfs"));
    let name = base_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("base");
    let prefix = format!("{name}.erofs");
    if let Some(parent) = out.parent()
        && let Ok(rd) = fs::read_dir(parent)
    {
        for entry in rd.flatten() {
            let fname = entry.file_name();
            let Some(f) = fname.to_str() else { continue };
            // Sweep leftover build partials and spools orphaned by a crash.
            if f.starts_with(&prefix) && (f.ends_with(".partial") || f.ends_with(".spool")) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ErofsReader;
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
        let tmp = std::env::temp_dir().join(format!("iii-erofs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("rootfs");
        make_src(&src);

        // First call builds the cache next to the source.
        let out = ensure_base_erofs(&src).unwrap();
        assert_eq!(out, base_erofs_path(&src));
        assert!(out.exists());

        // Round-trips through the ported reader: proves the image is a valid
        // erofs the kernel-compatible reader can parse, and content survives.
        let file = File::open(&out).unwrap();
        let mut reader = ErofsReader::new(file).expect("valid erofs image");
        assert_eq!(
            reader.read_file("/bin/sh").unwrap(),
            b"#!/bin/sh\necho hi\n"
        );

        // Force the cache to look older than the source dir -> stale -> rebuild.
        filetime::set_file_mtime(&out, filetime::FileTime::from_unix_time(1_000_000, 0)).unwrap();
        ensure_base_erofs(&src).unwrap();
        let m = fs::metadata(&out)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(m > 1_000_000, "stale erofs should have been rebuilt");

        // GC removes the cache (and any partial/spool).
        remove_base_erofs(&src);
        assert!(!out.exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn symlink_target_round_trips() {
        let tmp = std::env::temp_dir().join(format!("iii-erofs-meta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("rootfs");
        make_src(&src);

        let out = base_erofs_path(&src);
        build_erofs(&src, &out).unwrap();

        let file = File::open(&out).unwrap();
        let mut reader = ErofsReader::new(file).unwrap();
        // bin/ash -> sh symlink survives the build.
        assert_eq!(reader.read_link("/bin/ash").unwrap(), b"sh");

        let _ = fs::remove_dir_all(&tmp);
    }
}
