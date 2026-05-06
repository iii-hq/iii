// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
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
//! - Any top-level entry in the rootfs not in our hardcoded
//!   allowlist is unreachable post-pivot. OCI images follow a
//!   standard FHS layout so the allowlist covers the common case.
//!   Worker-writable state that needs to persist across restarts
//!   should live in `/workspace` (which is the virtiofs_0 mount
//!   used for user code) or under `/var` (bind-mounted).
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

/// Top-level entries we bind-mount from the source rootfs into the
/// new tmpfs root.
///
/// We can NOT enumerate the rootfs root via `readdir`: libkrun's
/// virtiofs has a getdents64 bug on the share's top-level directory
/// (see module header) that OOM-kills any process reading it. So we
/// stat a curated list of well-known paths and bind-mount the ones
/// that exist. `Path::exists()` on a specific name is a lookup (1 stat
/// call), not a readdir — it doesn't trip the bug.
///
/// Coverage:
/// - FHS standard dirs (bin, etc, usr, var, …) — every Debian/Alpine
///   base image ships these.
/// - `app`, `srv`, `service`, `opt` — the common non-FHS payload
///   paths OCI images use. `/app` is Docker's dominant convention
///   (official node, python, ruby image templates all use it);
///   `/srv` is FHS-standard for service data; `/service` appears in
///   s6/runit images. `/opt` was already in the FHS block.
/// - `init.krun` (file, not directory) — libkrun ships this at the
///   rootfs root. Bind-mounting files works the same as directories,
///   only the pre-creation target type differs.
///
/// `(name, is_directory)`. The dynamic workdir top-level component
/// from `III_WORKER_WORKDIR` is appended at runtime so images with
/// bespoke layouts (e.g. `WorkingDir: /service/current`) still boot.
const ROOTFS_ENTRIES: &[(&str, bool)] = &[
    ("app", true),
    ("bin", true),
    ("boot", true),
    ("etc", true),
    ("home", true),
    ("lib", true),
    ("lib64", true),
    ("media", true),
    ("mnt", true),
    ("opt", true),
    ("root", true),
    ("sbin", true),
    ("service", true),
    ("srv", true),
    ("usr", true),
    ("var", true),
    ("init.krun", false),
];

/// Env var that carries the worker's working directory from the host
/// into the guest. Set by `iii-worker __vm-boot` for OCI workers so
/// we can ensure the workdir's top-level component is bind-mounted
/// even if it's not in [`ROOTFS_ENTRIES`].
const III_WORKER_WORKDIR_ENV: &str = "III_WORKER_WORKDIR";

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

    // Phase 2 — bind-mount rootfs entries from our curated allowlist
    // (plus the runtime workdir's top-level, if set) into the new root.
    //
    // Each bind creates an independent mount entry that points at
    // the same underlying virtiofs files. After we later umount the
    // old root with MNT_DETACH, these binds remain live and keep
    // the virtiofs superblock pinned — so files are still readable
    // via `/bin`, `/etc`, etc., without the host ever seeing a
    // readdir on the virtiofs root again.
    //
    // We deliberately DO NOT readdir the source root — libkrun's
    // virtiofs has a getdents64 bug there that OOM-kills the caller
    // (see module header). Stat-by-name via `Path::exists()` is a
    // single lookup per name, which the bug doesn't reach.
    //
    // Missing entries are silently skipped — images that don't ship
    // e.g. `/service` don't need the bind. Extra entries that exist
    // in the image but aren't in our list stay inaccessible after
    // pivot; the runtime workdir injection below handles the one
    // case that actually matters (image-defined WorkingDir).
    let entries = rootfs_bind_entries(Path::new("/"));
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

/// Build the list of `(name, is_directory)` entries to bind-mount from
/// `source_root` into the new tmpfs root. Pure function — no syscalls
/// beyond `exists()` per candidate name — so unit tests can exercise
/// every selection rule against a tempdir-based fake rootfs without
/// needing root privileges or real mounts.
///
/// The set is the union of [`ROOTFS_ENTRIES`] and (if set and not
/// already present) the top-level component of `III_WORKER_WORKDIR`.
/// Candidates that don't exist under `source_root` are dropped so
/// images that lack e.g. `/service` don't cause a bind-mount error.
///
/// This function MUST NOT call `fs::read_dir` on `source_root` —
/// libkrun's virtiofs has a getdents64 bug on the shared root dir
/// (see module header) that OOM-kills the caller. The regression
/// tests `bind_entries_ignores_non_allowlisted_dirs` and
/// `bind_entries_does_not_read_source_root_directory` are there to
/// trip anyone who adds dynamic enumeration back in.
fn rootfs_bind_entries(source_root: &Path) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = ROOTFS_ENTRIES
        .iter()
        .filter(|(name, _)| source_root.join(name).exists())
        .map(|(n, d)| ((*n).to_string(), *d))
        .collect();
    if let Some(extra) = workdir_top_level_from_env()
        && !out.iter().any(|(n, _)| n == &extra)
        && source_root.join(&extra).exists()
    {
        out.push((extra, true));
    }
    out
}

