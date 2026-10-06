// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Regression tests for MOT-5084 (audit finding
//! `sec-sandbox-oci-whiteout-symlink-escape`).
//!
//! `extract_layer_with_limits` used to apply an OCI whiteout entry
//! (`.wh.<name>`) by calling `remove_file` / `remove_dir_all` on
//! `dest.join(parent).join(name)` directly. A symlink planted by the same or
//! an earlier layer (`escape -> /some/host/dir`, absolute or `../`) made
//! `escape/.wh.victim` delete `/some/host/dir/victim` on the host,
//! recursively for directories, and a bare `.wh.` deleted its parent or the
//! whole extraction root. `apply_whiteout` (#2273) now refuses such entries.
//!
//! The attack cases assert on what is left on disk, not on whether
//! extraction returns an error, so they keep holding if refused entries ever
//! become hard errors. All paths are inside tempdirs.

use iii_worker::cli::worker_manager::oci::extract_layer_with_limits;
use std::path::Path;

enum E<'a> {
    File(&'a str, &'a [u8]),
    Dir(&'a str),
    Symlink(&'a str, &'a str),
}

fn layer(entries: &[E<'_>]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    {
        let mut b = tar::Builder::new(&mut enc);
        for e in entries {
            match e {
                E::File(p, data) => {
                    let mut h = tar::Header::new_gnu();
                    h.set_size(data.len() as u64);
                    h.set_mode(0o644);
                    h.set_entry_type(tar::EntryType::Regular);
                    h.set_cksum();
                    b.append_data(&mut h, p, *data).unwrap();
                }
                E::Dir(p) => {
                    let mut h = tar::Header::new_gnu();
                    h.set_size(0);
                    h.set_mode(0o755);
                    h.set_entry_type(tar::EntryType::Directory);
                    h.set_cksum();
                    b.append_data(&mut h, p, std::io::empty()).unwrap();
                }
                E::Symlink(p, target) => {
                    let mut h = tar::Header::new_gnu();
                    h.set_size(0);
                    h.set_mode(0o777);
                    h.set_entry_type(tar::EntryType::Symlink);
                    b.append_link(&mut h, p, target).unwrap();
                }
            }
        }
        b.finish().unwrap();
    }
    enc.finish().unwrap()
}

fn extract_all(layers: &[Vec<u8>], dest: &Path) -> Vec<Result<(), String>> {
    let mut total = 0u64;
    let n = layers.len();
    layers
        .iter()
        .enumerate()
        .map(|(i, l)| {
            extract_layer_with_limits(l, dest, i, n, &mut total).map_err(|e| format!("{e:#}"))
        })
        .collect()
}

/// Host victim tree OUTSIDE the rootfs: victim/secret.txt, victim/keep/data.txt
fn make_victim(root: &Path) -> std::path::PathBuf {
    let victim = root.join("host-victim");
    std::fs::create_dir_all(victim.join("keep")).unwrap();
    std::fs::write(victim.join("secret.txt"), b"host secret").unwrap();
    std::fs::write(victim.join("keep/data.txt"), b"host data").unwrap();
    victim
}

/// Layer 1 plants `escape -> <abs host dir>`; layer 2 whiteouts
/// `escape/secret.txt`. The host file must survive.
#[test]
fn whiteout_through_symlink_from_lower_layer_must_not_delete_host_file() {
    let tmp = tempfile::tempdir().unwrap();
    let victim = make_victim(tmp.path());
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();

    let l1 = layer(&[E::Symlink("escape", victim.to_str().unwrap())]);
    let l2 = layer(&[E::File("escape/.wh.secret.txt", b"")]);
    let res = extract_all(&[l1, l2], &rootfs);
    eprintln!("extract results: {res:?}");

    assert!(
        victim.join("secret.txt").exists(),
        "HOST FILE DELETED: whiteout `escape/.wh.secret.txt` followed the \
         `escape -> {}` symlink out of the rootfs and removed {}",
        victim.display(),
        victim.join("secret.txt").display()
    );
}

