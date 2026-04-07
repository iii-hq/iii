//! Integration tests for iii-filesystem.
//!
//! Tests are split into two groups:
//! 1. Builder/construction tests (existing) — verify PassthroughFs can be built.
//! 2. DynFileSystem I/O tests (new) — exercise real filesystem operations via
//!    the DynFileSystem trait on a PassthroughFs backed by a temp directory.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::time::Duration;

use iii_filesystem::{
    CachePolicy, Context, DynFileSystem, Extensions, FsOptions, PassthroughConfig, PassthroughFs,
    ZeroCopyReader, ZeroCopyWriter,
};

const ROOT_ID: u64 = 1;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

struct FsHarness {
    fs: PassthroughFs,
    _dir: tempfile::TempDir,
}

fn setup_fs() -> FsHarness {
    let dir = tempfile::tempdir().unwrap();
    let fs = PassthroughFs::builder()
        .root_dir(dir.path())
        .cache_policy(CachePolicy::Never)
        .build()
        .unwrap();
    fs.init(FsOptions::empty()).unwrap();
    FsHarness { fs, _dir: dir }
}

fn test_ctx() -> Context {
    Context {
        uid: unsafe { libc::getuid() },
        gid: unsafe { libc::getgid() },
        pid: std::process::id() as libc::pid_t,
    }
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// A ZeroCopyWriter that captures bytes read from a File into an in-memory buffer.
struct TestWriter {
    buf: Vec<u8>,
}

impl TestWriter {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }
}

impl ZeroCopyWriter for TestWriter {
    fn write_from(&mut self, f: &File, count: usize, off: u64) -> io::Result<usize> {
        use std::os::unix::fs::FileExt;
        let mut tmp = vec![0u8; count];
        let n = f.read_at(&mut tmp, off)?;
        self.buf.extend_from_slice(&tmp[..n]);
        Ok(n)
    }
}

/// A ZeroCopyReader that supplies bytes from an in-memory buffer into a File.
struct TestReader {
    data: Vec<u8>,
    pos: usize,
}

impl TestReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}

impl ZeroCopyReader for TestReader {
    fn read_to(&mut self, f: &File, count: usize, off: u64) -> io::Result<usize> {
        use std::os::unix::fs::FileExt;
        let remaining = self.data.len() - self.pos;
        if remaining == 0 {
            return Ok(0);
        }
        let to_write = count.min(remaining);
        let n = f.write_at(&self.data[self.pos..self.pos + to_write], off)?;
        self.pos += n;
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// Builder / construction tests (kept from original)
// ---------------------------------------------------------------------------

#[test]
fn builder_creates_functional_passthrough_fs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

    let result = PassthroughFs::builder()
        .root_dir(dir.path())
        .entry_timeout(Duration::from_secs(10))
        .attr_timeout(Duration::from_secs(10))
        .cache_policy(CachePolicy::Auto)
        .build();

    assert!(result.is_ok());
}

#[test]
fn new_creates_passthrough_fs_with_config() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = PassthroughConfig {
        root_dir: dir.path().to_path_buf(),
        entry_timeout: Duration::from_secs(1),
        attr_timeout: Duration::from_secs(2),
        cache_policy: CachePolicy::Never,
        writeback: true,
    };

    assert!(PassthroughFs::new(cfg).is_ok());
}

#[test]
fn all_cache_policies_construct_successfully() {
    let dir = tempfile::tempdir().unwrap();
    for policy in [CachePolicy::Never, CachePolicy::Auto, CachePolicy::Always] {
        let result = PassthroughFs::builder()
            .root_dir(dir.path())
            .cache_policy(policy)
            .build();
        assert!(result.is_ok(), "Cache policy {:?} should work", policy);
    }
}

#[test]
fn builder_rejects_nonexistent_root() {
    let result = PassthroughFs::builder()
        .root_dir("/nonexistent_path_xyz_12345")
        .build();
    assert!(result.is_err());
}

#[test]
fn builder_rejects_missing_root_dir() {
    let result = PassthroughFs::builder().build();
    assert!(result.is_err());
}

#[test]
fn new_rejects_nonexistent_root() {
    let cfg = PassthroughConfig {
        root_dir: "/nonexistent_dir_abc_67890".into(),
        ..Default::default()
    };
    assert!(PassthroughFs::new(cfg).is_err());
}

#[test]
fn builder_with_writeback_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let result = PassthroughFs::builder()
        .root_dir(dir.path())
        .writeback(true)
        .build();
    assert!(result.is_ok());
}

