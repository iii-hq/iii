//! Integration tests for the `sandbox::fs::*` trigger surface.
//!
//! Mirrors the workflows in `tmp/try-vm-exec/fs-example.mjs`:
//! mkdir → write → stat → ls → grep → sed → chmod → mv → read → rm,
//! plus a streaming write/read round-trip and the cross-handler
//! invariants every `sandbox::fs::*` trigger shares (UUID validation,
//! missing sandbox, stopped sandbox, wire-error propagation, and the
//! contract that fs ops do not gate on `exec_in_progress`).
//!
//! All tests run against a `FakeFsRunner` that backs an in-memory
//! filesystem (no libkrun, no shell socket). The unit tests in each
//! `crates/iii-worker/src/sandbox_daemon/fs/*.rs` cover the per-handler
//! happy path with a canned `FsResult`; this file exercises the
//! cross-handler glue that they cannot.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use uuid::Uuid;

use iii_shell_proto::{FsEntry, FsMatch, FsOp, FsReadMeta, FsResult, FsSedFileResult};
use iii_worker::sandbox_daemon::SandboxError;
use iii_worker::sandbox_daemon::fs::adapter::FsRunner;
use iii_worker::sandbox_daemon::fs::chmod::{ChmodRequest, handle_chmod};
use iii_worker::sandbox_daemon::fs::grep::{GrepRequest, handle_grep};
use iii_worker::sandbox_daemon::fs::ls::{LsRequest, handle_ls};
use iii_worker::sandbox_daemon::fs::mkdir::{MkdirRequest, handle_mkdir};
use iii_worker::sandbox_daemon::fs::mv::{MvRequest, handle_mv};
use iii_worker::sandbox_daemon::fs::rm::{RmRequest, handle_rm};
use iii_worker::sandbox_daemon::fs::sed::{SedRequest, handle_sed};
use iii_worker::sandbox_daemon::fs::stat::{StatRequest, handle_stat};
use iii_worker::sandbox_daemon::fs::write::handle_write_with_reader;
use iii_worker::sandbox_daemon::registry::{SandboxRegistry, SandboxState};

// ────────────────────────────────────────────────────────────────────
// In-memory filesystem
// ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct FakeFile {
    bytes: Vec<u8>,
    mode: String,
    mtime: i64,
}

#[derive(Debug, Clone)]
struct FakeDir {
    mode: String,
    mtime: i64,
}

/// Minimal POSIX-shaped filesystem. Paths are absolute strings keyed
/// verbatim; `parent_of`/`basename` use the last `/` as the separator.
/// Symlinks are not modeled — every entry has `is_symlink: false`.
#[derive(Debug, Default)]
struct FakeFs {
    files: BTreeMap<String, FakeFile>,
    dirs: BTreeMap<String, FakeDir>,
}