/// Same, but a whole host DIRECTORY is removed recursively via
/// `remove_dir_all`.
#[test]
fn whiteout_through_symlink_must_not_recursively_delete_host_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let victim = make_victim(tmp.path());
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();

    // Single layer: symlink first, then the whiteout that walks through it.
    let l1 = layer(&[
        E::Symlink("escape", victim.to_str().unwrap()),
        E::File("escape/.wh.keep", b""),
    ]);
    let res = extract_all(&[l1], &rootfs);
    eprintln!("extract results: {res:?}");

    assert!(
        victim.join("keep/data.txt").exists(),
        "HOST DIRECTORY DELETED: whiteout `escape/.wh.keep` ran remove_dir_all \
         on {} through the symlink",
        victim.join("keep").display()
    );
}

/// Relative symlink variant (`escape -> ../host-victim`): the tar crate
/// happily creates relative symlinks pointing outside `dest`, and the
/// whiteout follows them just the same.
#[test]
fn whiteout_through_relative_symlink_must_not_delete_host_file() {
    let tmp = tempfile::tempdir().unwrap();
    let victim = make_victim(tmp.path());
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();

    let l1 = layer(&[
        E::Symlink("escape", "../host-victim"),
        E::File("escape/.wh.secret.txt", b""),
    ]);
    let res = extract_all(&[l1], &rootfs);
    eprintln!("extract results: {res:?}");

    assert!(
        victim.join("secret.txt").exists(),
        "HOST FILE DELETED via relative symlink `escape -> ../host-victim`"
    );
}

/// A bare `.wh.` entry (empty whiteout target) resolves to the PARENT
/// directory itself: `etc/.wh.` wipes the whole `etc/` of lower layers.
/// Not a valid OCI whiteout; it must be ignored or rejected, never
/// treated as "delete my parent".
#[test]
fn bare_wh_entry_must_not_delete_its_parent_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();

    let l1 = layer(&[
        E::Dir("etc"),
        E::File("etc/passwd", b"root:x:0:0::/root:/bin/sh\n"),
    ]);
    let l2 = layer(&[E::File("etc/.wh.", b"")]);
    let res = extract_all(&[l1, l2], &rootfs);
    eprintln!("extract results: {res:?}");

    assert!(
        rootfs.join("etc/passwd").exists(),
        "bare `etc/.wh.` entry removed the whole etc/ directory"
    );
}

/// A top-level bare `.wh.` resolves to `dest` itself: remove_dir_all(dest)
/// wipes the entire extraction root (all previously extracted layers).
#[test]
fn top_level_bare_wh_entry_must_not_delete_extraction_root() {
    let tmp = tempfile::tempdir().unwrap();
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();

    let l1 = layer(&[E::Dir("bin"), E::File("bin/sh", b"#!")]);
    let l2 = layer(&[E::File(".wh.", b"")]);
    let res = extract_all(&[l1, l2], &rootfs);
    eprintln!("extract results: {res:?}");

    assert!(
        rootfs.join("bin/sh").exists(),
        "top-level `.wh.` entry ran remove_dir_all on the extraction root"
    );
}

/// Sanity: a legitimate in-rootfs whiteout still works (and the tests above
/// are not failing for an unrelated reason).
#[test]
fn sanity_legit_whiteout_removes_lower_file() {
    let tmp = tempfile::tempdir().unwrap();
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();
    let l1 = layer(&[E::Dir("etc"), E::File("etc/motd", b"hi")]);
    let l2 = layer(&[E::File("etc/.wh.motd", b"")]);
    let res = extract_all(&[l1, l2], &rootfs);
    assert!(res.iter().all(|r| r.is_ok()), "{res:?}");
    assert!(!rootfs.join("etc/motd").exists());
    assert!(!rootfs.join("etc/.wh.motd").exists());
}

/// A symlinked component anywhere in the parent path stops the whiteout, not
/// only the first one.
#[test]
fn whiteout_through_intermediate_symlink_must_not_delete_host_file() {
    let tmp = tempfile::tempdir().unwrap();
    let victim = make_victim(tmp.path());
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();

    let l1 = layer(&[
        E::Symlink("escape", victim.to_str().unwrap()),
        E::File("escape/keep/.wh.data.txt", b""),
    ]);
    let res = extract_all(&[l1], &rootfs);
    eprintln!("extract results: {res:?}");

    assert!(
        victim.join("keep/data.txt").exists(),
        "HOST FILE DELETED through the intermediate symlink `escape/keep`"
    );
}