#[test]
fn passthrough_config_defaults() {
    let cfg = PassthroughConfig::default();
    assert_eq!(cfg.entry_timeout, Duration::from_secs(5));
    assert_eq!(cfg.attr_timeout, Duration::from_secs(5));
    assert_eq!(cfg.cache_policy, CachePolicy::Auto);
    assert!(!cfg.writeback);
}

#[test]
fn builder_full_options() {
    let dir = tempfile::tempdir().unwrap();
    let result = PassthroughFs::builder()
        .root_dir(dir.path())
        .entry_timeout(Duration::from_millis(500))
        .attr_timeout(Duration::from_millis(1000))
        .cache_policy(CachePolicy::Always)
        .writeback(true)
        .build();
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// DynFileSystem I/O operation tests
// ---------------------------------------------------------------------------

#[test]
fn lookup_existing_file() {
    let h = setup_fs();
    std::fs::write(h._dir.path().join("hello.txt"), "content").unwrap();

    let entry =
        h.fs.lookup(test_ctx(), ROOT_ID, &cstr("hello.txt"))
            .unwrap();
    assert_ne!(entry.inode, 0, "looked-up inode should be non-zero");
    assert_ne!(entry.inode, ROOT_ID, "file inode should differ from root");
}

#[test]
fn lookup_nonexistent_returns_error() {
    let h = setup_fs();
    let result = h.fs.lookup(test_ctx(), ROOT_ID, &cstr("no_such_file"));
    assert!(result.is_err());
}

#[test]
fn getattr_root() {
    let h = setup_fs();
    let (st, _dur) = h.fs.getattr(test_ctx(), ROOT_ID, None).unwrap();
    let mode = st.st_mode as u32;
    assert!(
        (mode & libc::S_IFMT as u32) == libc::S_IFDIR as u32,
        "root inode should be a directory, got mode {:#o}",
        mode
    );
}

#[test]
fn getattr_file_size() {
    let h = setup_fs();
    let data = b"hello world";
    std::fs::write(h._dir.path().join("sized.txt"), data).unwrap();

    let entry =
        h.fs.lookup(test_ctx(), ROOT_ID, &cstr("sized.txt"))
            .unwrap();
    let (st, _) = h.fs.getattr(test_ctx(), entry.inode, None).unwrap();
    assert_eq!(st.st_size, data.len() as i64);
}

#[test]
fn mkdir_and_lookup() {
    let h = setup_fs();
    let ctx = test_ctx();

    let dir_entry =
        h.fs.mkdir(
            ctx,
            ROOT_ID,
            &cstr("subdir"),
            0o755,
            0,
            Extensions::default(),
        )
        .unwrap();
    assert_ne!(dir_entry.inode, 0);

    let looked_up = h.fs.lookup(ctx, ROOT_ID, &cstr("subdir")).unwrap();
    assert_eq!(looked_up.inode, dir_entry.inode);

    let (st, _) = h.fs.getattr(ctx, dir_entry.inode, None).unwrap();
    assert!(
        (st.st_mode as u32 & libc::S_IFMT as u32) == libc::S_IFDIR as u32,
        "should be a directory"
    );
}

#[test]
fn create_write_read_roundtrip() {
    let h = setup_fs();
    let ctx = test_ctx();

    let (entry, handle, _opts) =
        h.fs.create(
            ctx,
            ROOT_ID,
            &cstr("data.bin"),
            0o644,
            false,
            libc::O_RDWR as u32,
            0,
            Extensions::default(),
        )
        .unwrap();

    let fh = handle.expect("create should return a file handle");
    let payload = b"integration test payload";

    let mut reader = TestReader::new(payload.to_vec());
    let written =
        h.fs.write(
            ctx,
            entry.inode,
            fh,
            &mut reader,
            payload.len() as u32,
            0,
            None,
            false,
            false,
            0,
        )
        .unwrap();
    assert_eq!(written, payload.len());

    let mut writer = TestWriter::new();
    let read =
        h.fs.read(
            ctx,
            entry.inode,
            fh,
            &mut writer,
            payload.len() as u32,
            0,
            None,
            0,
        )
        .unwrap();
    assert_eq!(read, payload.len());
    assert_eq!(&writer.buf, payload);

    h.fs.release(ctx, entry.inode, 0, fh, false, false, None)
        .unwrap();
}

#[test]
fn unlink_removes_file() {
    let h = setup_fs();
    let ctx = test_ctx();

    std::fs::write(h._dir.path().join("to_delete.txt"), "bye").unwrap();
    let _ = h.fs.lookup(ctx, ROOT_ID, &cstr("to_delete.txt")).unwrap();

    h.fs.unlink(ctx, ROOT_ID, &cstr("to_delete.txt")).unwrap();

    let result = h.fs.lookup(ctx, ROOT_ID, &cstr("to_delete.txt"));
    assert!(result.is_err(), "lookup after unlink should fail");
}

#[test]
fn rmdir_removes_directory() {
    let h = setup_fs();
    let ctx = test_ctx();

    h.fs.mkdir(
        ctx,
        ROOT_ID,
        &cstr("to_remove"),
        0o755,
        0,
        Extensions::default(),
    )
    .unwrap();

    h.fs.rmdir(ctx, ROOT_ID, &cstr("to_remove")).unwrap();

    let result = h.fs.lookup(ctx, ROOT_ID, &cstr("to_remove"));
    assert!(result.is_err(), "lookup after rmdir should fail");
}

#[test]
fn opendir_readdir_readdirplus() {
    let h = setup_fs();
    let ctx = test_ctx();

    let names = ["aaa.txt", "bbb.txt", "ccc.txt"];
    for name in &names {
        std::fs::write(h._dir.path().join(name), name.as_bytes()).unwrap();
        let _ = h.fs.lookup(ctx, ROOT_ID, &cstr(name)).unwrap();
    }

    let (dh, _) = h.fs.opendir(ctx, ROOT_ID, libc::O_RDONLY as u32).unwrap();
    let dir_handle = dh.expect("opendir should return a handle");

    let entries =
        h.fs.readdir(ctx, ROOT_ID, dir_handle, 64 * 1024, 0)
            .unwrap();
    let entry_names: Vec<String> = entries
        .iter()
        .map(|e| String::from_utf8_lossy(e.name).to_string())
        .collect();

    for name in &names {
        assert!(
            entry_names.contains(&name.to_string()),
            "readdir should contain '{}', got {:?}",
            name,
            entry_names
        );
    }
    assert!(entry_names.contains(&".".to_string()));
    assert!(entry_names.contains(&"..".to_string()));

    h.fs.releasedir(ctx, ROOT_ID, 0, dir_handle).unwrap();
}

#[test]
fn rename_file() {
    let h = setup_fs();
    let ctx = test_ctx();

    std::fs::write(h._dir.path().join("old_name.txt"), "data").unwrap();
    let _ = h.fs.lookup(ctx, ROOT_ID, &cstr("old_name.txt")).unwrap();

    h.fs.rename(
        ctx,
        ROOT_ID,
        &cstr("old_name.txt"),
        ROOT_ID,
        &cstr("new_name.txt"),
        0,
    )
    .unwrap();

    let result = h.fs.lookup(ctx, ROOT_ID, &cstr("old_name.txt"));
    assert!(result.is_err(), "old name should no longer exist");

    let entry = h.fs.lookup(ctx, ROOT_ID, &cstr("new_name.txt")).unwrap();
    assert_ne!(entry.inode, 0);
}

#[test]
fn symlink_and_readlink() {
    let h = setup_fs();
    let ctx = test_ctx();

    std::fs::write(h._dir.path().join("target.txt"), "real").unwrap();

    let entry =
        h.fs.symlink(
            ctx,
            &cstr("target.txt"),
            ROOT_ID,
            &cstr("link.txt"),
            Extensions::default(),
        )
        .unwrap();
    assert_ne!(entry.inode, 0);

    let target = h.fs.readlink(ctx, entry.inode).unwrap();
    assert_eq!(target, b"target.txt");
}

#[test]
fn statfs_returns_ok() {
    let h = setup_fs();
    let result = h.fs.statfs(test_ctx(), ROOT_ID);
    assert!(result.is_ok(), "statfs on root should succeed");
}

#[test]
fn open_read_existing_file() {
    let h = setup_fs();
    let ctx = test_ctx();
    let data = b"pre-existing content";

    std::fs::write(h._dir.path().join("existing.txt"), data).unwrap();
    let entry = h.fs.lookup(ctx, ROOT_ID, &cstr("existing.txt")).unwrap();

    let (handle, _opts) =
        h.fs.open(ctx, entry.inode, false, libc::O_RDONLY as u32)
            .unwrap();
    let fh = handle.expect("open should return a handle");

    let mut writer = TestWriter::new();
    let n =
        h.fs.read(
            ctx,
            entry.inode,
            fh,
            &mut writer,
            data.len() as u32,
            0,
            None,
            0,
        )
        .unwrap();
    assert_eq!(n, data.len());
    assert_eq!(&writer.buf, data);

    h.fs.release(ctx, entry.inode, 0, fh, false, false, None)
        .unwrap();
}
