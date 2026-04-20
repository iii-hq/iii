// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Pivot the guest root off the libkrun virtiofs share onto a fresh
//! tmpfs, preserving access to the rootfs content via bind mounts.
//!
//! ## Why
//!
//! libkrun's virtiofs implementation has a readdir bug on the shared
//! directory's root: `getdents64` appears to return duplicate or
//! looping entries for `/`, causing userspace tools like `ls` to
//! accumulate ~90+ MiB of dirent state before the guest OOM-kills
//! them. Observable symptom: `ls /` dies with "Killed" (SIGKILL,
//! exit 137) even on an idle VM with plenty of free memory.
//!
//! The bug is localized to the virtiofs mount's top-level
//! directory. Deeper paths (`/etc`, `/usr/bin`, …) read correctly.
//! So the fix is to replace `/` with something kernel-backed
//! (tmpfs) and expose the rootfs content via bind mounts of its
//! subdirectories. After pivot, `ls /` reads from the tmpfs (clean,
//! well-behaved dirents) and `ls /etc` reads through the bind mount
//! into the original virtiofs (which works fine).
//!
//! ## Layout after pivot
//!
//! ```text
//! /              tmpfs (our new root, ~25 dirents, no bug)
//! /bin           bind → virtiofs:/bin
//! /etc           bind → virtiofs:/etc
//! /usr           bind → virtiofs:/usr
//! /var           bind → virtiofs:/var
//! ... (every rootfs top-level the image ships)
//! /workspace     original virtiofs_0 mount, moved via MS_MOVE
//! /proc, /sys, /dev, /tmp, /run
//!                empty mount points — mount_filesystems() mounts
//!                the kernel filesystems on top after we return
//! ```
//!
//! The old virtiofs root stays pinned in the kernel's mount table
//! (referenced by the bind mounts) but is not exposed in any
//! namespace path. `ls /` can never touch it again.
//!
//! ## Caveats
//!
//! - Top-level entries are enumerated dynamically from `/` so
//!   non-FHS paths common in OCI images (e.g. `/app`, `/srv/app`)
//!   get preserved alongside the standard FHS directories. Only the
//!   names in [`SKIP_TOP_LEVEL`] (kernel fs mount points, pivot
//!   staging, and `/workspace`) are skipped. Worker-writable state
//!   that needs to persist across restarts should live in
//!   `/workspace` (which is the virtiofs_0 mount used for user code)
//!   or under `/var` (bind-mounted).
//! - If pivot fails part-way, we leave the VM in a partially
//!   constructed state and the `Err` propagates to `main()`, which
//!   aborts PID 1 — libkrun will then surface a boot failure to the
//!   host. That's preferable to continuing with an inconsistent
//!   mount table.

use std::fs;
use std::path::Path;

use nix::mount::{MntFlags, MsFlags, mount, umount2};
use nix::sys::stat::Mode;
use nix::unistd::{chdir, mkdir, pivot_root};

use crate::error::InitError;

/// Names we deliberately skip when enumerating the source rootfs's
/// top-level for bind-mounts:
///
/// - `proc`, `sys`, `dev`, `tmp`, `run` — kernel filesystems that
///   `mount_filesystems()` mounts FRESH on the new tmpfs root right
///   after we return. Bind-mounting the old ones would mask those
///   fresh mounts.
/// - `new-root`, `old-root` — the pivot-root staging dirs we created
///   on this same rootfs. Nested bind-mounts of our own state would
///   be meaningless.
/// - `workspace` — the virtiofs_0 user-code mount, relocated with
///   `MS_MOVE` in a dedicated phase so its sub-mounts survive the
///   pivot atomically.
const SKIP_TOP_LEVEL: &[&str] = &[
    "proc",
    "sys",
    "dev",
    "tmp",
    "run",
    "new-root",
    "old-root",
    "workspace",
];

/// Mount points mount_filesystems() will populate after we pivot.
/// Creating the dirs here (on the tmpfs) keeps mount_filesystems
/// unchanged and self-contained.
const KERNEL_FS_DIRS: &[(&str, u32)] = &[
    ("proc", 0o755),
    ("sys", 0o755),
    ("dev", 0o755),
    ("tmp", 0o1777),
    ("run", 0o755),
];

