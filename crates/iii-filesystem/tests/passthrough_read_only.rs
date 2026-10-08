//! `PassthroughFsBuilder::read_only(true)`, the mode the sandbox daemon
//! boots its per-sandbox trampoline root with
//! (`__vm-boot --rootfs-readonly`).
//!
//! Every mutating FUSE op a guest can send must fail with Linux `EROFS` and
//! leave the host tree unchanged; reads keep working. The default share stays
//! writable, so managed workers (which do not opt in) are unaffected.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use iii_filesystem::{
    Context, DynFileSystem, Extensions, FsOptions, PassthroughFs, SetattrValid, ZeroCopyReader,
    ZeroCopyWriter, stat64,
};

const ROOT_ID: u64 = 1;
const LINUX_EROFS: i32 = 30;
// Guest (Linux) open flags, as they arrive over virtio-fs.
const LINUX_O_RDONLY: u32 = 0;
const LINUX_O_WRONLY: u32 = 0o1;
const LINUX_O_RDWR: u32 = 0o2;
const LINUX_O_TRUNC: u32 = 0o1000;

/// Guest write payload (what a FUSE WRITE carries).
struct Payload(Vec<u8>);
impl ZeroCopyReader for Payload {
    fn read_to(&mut self, f: &File, count: usize, off: u64) -> io::Result<usize> {
        let n = count.min(self.0.len());
        f.write_at(&self.0[..n], off)
    }
}