impl FakeFs {
    fn new() -> Self {
        let mut fs = Self::default();
        // Pre-populate the root and /tmp so callers can mkdir under them
        // without first calling mkdir("/", ...).
        fs.dirs.insert(
            "/".into(),
            FakeDir {
                mode: "0755".into(),
                mtime: 0,
            },
        );
        fs.dirs.insert(
            "/tmp".into(),
            FakeDir {
                mode: "0755".into(),
                mtime: 0,
            },
        );
        fs
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path) || self.dirs.contains_key(path)
    }

    fn entry(&self, path: &str, name: &str) -> Option<FsEntry> {
        if let Some(f) = self.files.get(path) {
            return Some(FsEntry {
                name: name.into(),
                is_dir: false,
                size: f.bytes.len() as u64,
                mode: f.mode.clone(),
                mtime: f.mtime,
                is_symlink: false,
            });
        }
        if let Some(d) = self.dirs.get(path) {
            return Some(FsEntry {
                name: name.into(),
                is_dir: true,
                size: 0,
                mode: d.mode.clone(),
                mtime: d.mtime,
                is_symlink: false,
            });
        }
        None
    }

    fn ls(&self, path: &str) -> Result<FsResult, SandboxError> {
        if !self.dirs.contains_key(path) {
            return Err(if self.files.contains_key(path) {
                SandboxError::FsWrongType { path: path.into() }
            } else {
                SandboxError::FsNotFound { path: path.into() }
            });
        }
        let prefix = if path == "/" {
            "/".to_string()
        } else {
            format!("{path}/")
        };
        let mut entries = Vec::new();
        let candidates = self
            .files
            .keys()
            .chain(self.dirs.keys().filter(|k| k.as_str() != path));
        for k in candidates {
            if let Some(rest) = k.strip_prefix(&prefix) {
                if !rest.is_empty() && !rest.contains('/') {
                    if let Some(e) = self.entry(k, rest) {
                        entries.push(e);
                    }
                }
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(FsResult::Ls { entries })
    }

    fn stat(&self, path: &str) -> Result<FsResult, SandboxError> {
        let name = basename(path).to_string();
        match self.entry(path, &name) {
            Some(e) => Ok(FsResult::Stat(e)),
            None => Err(SandboxError::FsNotFound { path: path.into() }),
        }
    }

    fn mkdir(&mut self, path: &str, mode: String, parents: bool) -> Result<FsResult, SandboxError> {
        if self.exists(path) {
            // `mkdir -p` is idempotent on existing directories.
            if parents && self.dirs.contains_key(path) {
                return Ok(FsResult::Mkdir { created: false });
            }
            return Err(SandboxError::FsAlreadyExists { path: path.into() });
        }
        let parent = parent_of(path);
        if let Some(p) = &parent {
            if !self.dirs.contains_key(p) {
                if !parents {
                    return Err(SandboxError::FsNotFound { path: p.clone() });
                }
                self.mkdir(p, mode.clone(), true)?;
            }
        }
        self.dirs.insert(
            path.into(),
            FakeDir {
                mode,
                mtime: now_seconds(),
            },
        );
        Ok(FsResult::Mkdir { created: true })
    }

    fn rm(&mut self, path: &str, recursive: bool) -> Result<FsResult, SandboxError> {
        if self.files.remove(path).is_some() {
            return Ok(FsResult::Rm { removed: true });
        }
        if !self.dirs.contains_key(path) {
            return Err(SandboxError::FsNotFound { path: path.into() });
        }
        let prefix = format!("{path}/");
        let has_children = self
            .files
            .keys()
            .chain(self.dirs.keys())
            .any(|k| k.starts_with(&prefix));
        if has_children && !recursive {
            return Err(SandboxError::FsNotEmpty { path: path.into() });
        }
        if recursive {
            self.files.retain(|k, _| !k.starts_with(&prefix));
            self.dirs
                .retain(|k, _| !k.starts_with(&prefix) && k != path);
        } else {
            self.dirs.remove(path);
        }
        Ok(FsResult::Rm { removed: true })
    }

    fn chmod(
        &mut self,
        path: &str,
        mode: String,
        recursive: bool,
    ) -> Result<FsResult, SandboxError> {
        if !self.exists(path) {
            return Err(SandboxError::FsNotFound { path: path.into() });
        }
        let mut updated: u64 = 0;
        if let Some(f) = self.files.get_mut(path) {
            f.mode = mode.clone();
            updated += 1;
        }
        if let Some(d) = self.dirs.get_mut(path) {
            d.mode = mode.clone();
            updated += 1;
        }
        if recursive {
            let prefix = format!("{path}/");
            for (_, f) in self
                .files
                .iter_mut()
                .filter(|(k, _)| k.starts_with(&prefix))
            {
                f.mode = mode.clone();
                updated += 1;
            }
            for (_, d) in self.dirs.iter_mut().filter(|(k, _)| k.starts_with(&prefix)) {
                d.mode = mode.clone();
                updated += 1;
            }
        }
        Ok(FsResult::Chmod { updated })
    }

    fn mv(&mut self, src: &str, dst: &str, overwrite: bool) -> Result<FsResult, SandboxError> {
        if !self.exists(src) {
            return Err(SandboxError::FsNotFound { path: src.into() });
        }
        if self.exists(dst) && !overwrite {
            return Err(SandboxError::FsAlreadyExists { path: dst.into() });
        }
        if let Some(p) = parent_of(dst) {
            if !self.dirs.contains_key(&p) {
                return Err(SandboxError::FsNotFound { path: p });
            }
        }
        if let Some(f) = self.files.remove(src) {
            self.files.insert(dst.into(), f);
        } else if let Some(d) = self.dirs.remove(src) {
            self.dirs.insert(dst.into(), d);
        }
        Ok(FsResult::Mv { moved: true })
    }

    fn grep(
        &self,
        path: &str,
        pattern: &str,
        recursive: bool,
        ignore_case: bool,
        max_matches: u64,
        max_line_bytes: u64,
    ) -> Result<FsResult, SandboxError> {
        let needle: String = if ignore_case {
            pattern.to_lowercase()
        } else {
            pattern.into()
        };
        let prefix = if path == "/" {
            "/".into()
        } else {
            format!("{path}/")
        };
        let mut matches: Vec<FsMatch> = Vec::new();
        let mut truncated = false;
        for (key, file) in self.files.iter() {
            let candidate = if key == path {
                true
            } else if key.starts_with(&prefix) {
                if recursive {
                    true
                } else {
                    !key[prefix.len()..].contains('/')
                }
            } else {
                false
            };
            if !candidate {
                continue;
            }
            let text = String::from_utf8_lossy(&file.bytes);
            for (idx, line) in text.lines().enumerate() {
                let hay = if ignore_case {
                    line.to_lowercase()
                } else {
                    line.to_string()
                };
                if hay.contains(&needle) {
                    if matches.len() as u64 >= max_matches {
                        truncated = true;
                        break;
                    }
                    let content = truncate_to_bytes(line, max_line_bytes as usize);
                    matches.push(FsMatch {
                        path: key.clone(),
                        line: (idx + 1) as u64,
                        content,
                    });
                }
            }
            if truncated {
                break;
            }
        }
        Ok(FsResult::Grep { matches, truncated })
    }

    fn sed(
        &mut self,
        files: Vec<String>,
        path: Option<String>,
        recursive: bool,
        pattern: &str,
        replacement: &str,
        first_only: bool,
    ) -> Result<FsResult, SandboxError> {
        // Resolve target file list. Mirrors the handler's "files xor path" guard.
        let targets: Vec<String> = if !files.is_empty() {
            files
        } else if let Some(root) = path {
            if let Some(file) = self.files.get_key_value(root.as_str()) {
                vec![file.0.clone()]
            } else if self.dirs.contains_key(&root) {
                let prefix = format!("{root}/");
                self.files
                    .keys()
                    .filter(|k| {
                        if !k.starts_with(&prefix) {
                            return false;
                        }
                        recursive || !k[prefix.len()..].contains('/')
                    })
                    .cloned()
                    .collect()
            } else {
                return Err(SandboxError::FsNotFound { path: root });
            }
        } else {
            return Err(SandboxError::FsInvalidRequest(
                "sed: must provide exactly one of files or path".into(),
            ));
        };
        let mut total: u64 = 0;
        let mut results = Vec::new();
        for t in targets {
            match self.files.get_mut(&t) {
                None => results.push(FsSedFileResult {
                    path: t.clone(),
                    replacements: 0,
                    success: false,
                    error: Some("file not found".into()),
                }),
                Some(f) => {
                    let text = String::from_utf8_lossy(&f.bytes).into_owned();
                    let (new_text, count) = if first_only {
                        match text.find(pattern) {
                            Some(_) => (text.replacen(pattern, replacement, 1), 1u64),
                            None => (text, 0u64),
                        }
                    } else {
                        let count = text.matches(pattern).count() as u64;
                        (text.replace(pattern, replacement), count)
                    };
                    f.bytes = new_text.into_bytes();
                    f.mtime = now_seconds();
                    total += count;
                    results.push(FsSedFileResult {
                        path: t,
                        replacements: count,
                        success: true,
                        error: None,
                    });
                }
            }
        }
        Ok(FsResult::Sed {
            results,
            total_replacements: total,
        })
    }
}

fn parent_of(path: &str) -> Option<String> {
    if path == "/" {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        None => None,
        Some(0) => Some("/".into()),
        Some(i) => Some(trimmed[..i].into()),
    }
}

fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    match trimmed.rfind('/') {
        Some(i) => &trimmed[i + 1..],
        None => trimmed,
    }
}