const NEW_ROOT: &str = "/new-root";
const PIVOT_PUT_OLD: &str = "/new-root/old-root";

/// Perform the pivot. Safe to call once, at boot, before
/// `mount_filesystems()`. Idempotency is not guaranteed — calling
/// twice will fail at the tmpfs mount step.
pub fn pivot_to_tmpfs_root() -> Result<(), InitError> {
    // Phase 1 — stage the new root.

    mkdir_ignore_exists(NEW_ROOT, 0o755)?;

    // Mount the replacement tmpfs. `mode=755` keeps permissions
    // conventional; the user worker runs as root so stricter modes
    // buy nothing.
    mount(
        Some("tmpfs"),
        NEW_ROOT,
        Some("tmpfs"),
        MsFlags::empty(),
        Some("mode=755"),
    )
    .map_err(|e| InitError::Mount {
        target: NEW_ROOT.into(),
        source: e,
    })?;

    // Make the new tmpfs a private mount so post-pivot changes
    // don't propagate to anything that might share (libkrun doesn't
    // create shared mounts, but this is belt-and-suspenders).
    mount(
        None::<&str>,
        NEW_ROOT,
        None::<&str>,
        MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .map_err(|e| InitError::Mount {
        target: NEW_ROOT.into(),
        source: e,
    })?;

    // Phase 2 — bind-mount every top-level entry of the source
    // rootfs into the new root.
    //
    // Each bind creates an independent mount entry that points at
    // the same underlying virtiofs files. After we later umount the
    // old root with MNT_DETACH, these binds remain live and keep
    // the virtiofs superblock pinned — so files are still readable
    // via `/bin`, `/etc`, etc., without the host ever seeing a
    // readdir on the virtiofs root again.
    //
    // We enumerate dynamically rather than match a fixed FHS
    // allowlist because OCI images routinely ship their app payload
    // at non-FHS top-level paths (Docker's convention is `/app`, but
    // projects also use `/srv/app`, `/service`, etc.). Hardcoding
    // FHS made every such image boot-fail with a cryptic
    // `chdir(/app): ENOENT` from the supervisor's `spawn_child` —
    // the exact symptom that prompted adding this dynamic pass.
    let entries = enumerate_rootfs_entries(Path::new("/"))?;
    for (name, is_dir) in &entries {
        let source = format!("/{name}");
        let target = format!("{NEW_ROOT}/{name}");
        if *is_dir {
            mkdir_ignore_exists(&target, 0o755)?;
        } else {
            // Bind target for a regular file must exist as a file.
            // Create empty; the bind mount covers its content.
            let _ = fs::File::create(&target);
        }
        mount(
            Some(source.as_str()),
            target.as_str(),
            None::<&str>,
            MsFlags::MS_BIND,
            None::<&str>,
        )
        .map_err(|e| InitError::Mount { target, source: e })?;
    }

    // Phase 3 — relocate the /workspace virtiofs mount so it
    // survives the pivot.
    //
    // libkrun attaches the user's workspace at `/workspace` via a
    // separate virtiofs device before PID 1 runs. pivot_root would
    // leave it under `/old-root/workspace`; we don't want that.
    // MS_MOVE relocates the existing mount tree (with all its
    // sub-mounts — the deps bind mounts under /workspace/node_modules
    // etc.) atomically into `/new-root/workspace`.
    //
    // Best-effort: if `/workspace` is not a distinct mount point
    // (e.g., the image didn't have it), skip silently.
    if is_mount_point("/workspace") {
        let target = format!("{NEW_ROOT}/workspace");
        mkdir_ignore_exists(&target, 0o755)?;
        mount(
            Some("/workspace"),
            target.as_str(),
            None::<&str>,
            MsFlags::MS_MOVE,
            None::<&str>,
        )
        .map_err(|e| InitError::Mount { target, source: e })?;
    }

    // Phase 4 — pre-create kernel-filesystem mount points on the
    // new root so `mount_filesystems()` can mount onto them
    // unchanged.
    for (dir, mode) in KERNEL_FS_DIRS {
        mkdir_ignore_exists(&format!("{NEW_ROOT}/{dir}"), *mode)?;
    }
    // `/dev/pts` and `/dev/shm` live inside /dev, which is a kernel
    // mount — mount_filesystems creates those at runtime on the
    // devtmpfs, no need to pre-create here.

    // Phase 5 — create the pivot_root put_old target. Must be a
    // directory under the new root.
    mkdir_ignore_exists(PIVOT_PUT_OLD, 0o755)?;

    // Phase 6 — pivot.
    //
    // pivot_root(".", "old-root") with cwd=NEW_ROOT: the kernel
    // swaps the new root in at `/` and parks the old root at
    // `/old-root`. The `.`/relative-path idiom matches man-pivot_root(2)
    // recommendations and avoids the quirky "same-fs" rejection
    // some kernels apply to absolute new_root arguments.
    chdir(NEW_ROOT).map_err(|e| InitError::Mount {
        target: NEW_ROOT.into(),
        source: e,
    })?;
    pivot_root(".", "old-root").map_err(|e| InitError::Mount {
        target: "pivot_root".into(),
        source: e,
    })?;
    chdir("/").map_err(|e| InitError::Mount {
        target: "/".into(),
        source: e,
    })?;

    // Phase 7 — detach the old root. MNT_DETACH is essential:
    // pre-existing mounts from libkrun (devtmpfs, proc, sysfs, any
    // virtiofs aux shares) are children of the old root, so a
    // plain umount would fail EBUSY. MNT_DETACH unmounts lazily —
    // the filesystem disappears from the namespace immediately but
    // the kernel keeps it alive until all its fds/child-mounts are
    // gone. Bind mounts into the old virtiofs keep the virtiofs
    // superblock alive as long as we need it (i.e., until shutdown).
    umount2("/old-root", MntFlags::MNT_DETACH).map_err(|e| InitError::Mount {
        target: "/old-root".into(),
        source: e,
    })?;
    let _ = fs::remove_dir("/old-root");

    Ok(())
}

fn mkdir_ignore_exists(path: &str, mode: u32) -> Result<(), InitError> {
    match mkdir(path, Mode::from_bits_truncate(mode)) {
        Ok(()) | Err(nix::Error::EEXIST) => Ok(()),
        Err(e) => Err(InitError::Mkdir {
            path: path.into(),
            source: e,
        }),
    }
}

/// Enumerate `root`'s direct children for bind-mount propagation
/// into the new tmpfs root, returning `(name, is_directory)` pairs.
///
/// Excludes [`SKIP_TOP_LEVEL`] (kernel fs mount points, pivot
/// staging, and `/workspace` which is handled via MS_MOVE in a
/// later phase).
///
/// `is_directory` follows symlinks so `/bin -> /usr/bin` (Debian
/// layout) reports as a directory, and the caller can pre-create
/// the bind target as a directory. A broken symlink or stat error
/// filters the entry out silently, matching the old allowlist's
/// `Path::exists()` behavior.
///
/// Output is sorted so the bind-mount order is deterministic across
/// boots; makes `/proc/self/mountinfo` diffs meaningful when
/// debugging.
fn enumerate_rootfs_entries(root: &Path) -> Result<Vec<(String, bool)>, InitError> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let iter = fs::read_dir(root).map_err(|e| InitError::Mount {
        target: root.to_string_lossy().into_owned(),
        source: nix::Error::from_raw(e.raw_os_error().unwrap_or(libc::EIO)),
    })?;
    for entry in iter {
        let entry = match entry {
            Ok(e) => e,
            // Broken entry; skip rather than abort the whole pivot.
            Err(_) => continue,
        };
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            // Non-UTF-8 name: extremely unlikely in an OCI rootfs;
            // fall back to the lossy form so we don't silently drop
            // a directory the user cares about.
            Err(os) => os.to_string_lossy().into_owned(),
        };
        if name.is_empty() || SKIP_TOP_LEVEL.contains(&name.as_str()) {
            continue;
        }
        // Use `metadata` (follows symlinks) so a Debian-style
        // `bin -> usr/bin` symlink reports as a directory and we
        // create the bind target with `mkdir`, not `File::create`.
        let is_dir = match fs::metadata(entry.path()) {
            Ok(m) => m.is_dir(),
            Err(_) => continue, // broken symlink or raced deletion
        };
        out.push((name, is_dir));
    }
    out.sort();
    Ok(out)
}