/// Collects what a FUSE READ returns to the guest.
struct Sink(Vec<u8>);
impl ZeroCopyWriter for Sink {
    fn write_from(&mut self, f: &File, count: usize, off: u64) -> io::Result<usize> {
        let mut buf = vec![0u8; count];
        let n = f.read_at(&mut buf, off)?;
        self.0.extend_from_slice(&buf[..n]);
        Ok(n)
    }
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn root_ctx() -> Context {
    Context {
        uid: 0,
        gid: 0,
        pid: 1,
    }
}

fn share(root: &Path, read_only: bool) -> PassthroughFs {
    let fs = PassthroughFs::builder()
        .root_dir(root)
        .read_only(read_only)
        .build()
        .unwrap();
    fs.init(FsOptions::empty()).unwrap();
    fs
}

fn lookup_path(fs: &PassthroughFs, path: &str) -> io::Result<u64> {
    let mut ino = ROOT_ID;
    for comp in path.split('/').filter(|c| !c.is_empty()) {
        ino = fs.lookup(root_ctx(), ino, &cstr(comp))?.inode;
    }
    Ok(ino)
}

fn fake_tree(dir: &Path) {
    std::fs::create_dir_all(dir.join("usr/bin")).unwrap();
    std::fs::create_dir_all(dir.join("etc/empty.d")).unwrap();
    std::fs::write(dir.join("usr/bin/tool"), b"ORIGINAL").unwrap();
    std::fs::set_permissions(
        dir.join("usr/bin/tool"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(dir.join("etc/passwd"), b"root:x:0:0::/root:/bin/sh\n").unwrap();
}

/// Relative path -> (kind, mode, content) for every entry under `dir`.
fn snapshot(dir: &Path) -> BTreeMap<String, (&'static str, u32, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, (&'static str, u32, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let rel = path.strip_prefix(base).unwrap().display().to_string();
            let mode = meta.permissions().mode();
            if meta.file_type().is_symlink() {
                let target = std::fs::read_link(&path).unwrap();
                out.insert(
                    rel,
                    ("symlink", mode, target.display().to_string().into_bytes()),
                );
            } else if meta.is_dir() {
                out.insert(rel, ("dir", mode, Vec::new()));
                walk(base, &path, out);
            } else {
                out.insert(rel, ("file", mode, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn assert_erofs<T>(op: &str, r: io::Result<T>) {
    match r {
        Err(e) => assert_eq!(
            e.raw_os_error(),
            Some(LINUX_EROFS),
            "{op}: expected EROFS, got {e:?}"
        ),
        Ok(_) => panic!("{op}: expected EROFS, got Ok"),
    }
}

#[test]
fn read_only_share_rejects_every_mutation_and_host_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    fake_tree(dir.path());
    let before = snapshot(dir.path());
    let fs = share(dir.path(), true);
    let ctx = root_ctx();
    let tool = lookup_path(&fs, "usr/bin/tool").unwrap();
    let bin = lookup_path(&fs, "usr/bin").unwrap();
    let etc = lookup_path(&fs, "etc").unwrap();

    assert_erofs("open O_WRONLY", fs.open(ctx, tool, false, LINUX_O_WRONLY));
    assert_erofs("open O_RDWR", fs.open(ctx, tool, false, LINUX_O_RDWR));
    assert_erofs(
        "open O_RDONLY|O_TRUNC",
        fs.open(ctx, tool, false, LINUX_O_RDONLY | LINUX_O_TRUNC),
    );
    assert_erofs(
        "create",
        fs.create(
            ctx,
            etc,
            &cstr("new.sh"),
            0o755,
            false,
            LINUX_O_RDWR,
            0,
            Extensions::default(),
        ),
    );
    assert_erofs(
        "mkdir",
        fs.mkdir(ctx, etc, &cstr("newdir"), 0o755, 0, Extensions::default()),
    );
    assert_erofs("unlink", fs.unlink(ctx, etc, &cstr("passwd")));
    assert_erofs("rmdir", fs.rmdir(ctx, etc, &cstr("empty.d")));
    assert_erofs(
        "rename",
        fs.rename(ctx, bin, &cstr("tool"), etc, &cstr("tool"), 0),
    );
    assert_erofs(
        "symlink",
        fs.symlink(
            ctx,
            &cstr("/usr/bin/tool"),
            etc,
            &cstr("link"),
            Extensions::default(),
        ),
    );
    assert_erofs("link", fs.link(ctx, tool, etc, &cstr("hardlink")));

    // WRITE on a handle the guest legitimately opened read-only.
    let (h, _) = fs.open(ctx, tool, false, LINUX_O_RDONLY).unwrap();
    let h = h.expect("read-only open returns a handle");
    assert_erofs(
        "write via read-only handle",
        fs.write(
            ctx,
            tool,
            h,
            &mut Payload(b"evil".to_vec()),
            4,
            0,
            None,
            false,
            false,
            0,
        ),
    );
    fs.release(ctx, tool, 0, h, false, false, None).unwrap();

    let mut attr: stat64 = unsafe { std::mem::zeroed() };
    attr.st_mode = 0o777;
    assert_erofs(
        "setattr chmod",
        fs.setattr(ctx, tool, attr, None, SetattrValid::MODE),
    );
    attr.st_size = 0;
    assert_erofs(
        "setattr truncate",
        fs.setattr(ctx, tool, attr, None, SetattrValid::SIZE),
    );

    assert_eq!(
        snapshot(dir.path()),
        before,
        "a read-only share changed the host tree"
    );
}

#[test]
fn read_only_share_still_serves_reads() {
    let dir = tempfile::tempdir().unwrap();
    fake_tree(dir.path());
    let fs = share(dir.path(), true);
    let ctx = root_ctx();

    let tool = lookup_path(&fs, "usr/bin/tool").unwrap();
    let (st, _) = fs.getattr(ctx, tool, None).unwrap();
    assert_eq!(st.st_size, 8);
    let (h, _) = fs.open(ctx, tool, false, LINUX_O_RDONLY).unwrap();
    let h = h.expect("read-only open returns a handle");
    let mut sink = Sink(Vec::new());
    let n = fs.read(ctx, tool, h, &mut sink, 64, 0, None, 0).unwrap();
    assert_eq!(n, 8);
    assert_eq!(sink.0, b"ORIGINAL");
    fs.release(ctx, tool, 0, h, false, false, None).unwrap();
}

#[test]
fn read_only_share_never_negotiates_writeback() {
    let dir = tempfile::tempdir().unwrap();
    fake_tree(dir.path());
    let negotiated = |read_only: bool| {
        PassthroughFs::builder()
            .root_dir(dir.path())
            .writeback(true)
            .read_only(read_only)
            .build()
            .unwrap()
            .init(FsOptions::WRITEBACK_CACHE)
            .unwrap()
    };
    assert!(
        negotiated(false).contains(FsOptions::WRITEBACK_CACHE),
        "control: a writable share with writeback(true) negotiates it"
    );
    assert!(
        !negotiated(true).contains(FsOptions::WRITEBACK_CACHE),
        "a read-only share must not negotiate the writeback cache"
    );
}

#[test]
fn default_share_stays_writable() {
    let dir = tempfile::tempdir().unwrap();
    fake_tree(dir.path());
    let fs = share(dir.path(), false);
    let etc = lookup_path(&fs, "etc").unwrap();

    fs.unlink(root_ctx(), etc, &cstr("passwd")).unwrap();
    assert!(
        !dir.path().join("etc/passwd").exists(),
        "the default (writable) share must keep working for managed workers"
    );
}