/// Return the top-level component of `III_WORKER_WORKDIR` so it can
/// be added to the bind-mount set. `/app/dist` → `"app"`,
/// `/srv/myservice/current` → `"srv"`, `/` or unset → `None`.
///
/// Naming restrictions match the image-layout names we trust: letters,
/// digits, dot, dash, underscore. Anything else (whitespace, control
/// bytes, shell metacharacters, `..`) returns None. iii-worker builds
/// the env value from a trusted OCI config, but a malicious image
/// could still encode weird names — refusing to act on them is safer
/// than trying to sanitize.
fn workdir_top_level_from_env() -> Option<String> {
    let raw = std::env::var(III_WORKER_WORKDIR_ENV).ok()?;
    let first = raw.trim_start_matches('/').split('/').next()?;
    if first.is_empty() {
        return None;
    }
    let ok = first
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
    if !ok || first == ".." || first == "." {
        return None;
    }
    Some(first.to_string())
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

    /// Shared across every test that touches `III_WORKER_WORKDIR`.
    /// Per-test `static LOCK` inside each function only serializes
    /// repeat entries into *that* test, not between different tests
    /// that both mutate the same env var — under `cargo test --jobs>1`
    /// two tests could still race and flip the var mid-read. One
    /// module-level mutex fixes that.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Run `f` with `III_WORKER_WORKDIR` set (or unset) under
    /// [`ENV_LOCK`]. Always unsets on exit, even on panic, so a
    /// failing test doesn't poison later ones. Exists so new tests
    /// can't forget the lock dance.
    fn with_workdir_env<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                unsafe { std::env::remove_var(III_WORKER_WORKDIR_ENV) };
            }
        }
        let _cleanup = Guard;
        match value {
            Some(v) => unsafe { std::env::set_var(III_WORKER_WORKDIR_ENV, v) },
            None => unsafe { std::env::remove_var(III_WORKER_WORKDIR_ENV) },
        }
        f()
    }

    /// Regression for the OCI `/app` boot failure: `/app` must be in
    /// the allowlist so images with `WorkingDir: /app` have that dir
    /// after pivot. Guards against anyone trimming the list back to
    /// strict FHS in the future.
    #[test]
    fn allowlist_includes_app_for_oci_images() {
        let names: Vec<&str> = ROOTFS_ENTRIES.iter().map(|(n, _)| *n).collect();
        assert!(
            names.contains(&"app"),
            "`/app` must be in the allowlist for OCI images"
        );
    }

    #[test]
    fn allowlist_skips_kernel_filesystems_and_staging() {
        let names: Vec<&str> = ROOTFS_ENTRIES.iter().map(|(n, _)| *n).collect();
        for banned in [
            "proc",
            "sys",
            "dev",
            "tmp",
            "run",
            "new-root",
            "old-root",
            "workspace",
        ] {
            assert!(
                !names.contains(&banned),
                "{banned} must not be in the allowlist (handled separately or mounted fresh)"
            );
        }
    }

    #[test]
    fn workdir_env_extracts_top_level() {
        for (val, expected) in [
            ("/app", Some("app")),
            ("/app/dist", Some("app")),
            ("/srv/myservice/current", Some("srv")),
            ("/home/node/work", Some("home")),
            // Leading whitespace or shell chars => reject.
            (" /app", None),
            ("/; rm -rf", None),
            ("/..", None),
            ("/.", None),
            // Root or empty => None.
            ("/", None),
            ("", None),
        ] {
            let got = with_workdir_env(Some(val), workdir_top_level_from_env);
            assert_eq!(
                got.as_deref(),
                expected,
                "workdir_top_level_from_env({val:?}) = {got:?}, expected {expected:?}"
            );
        }
        assert_eq!(with_workdir_env(None, workdir_top_level_from_env), None);
    }

    // Tests below hit `rootfs_bind_entries` against a tempdir-based
    // fake rootfs. They exist to lock in the behavior that caused the
    // OCI-worker outage: dynamic `read_dir(source_root)` triggers a
    // libkrun virtiofs OOM. Each test below would fail if someone
    // replaces the stat-by-name pass with enumeration of the source
    // root, so reverting to the broken approach is blocked.

    fn names_of(entries: &[(String, bool)]) -> Vec<&str> {
        entries.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// End-to-end: an OCI-style rootfs with `/app` present must have
    /// `/app` in the bind set. This is the exact symptom the outage
    /// produced — without `/app`, the in-VM supervisor's
    /// `chdir("/app")` returns ENOENT after pivot.
    #[test]
    fn bind_entries_includes_app_when_present_in_rootfs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        for name in ["app", "bin", "etc", "usr", "var"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let entries = rootfs_bind_entries(root);
        let names = names_of(&entries);
        assert!(
            names.contains(&"app"),
            "`/app` must be bound for OCI images; got {names:?}"
        );
        assert!(names.contains(&"bin"));
    }

    /// Allowlisted names that don't exist in the source rootfs get
    /// filtered out. Without this filter, `mount(/lib64)` on an
    /// Alpine image (no lib64) would fail and abort boot.
    #[test]
    fn bind_entries_omits_missing_allowlist_entries() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        // Only `/bin` exists; every other allowlist entry is absent.
        fs::create_dir(root.join("bin")).unwrap();
        let entries = rootfs_bind_entries(root);
        let names = names_of(&entries);
        assert_eq!(
            names,
            vec!["bin"],
            "missing allowlist entries must be dropped; got {names:?}"
        );
    }

    /// Regression for the cherry-picked dynamic-enumeration fix that
    /// OOM-killed iii-init. A rootfs with non-allowlisted top-level
    /// directories (`/rogue`, `/secrets`, image-specific junk) must
    /// NOT pick those up — we are curated-allowlist-only. If this
    /// test starts failing, someone re-introduced `read_dir(root)`.
    #[test]
    fn bind_entries_ignores_non_allowlisted_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        for name in ["bin", "etc", "rogue", "secrets", "xyzzy", ".hidden"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let entries = rootfs_bind_entries(root);
        let names = names_of(&entries);
        for leaked in ["rogue", "secrets", "xyzzy", ".hidden"] {
            assert!(
                !names.contains(&leaked),
                "{leaked} leaked into the bind set — has someone re-added readdir? got {names:?}"
            );
        }
    }

    /// Stronger guard: assert the function does not rely on any
    /// readdir of the source root at all. We put ONLY non-allowlisted
    /// entries in the fake rootfs. A readdir-based impl would still
    /// return them; a stat-by-name impl returns an empty list.
    #[test]
    fn bind_entries_does_not_read_source_root_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        // ONLY non-allowlisted names present.
        for name in ["rogue", "image-specific", "weird.payload"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let entries = rootfs_bind_entries(root);
        assert!(
            entries.is_empty(),
            "stat-by-name must return empty when no allowlist entry exists; \
             a non-empty result here means readdir snuck back in. got {entries:?}"
        );
    }

    /// Runtime escape hatch: an image with a bespoke WorkingDir (e.g.
    /// `/service/current` from an s6-style image, or something an
    /// OCI config points at that we don't know about) gets picked up
    /// via `III_WORKER_WORKDIR`. Makes the static allowlist safe by
    /// default without being a wall.
    #[test]
    fn bind_entries_picks_up_workdir_env_when_allowlist_misses() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("bin")).unwrap();
        fs::create_dir(root.join("custom-payload")).unwrap();

        let entries = with_workdir_env(Some("/custom-payload/inner"), || rootfs_bind_entries(root));
        let names = names_of(&entries);

        assert!(
            names.contains(&"custom-payload"),
            "workdir-env top-level must be bound even if not in allowlist; got {names:?}"
        );
    }

    /// Env injection must not cause duplicates when the workdir
    /// top-level is already in the allowlist. `/app/dist` + the
    /// allowlist's `app` should yield a single `app` entry.
    #[test]
    fn bind_entries_does_not_duplicate_when_env_overlaps_allowlist() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("app")).unwrap();

        let entries = with_workdir_env(Some("/app/dist"), || rootfs_bind_entries(root));
        let names = names_of(&entries);

        let app_count = names.iter().filter(|n| **n == "app").count();
        assert_eq!(app_count, 1, "app must appear exactly once; got {names:?}");
    }

    /// Env-injected name that doesn't exist in the rootfs gets
    /// dropped. Without this, a mis-configured image would fail the
    /// subsequent `mount()` call and abort boot.
    #[test]
    fn bind_entries_drops_env_workdir_when_absent_from_rootfs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("bin")).unwrap();
        // No "/phantom" in the tempdir.

        let entries = with_workdir_env(Some("/phantom/here"), || rootfs_bind_entries(root));
        let names = names_of(&entries);

        assert!(
            !names.contains(&"phantom"),
            "missing env workdir top-level must be filtered; got {names:?}"
        );
    }
}