/// Check whether `path` is the root of a distinct mount. Uses the
/// classic stat trick: a mount root has a different `st_dev` than
/// its parent. Returns false for missing paths or stat errors —
/// callers treat "not a mount point" as "nothing to relocate".
fn is_mount_point(path: &str) -> bool {
    let p = Path::new(path);
    let parent = p.parent().unwrap_or(Path::new("/"));
    match (nix::sys::stat::stat(p), nix::sys::stat::stat(parent)) {
        (Ok(a), Ok(b)) => a.st_dev != b.st_dev,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn names_of(entries: &[(String, bool)]) -> Vec<&str> {
        entries.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Regression for the OCI `/app` boot failure: an image that
    /// ships its payload at a non-FHS top-level directory must
    /// appear in the bind-mount set so that, after pivot, the
    /// supervisor's `chdir(/app)` succeeds.
    #[test]
    fn enumerate_includes_non_fhs_app_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        for name in ["app", "bin", "etc", "usr", "var"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let entries = enumerate_rootfs_entries(root).unwrap();
        let names = names_of(&entries);
        assert!(
            names.contains(&"app"),
            "`/app` must be enumerated for OCI images"
        );
        assert!(names.contains(&"bin"));
        assert!(names.contains(&"usr"));
    }

    #[test]
    fn enumerate_skips_kernel_filesystems_and_staging() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        for name in [
            "proc",
            "sys",
            "dev",
            "tmp",
            "run",
            "new-root",
            "old-root",
            "workspace",
            "etc",
        ] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let entries = enumerate_rootfs_entries(root).unwrap();
        let names = names_of(&entries);
        assert_eq!(
            names,
            vec!["etc"],
            "kernel fs, pivot staging, and workspace must be filtered; got {names:?}"
        );
    }

    #[test]
    fn enumerate_reports_symlinked_directories_as_directories() {
        // Debian layout: `/bin -> usr/bin`. The bind target must be
        // pre-created as a directory, not a file, so report is_dir=true.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("usr")).unwrap();
        fs::create_dir(root.join("usr/bin")).unwrap();
        symlink("usr/bin", root.join("bin")).unwrap();

        let entries = enumerate_rootfs_entries(root).unwrap();
        let (_, is_dir) = entries
            .iter()
            .find(|(n, _)| n == "bin")
            .expect("bin symlink should be enumerated");
        assert!(*is_dir, "symlink to directory must report as directory");
    }

    #[test]
    fn enumerate_preserves_regular_files_like_init_krun() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("init.krun"), b"#!/bin/sh\n").unwrap();
        let entries = enumerate_rootfs_entries(root).unwrap();
        let (_, is_dir) = entries
            .iter()
            .find(|(n, _)| n == "init.krun")
            .expect("init.krun should be enumerated");
        assert!(!*is_dir, "regular file must report is_dir=false");
    }

    #[test]
    fn enumerate_drops_broken_symlinks() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        symlink("does/not/exist", root.join("dangling")).unwrap();
        fs::create_dir(root.join("etc")).unwrap();
        let entries = enumerate_rootfs_entries(root).unwrap();
        let names = names_of(&entries);
        assert!(
            !names.contains(&"dangling"),
            "broken symlink must be filtered; got {names:?}"
        );
        assert!(names.contains(&"etc"));
    }

    #[test]
    fn enumerate_is_sorted_deterministically() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        for name in ["var", "etc", "app", "usr", "bin"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let entries = enumerate_rootfs_entries(root).unwrap();
        let names = names_of(&entries);
        assert_eq!(names, vec!["app", "bin", "etc", "usr", "var"]);
    }
}