fn now_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn truncate_to_bytes(line: &str, max: usize) -> String {
    if line.len() <= max {
        return line.to_string();
    }
    let mut end = max;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &line[..end])
}

// ────────────────────────────────────────────────────────────────────
// FakeFsRunner — dispatches FsOp onto FakeFs, plus error-injection mode
// ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct ErrorInjector {
    errors: HashMap<&'static str, fn() -> SandboxError>,
}

impl ErrorInjector {
    fn take(&self, key: &'static str) -> Option<SandboxError> {
        self.errors.get(key).map(|f| f())
    }
}

struct FakeFsRunner {
    fs: Arc<Mutex<FakeFs>>,
    /// Force a specific FsRunner method to return the configured error
    /// instead of touching state. Used to pin error-propagation paths.
    inject: ErrorInjector,
}

impl FakeFsRunner {
    fn new(fs: Arc<Mutex<FakeFs>>) -> Self {
        Self {
            fs,
            inject: ErrorInjector::default(),
        }
    }

    fn with_error(mut self, key: &'static str, build: fn() -> SandboxError) -> Self {
        self.inject.errors.insert(key, build);
        self
    }
}

#[async_trait::async_trait]
impl FsRunner for FakeFsRunner {
    async fn fs_call(&self, _shell_sock: PathBuf, op: FsOp) -> Result<FsResult, SandboxError> {
        if let Some(err) = self.inject.take("fs_call") {
            return Err(err);
        }
        let mut fs = self.fs.lock().await;
        match op {
            FsOp::Ls { path } => fs.ls(&path),
            FsOp::Stat { path } => fs.stat(&path),
            FsOp::Mkdir {
                path,
                mode,
                parents,
            } => fs.mkdir(&path, mode, parents),
            FsOp::Rm { path, recursive } => fs.rm(&path, recursive),
            FsOp::Chmod {
                path,
                mode,
                recursive,
                ..
            } => fs.chmod(&path, mode, recursive),
            FsOp::Mv {
                src,
                dst,
                overwrite,
            } => fs.mv(&src, &dst, overwrite),
            FsOp::Grep {
                path,
                pattern,
                recursive,
                ignore_case,
                max_matches,
                max_line_bytes,
                ..
            } => fs.grep(
                &path,
                &pattern,
                recursive,
                ignore_case,
                max_matches,
                max_line_bytes,
            ),
            FsOp::Sed {
                files,
                path,
                recursive,
                pattern,
                replacement,
                first_only,
                ..
            } => fs.sed(files, path, recursive, &pattern, &replacement, first_only),
            FsOp::WriteStart { .. } | FsOp::ReadStart { .. } => {
                panic!("WriteStart/ReadStart should reach fs_write_stream / fs_read_stream")
            }
        }
    }