/// Whiting out a directory removes it without following symlinks inside it.
#[test]
fn whiteout_of_directory_must_not_follow_symlinks_inside_it() {
    let tmp = tempfile::tempdir().unwrap();
    let victim = make_victim(tmp.path());
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();

    let l1 = layer(&[
        E::Dir("opt"),
        E::File("opt/file", b"x"),
        E::Symlink("opt/out", victim.to_str().unwrap()),
    ]);
    let l2 = layer(&[E::File(".wh.opt", b"")]);
    let res = extract_all(&[l1, l2], &rootfs);
    assert!(res.iter().all(|r| r.is_ok()), "{res:?}");

    assert!(
        !rootfs.join("opt").exists(),
        "legit whiteout of opt/ not applied"
    );
    assert!(victim.join("secret.txt").exists());
    assert!(victim.join("keep/data.txt").exists());
}

/// Whiting out a hard link only drops that name; the other one keeps the data.
#[test]
fn whiteout_of_hardlink_must_not_delete_host_file() {
    let tmp = tempfile::tempdir().unwrap();
    let victim = make_victim(tmp.path());
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();
    std::fs::hard_link(victim.join("secret.txt"), rootfs.join("h")).unwrap();

    let res = extract_all(&[layer(&[E::File(".wh.h", b"")])], &rootfs);
    assert!(res.iter().all(|r| r.is_ok()), "{res:?}");

    assert!(!rootfs.join("h").exists());
    assert_eq!(
        std::fs::read(victim.join("secret.txt")).unwrap(),
        b"host secret"
    );
}

/// `.wh..` and `.wh...` name the directory itself and its parent: at the top
/// level, the extraction root and the directory that holds it.
#[test]
fn dot_whiteout_targets_must_not_delete_anything() {
    let tmp = tempfile::tempdir().unwrap();
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();
    std::fs::write(tmp.path().join("sibling"), b"keep").unwrap();

    let l1 = layer(&[E::Dir("etc"), E::File("etc/passwd", b"root\n")]);
    let l2 = layer(&[
        E::File(".wh..", b""),
        E::File(".wh...", b""),
        E::File("etc/.wh..", b""),
        E::File("etc/.wh...", b""),
    ]);
    let res = extract_all(&[l1, l2], &rootfs);
    eprintln!("extract results: {res:?}");

    assert!(rootfs.join("etc/passwd").exists());
    assert!(tmp.path().join("sibling").exists());
}

/// Missing parents, parents that are regular files and symlink loops leave
/// the rootfs untouched.
#[test]
fn whiteout_with_unusable_parent_must_not_delete_anything() {
    let tmp = tempfile::tempdir().unwrap();
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();

    let l1 = layer(&[
        E::File("etc", b"not a dir"),
        E::Symlink("a", "b"),
        E::Symlink("b", "a"),
    ]);
    let l2 = layer(&[
        E::File("missing/.wh.x", b""),
        E::File("etc/.wh.x", b""),
        E::File("etc/sub/.wh.x", b""),
        E::File("a/.wh.x", b""),
    ]);
    let res = extract_all(&[l1, l2], &rootfs);
    eprintln!("extract results: {res:?}");

    assert_eq!(std::fs::read(rootfs.join("etc")).unwrap(), b"not a dir");
    assert!(std::fs::symlink_metadata(rootfs.join("a")).is_ok());
}

/// A parent the extractor cannot search is skipped and nothing below it is
/// removed. Root bypasses search permission, so the case only runs when the
/// permission is enforced.
#[test]
fn whiteout_under_unsearchable_parent_must_not_delete_anything() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();
    let res = extract_all(&[layer(&[E::File("locked/inner/file", b"x")])], &rootfs);
    assert!(res.iter().all(|r| r.is_ok()), "{res:?}");

    let locked = rootfs.join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let enforced = std::fs::metadata(locked.join("inner")).is_err();
    let res =
        enforced.then(|| extract_all(&[layer(&[E::File("locked/inner/.wh.file", b"")])], &rootfs));
    // Restore first so the tempdir can always be cleaned up.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    match res {
        Some(res) => {
            eprintln!("extract results: {res:?}");
            assert!(locked.join("inner/file").exists());
        }
        None => eprintln!("skipped: search permission not enforced (running as root?)"),
    }
}