    async fn fs_write_stream(
        &self,
        _shell_sock: PathBuf,
        path: String,
        mode: String,
        parents: bool,
        mut reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
    ) -> Result<FsResult, SandboxError> {
        if let Some(err) = self.inject.take("fs_write_stream") {
            return Err(err);
        }
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .await
            .map_err(|e| SandboxError::FsIo(format!("fake read_to_end: {e}")))?;

        let mut fs = self.fs.lock().await;
        if let Some(parent) = parent_of(&path) {
            if !fs.dirs.contains_key(&parent) {
                if !parents {
                    return Err(SandboxError::FsNotFound { path: parent });
                }
                // `mkdir -p` semantics for the parent chain.
                fs.mkdir(&parent, "0755".into(), true)?;
            }
        }
        let bytes = buf.len() as u64;
        fs.files.insert(
            path.clone(),
            FakeFile {
                bytes: buf,
                mode,
                mtime: now_seconds(),
            },
        );
        Ok(FsResult::Write {
            bytes_written: bytes,
            path,
        })
    }

    async fn fs_read_stream(
        &self,
        _shell_sock: PathBuf,
        path: String,
    ) -> Result<(FsReadMeta, Box<dyn tokio::io::AsyncRead + Unpin + Send>), SandboxError> {
        if let Some(err) = self.inject.take("fs_read_stream") {
            return Err(err);
        }
        let fs = self.fs.lock().await;
        let file = fs
            .files
            .get(&path)
            .ok_or_else(|| SandboxError::FsNotFound { path: path.clone() })?
            .clone();
        let meta = FsReadMeta {
            size: file.bytes.len() as u64,
            mode: file.mode,
            mtime: file.mtime,
        };
        Ok((meta, Box::new(Cursor::new(file.bytes))))
    }
}

// ────────────────────────────────────────────────────────────────────
// Test harness helpers
// ────────────────────────────────────────────────────────────────────

fn fixture_state(id: Uuid) -> SandboxState {
    SandboxState {
        id,
        name: None,
        image: "node".into(),
        rootfs: PathBuf::from("/tmp/r"),
        workdir: PathBuf::from("/tmp/w"),
        shell_sock: PathBuf::from("/tmp/s"),
        vm_pid: Some(1),
        created_at: Instant::now(),
        last_exec_at: Instant::now(),
        exec_in_progress: false,
        idle_timeout_secs: 300,
        stopped: false,
    }
}

async fn live_sandbox(reg: &SandboxRegistry) -> Uuid {
    let id = Uuid::new_v4();
    reg.insert(fixture_state(id)).await;
    id
}

fn new_fs() -> Arc<Mutex<FakeFs>> {
    Arc::new(Mutex::new(FakeFs::new()))
}

// ────────────────────────────────────────────────────────────────────
// Happy-path workflow — mirrors fs-example.mjs end-to-end
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fs_full_workflow_round_trips_through_every_handler() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let fs = new_fs();
    let runner = FakeFsRunner::new(fs.clone());

    // 1. mkdir /tmp/iii-fs-demo with parents
    let mk = handle_mkdir(
        MkdirRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo".into(),
            mode: "0755".into(),
            parents: true,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert!(mk.created);

    // 2. write hello.txt via streaming reader
    let payload = b"hello from outside the sandbox\nsecond line: TODO(demo): replace me\nline three\nline four\n".to_vec();
    let write_resp = handle_write_with_reader(
        id.to_string(),
        "/tmp/iii-fs-demo/hello.txt".into(),
        "0644".into(),
        false,
        Box::new(Cursor::new(payload.clone())),
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert_eq!(write_resp.bytes_written, payload.len() as u64);
    assert_eq!(write_resp.path, "/tmp/iii-fs-demo/hello.txt");

    // 3. stat the new file
    let st = handle_stat(
        StatRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo/hello.txt".into(),
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert_eq!(st.name, "hello.txt");
    assert!(!st.is_dir);
    assert_eq!(st.size, payload.len() as u64);
    assert_eq!(st.mode, "0644");

    // 4. ls — only hello.txt is present so far
    let ls = handle_ls(
        LsRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo".into(),
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert_eq!(ls.entries.len(), 1);
    assert_eq!(ls.entries[0].name, "hello.txt");

    // 5. write a second file so grep + sed have multiple targets
    let notes = b"# notes\n- TODO(perf): inline this\n- done: ship\n".to_vec();
    handle_write_with_reader(
        id.to_string(),
        "/tmp/iii-fs-demo/notes.md".into(),
        "0644".into(),
        false,
        Box::new(Cursor::new(notes.clone())),
        &reg,
        &runner,
    )
    .await
    .unwrap();

    // 6. grep recursive across both files for TODO
    let gr = handle_grep(
        GrepRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo".into(),
            pattern: "TODO".into(),
            recursive: true,
            ignore_case: false,
            include_glob: vec![],
            exclude_glob: vec![],
            max_matches: 100,
            max_line_bytes: 1024,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert!(!gr.truncated);
    assert_eq!(gr.matches.len(), 2);
    let mut hit_paths: Vec<_> = gr.matches.iter().map(|m| m.path.as_str()).collect();
    hit_paths.sort();
    assert_eq!(
        hit_paths,
        vec!["/tmp/iii-fs-demo/hello.txt", "/tmp/iii-fs-demo/notes.md",]
    );

    // 7. sed (path form) — replace TODO → DONE in both files
    let sd = handle_sed(
        SedRequest {
            sandbox_id: id.to_string(),
            files: vec![],
            path: Some("/tmp/iii-fs-demo".into()),
            recursive: true,
            include_glob: vec![],
            exclude_glob: vec![],
            pattern: "TODO".into(),
            replacement: "DONE".into(),
            regex: false,
            first_only: false,
            ignore_case: false,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert_eq!(sd.total_replacements, 2);
    assert_eq!(sd.results.len(), 2);
    assert!(sd.results.iter().all(|r| r.success));

    // After sed, grep for TODO should find nothing.
    let after = handle_grep(
        GrepRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo".into(),
            pattern: "TODO".into(),
            recursive: true,
            ignore_case: false,
            include_glob: vec![],
            exclude_glob: vec![],
            max_matches: 100,
            max_line_bytes: 1024,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert!(after.matches.is_empty());

    // 8. chmod — bump hello.txt to 0600 and confirm via stat
    let cm = handle_chmod(
        ChmodRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo/hello.txt".into(),
            mode: "0600".into(),
            uid: None,
            gid: None,
            recursive: false,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert_eq!(cm.updated, 1);
    let st2 = handle_stat(
        StatRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo/hello.txt".into(),
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert_eq!(st2.mode, "0600");

    // 9. mv — rename hello.txt → greetings.txt
    let mvr = handle_mv(
        MvRequest {
            sandbox_id: id.to_string(),
            src: "/tmp/iii-fs-demo/hello.txt".into(),
            dst: "/tmp/iii-fs-demo/greetings.txt".into(),
            overwrite: false,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert!(mvr.moved);
    let ls2 = handle_ls(
        LsRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo".into(),
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    let names: Vec<_> = ls2.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"greetings.txt"));
    assert!(!names.contains(&"hello.txt"));

    // 10. read greetings.txt back via fs_read_stream — the engine-coupled
    //     handle_read path is covered by Phase 6 e2e tests; here we
    //     drive the FsRunner method directly to confirm the bytes
    //     round-trip with the post-sed content.
    let (meta, mut reader) = runner
        .fs_read_stream(
            PathBuf::from("/tmp/s"),
            "/tmp/iii-fs-demo/greetings.txt".into(),
        )
        .await
        .unwrap();
    let mut got = Vec::new();
    reader.read_to_end(&mut got).await.unwrap();
    assert_eq!(meta.size, got.len() as u64);
    let post_sed = String::from_utf8(got).unwrap();
    assert!(post_sed.contains("DONE(demo)"));
    assert!(!post_sed.contains("TODO(demo)"));

    // 11. rm — remove the demo directory recursively
    let rm = handle_rm(
        RmRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo".into(),
            recursive: true,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert!(rm.removed);
    let after_rm = handle_stat(
        StatRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/iii-fs-demo".into(),
        },
        &reg,
        &runner,
    )
    .await
    .unwrap_err();
    assert_eq!(after_rm.code().as_str(), "S211");
}

// ────────────────────────────────────────────────────────────────────
// Cross-handler invariants — bad UUID, missing sandbox, stopped sandbox
// ────────────────────────────────────────────────────────────────────

/// Run `assert_outcome` against every fs handler. Picks one deliberate
/// request per handler so the test only varies the cross-handler
/// precondition under test.
async fn for_each_fs_handler(
    sandbox_id: String,
    reg: &SandboxRegistry,
    runner: &FakeFsRunner,
    mut assert_outcome: impl FnMut(&'static str, Result<(), SandboxError>),
) {
    let sid = sandbox_id;

    macro_rules! check {
        ($name:expr, $call:expr) => {{
            let result: Result<(), SandboxError> = match $call.await {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            };
            assert_outcome($name, result);
        }};
    }

    check!(
        "ls",
        handle_ls(
            LsRequest {
                sandbox_id: sid.clone(),
                path: "/tmp".into(),
            },
            reg,
            runner,
        )
    );
    check!(
        "stat",
        handle_stat(
            StatRequest {
                sandbox_id: sid.clone(),
                path: "/tmp".into(),
            },
            reg,
            runner,
        )
    );
    check!(
        "mkdir",
        handle_mkdir(
            MkdirRequest {
                sandbox_id: sid.clone(),
                path: "/tmp/x".into(),
                mode: "0755".into(),
                parents: true,
            },
            reg,
            runner,
        )
    );
    check!(
        "rm",
        handle_rm(
            RmRequest {
                sandbox_id: sid.clone(),
                path: "/tmp/x".into(),
                recursive: true,
            },
            reg,
            runner,
        )
    );
    check!(
        "chmod",
        handle_chmod(
            ChmodRequest {
                sandbox_id: sid.clone(),
                path: "/tmp".into(),
                mode: "0700".into(),
                uid: None,
                gid: None,
                recursive: false,
            },
            reg,
            runner,
        )
    );
    check!(
        "mv",
        handle_mv(
            MvRequest {
                sandbox_id: sid.clone(),
                src: "/tmp/a".into(),
                dst: "/tmp/b".into(),
                overwrite: false,
            },
            reg,
            runner,
        )
    );
    check!(
        "grep",
        handle_grep(
            GrepRequest {
                sandbox_id: sid.clone(),
                path: "/tmp".into(),
                pattern: "x".into(),
                recursive: false,
                ignore_case: false,
                include_glob: vec![],
                exclude_glob: vec![],
                max_matches: 1,
                max_line_bytes: 1,
            },
            reg,
            runner,
        )
    );
    check!(
        "sed",
        handle_sed(
            SedRequest {
                sandbox_id: sid.clone(),
                files: vec!["/tmp/x".into()],
                path: None,
                recursive: false,
                include_glob: vec![],
                exclude_glob: vec![],
                pattern: "a".into(),
                replacement: "b".into(),
                regex: false,
                first_only: true,
                ignore_case: false,
            },
            reg,
            runner,
        )
    );
    check!(
        "write",
        handle_write_with_reader(
            sid.clone(),
            "/tmp/x".into(),
            "0644".into(),
            false,
            Box::new(Cursor::new(b"x".to_vec())),
            reg,
            runner,
        )
    );
    // read does not have an engine-decoupled handler entry point we can
    // call from an integration test (it allocates an iii_sdk channel).
    // The shared validation block runs the same UUID/registry check as
    // every other handler — covered by the unit tests in read.rs.
}

#[tokio::test]
async fn every_fs_handler_rejects_bad_uuid_with_s001() {
    let reg = SandboxRegistry::new();
    let runner = FakeFsRunner::new(new_fs());
    for_each_fs_handler("not-a-uuid".into(), &reg, &runner, |name, r| match r {
        Err(e) => assert_eq!(
            e.code().as_str(),
            "S001",
            "{name} should map bad UUID to S001 (got {e:?})"
        ),
        Ok(()) => panic!("{name} accepted a malformed UUID"),
    })
    .await;
}

#[tokio::test]
async fn every_fs_handler_rejects_unknown_sandbox_with_s002() {
    let reg = SandboxRegistry::new();
    let runner = FakeFsRunner::new(new_fs());
    let unknown = Uuid::new_v4().to_string();
    for_each_fs_handler(unknown, &reg, &runner, |name, r| match r {
        Err(e) => assert_eq!(
            e.code().as_str(),
            "S002",
            "{name} should map unknown sandbox to S002 (got {e:?})"
        ),
        Ok(()) => panic!("{name} accepted a missing sandbox"),
    })
    .await;
}

#[tokio::test]
async fn every_fs_handler_rejects_stopped_sandbox_with_s004() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    reg.mark_stopped(id).await;
    let runner = FakeFsRunner::new(new_fs());
    for_each_fs_handler(id.to_string(), &reg, &runner, |name, r| match r {
        Err(e) => assert_eq!(
            e.code().as_str(),
            "S004",
            "{name} should map stopped sandbox to S004 (got {e:?})"
        ),
        Ok(()) => panic!("{name} accepted a stopped sandbox"),
    })
    .await;
}

// ────────────────────────────────────────────────────────────────────
// Streaming write/read 1 MiB round-trip
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fs_write_then_read_stream_round_trips_one_mib() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let fs = new_fs();
    let runner = FakeFsRunner::new(fs.clone());

    // Pre-create the parent dir so write doesn't need parents=true.
    handle_mkdir(
        MkdirRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/big".into(),
            mode: "0755".into(),
            parents: true,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap();

    let size: usize = 1024 * 1024; // 1 MiB
    let mut payload = Vec::with_capacity(size);
    for i in 0..size {
        payload.push((i % 251) as u8); // non-trivial pattern, avoids long zero runs
    }

    let resp = handle_write_with_reader(
        id.to_string(),
        "/tmp/big/blob.bin".into(),
        "0644".into(),
        false,
        Box::new(Cursor::new(payload.clone())),
        &reg,
        &runner,
    )
    .await
    .unwrap();
    assert_eq!(resp.bytes_written, size as u64);

    // Read back via the FsRunner trait (engine-decoupled path).
    let (meta, mut reader) = runner
        .fs_read_stream(PathBuf::from("/tmp/s"), "/tmp/big/blob.bin".into())
        .await
        .unwrap();
    assert_eq!(meta.size, size as u64);
    let mut got = Vec::with_capacity(size);
    reader.read_to_end(&mut got).await.unwrap();
    assert_eq!(got.len(), size);
    assert_eq!(got, payload, "round-trip bytes must match");
}

// ────────────────────────────────────────────────────────────────────
// Wire-error propagation — handlers preserve the FsRunner's typed error
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn s211_not_found_propagates_through_handler() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner = FakeFsRunner::new(new_fs()).with_error("fs_call", || SandboxError::FsNotFound {
        path: "/missing".into(),
    });
    let err = handle_stat(
        StatRequest {
            sandbox_id: id.to_string(),
            path: "/missing".into(),
        },
        &reg,
        &runner,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S211");
}

#[tokio::test]
async fn s213_already_exists_propagates_through_handler() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner =
        FakeFsRunner::new(new_fs()).with_error("fs_call", || SandboxError::FsAlreadyExists {
            path: "/tmp/dup".into(),
        });
    let err = handle_mkdir(
        MkdirRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/dup".into(),
            mode: "0755".into(),
            parents: false,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S213");
}

#[tokio::test]
async fn s215_permission_propagates_through_handler() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner = FakeFsRunner::new(new_fs())
        .with_error("fs_call", || SandboxError::FsPermission("EACCES".into()));
    let err = handle_chmod(
        ChmodRequest {
            sandbox_id: id.to_string(),
            path: "/etc/shadow".into(),
            mode: "0644".into(),
            uid: None,
            gid: None,
            recursive: false,
        },
        &reg,
        &runner,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S215");
}

#[tokio::test]
async fn s218_channel_aborted_propagates_through_write_handler() {
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner = FakeFsRunner::new(new_fs()).with_error("fs_write_stream", || {
        SandboxError::FsChannelAborted("client hung up".into())
    });
    let err = handle_write_with_reader(
        id.to_string(),
        "/tmp/aborted.bin".into(),
        "0644".into(),
        false,
        Box::new(Cursor::new(b"never lands".to_vec())),
        &reg,
        &runner,
    )
    .await
    .unwrap_err();
    assert_eq!(err.code().as_str(), "S218");
}

// ────────────────────────────────────────────────────────────────────
// Concurrent fs ops do NOT gate on `exec_in_progress`
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn fs_ops_succeed_while_exec_is_in_progress() {
    // exec serializes on `begin_exec`; fs ops must not. This test pins
    // that contract so a future tightening of the registry gate
    // (e.g. unifying exec + fs serialization) shows up here.
    let reg = SandboxRegistry::new();
    let id = live_sandbox(&reg).await;
    let runner = FakeFsRunner::new(new_fs());

    // Acquire the exec slot — equivalent to an in-flight `sandbox::exec`.
    let _slot = reg.begin_exec(id).await.unwrap();
    let busy = reg.get(id).await.unwrap();
    assert!(busy.exec_in_progress);

    // mkdir + write + ls all proceed while exec is busy.
    handle_mkdir(
        MkdirRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/concurrent".into(),
            mode: "0755".into(),
            parents: true,
        },
        &reg,
        &runner,
    )
    .await
    .expect("mkdir must not block on exec_in_progress");

    handle_write_with_reader(
        id.to_string(),
        "/tmp/concurrent/note.txt".into(),
        "0644".into(),
        false,
        Box::new(Cursor::new(b"concurrent ok".to_vec())),
        &reg,
        &runner,
    )
    .await
    .expect("write must not block on exec_in_progress");

    let ls = handle_ls(
        LsRequest {
            sandbox_id: id.to_string(),
            path: "/tmp/concurrent".into(),
        },
        &reg,
        &runner,
    )
    .await
    .expect("ls must not block on exec_in_progress");
    assert_eq!(ls.entries.len(), 1);

    // The fs ops should have bumped `last_exec_at` (the idle reaper
    // gate) without clearing `exec_in_progress`.
    let after = reg.get(id).await.unwrap();
    assert!(
        after.exec_in_progress,
        "fs ops must not clear exec_in_progress; that belongs to end_exec"
    );
}
