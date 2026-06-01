// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Unit tests for `fs_handler` ops and streaming handlers.
//!
//! Each test creates an isolated tmpdir via `tempfile::tempdir()`,
//! exercises a handler, and asserts the returned `FsResult` or
//! `FsError`. No shared state between tests.
//!
//! All tests are `#[test]` (synchronous). No tokio.

use super::ops;
use super::streaming;
use iii_shell_proto::{FsResult, ShellMessage};
use std::io::Write;

fn tmpdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn write_file(dir: &std::path::Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, body).unwrap();
}

// ──────────────────────────────────────────────────────────────────────────────
// ls
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn ls_returns_entries_for_dir() {
    let dir = tmpdir();
    std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();

    let res = ops::ls(dir.path().to_string_lossy().into()).unwrap();
    let FsResult::Ls { mut entries } = res else {
        panic!("expected Ls result");
    };
    entries.sort_by(|x, y| x.name.cmp(&y.name));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "a.txt");
    assert!(!entries[0].is_dir);
    assert_eq!(entries[0].size, 5);
    assert_eq!(entries[1].name, "sub");
    assert!(entries[1].is_dir);
}

#[test]
fn ls_on_missing_returns_s211() {
    let err = ops::ls("/nonexistent/path/xyz".into()).unwrap_err();
    assert_eq!(err.code, "S211");
}

#[test]
fn ls_on_file_returns_s212() {
    let dir = tmpdir();
    let f = dir.path().join("a.txt");
    std::fs::write(&f, b"hi").unwrap();
    let err = ops::ls(f.to_string_lossy().into()).unwrap_err();
    assert_eq!(err.code, "S212");
}

// ──────────────────────────────────────────────────────────────────────────────
// stat
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn stat_returns_file_metadata() {
    let dir = tmpdir();
    let f = dir.path().join("a.txt");
    let mut fh = std::fs::File::create(&f).unwrap();
    fh.write_all(b"hello").unwrap();
    drop(fh);

    let res = ops::stat(f.to_string_lossy().into()).unwrap();
    let FsResult::Stat(entry) = res else {
        panic!("expected Stat result");
    };
    assert_eq!(entry.name, "a.txt");
    assert_eq!(entry.size, 5);
    assert!(!entry.is_dir);
    assert!(!entry.is_symlink);
}

#[test]
fn stat_on_symlink_reports_is_symlink() {
    let dir = tmpdir();
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    std::fs::write(&target, b"x").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let res = ops::stat(link.to_string_lossy().into()).unwrap();
    let FsResult::Stat(entry) = res else {
        panic!("expected Stat result");
    };
    assert!(entry.is_symlink, "symlink flag not set");
}

#[test]
fn stat_missing_returns_s211() {
    let err = ops::stat("/no/such/path".into()).unwrap_err();
    assert_eq!(err.code, "S211");
}

// ──────────────────────────────────────────────────────────────────────────────
// mkdir
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn mkdir_creates_with_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmpdir();
    let p = dir.path().join("new");
    ops::mkdir(p.to_string_lossy().into(), "0750".into(), false).unwrap();
    let md = std::fs::metadata(&p).unwrap();
    assert!(md.is_dir());
    assert_eq!(md.permissions().mode() & 0o777, 0o750);
}

#[test]
fn mkdir_parents_true_creates_ancestors() {
    let dir = tmpdir();
    let p = dir.path().join("a/b/c");
    ops::mkdir(p.to_string_lossy().into(), "0755".into(), true).unwrap();
    assert!(p.is_dir());
}

#[test]
fn mkdir_existing_without_parents_returns_s213() {
    let dir = tmpdir();
    let err = ops::mkdir(dir.path().to_string_lossy().into(), "0755".into(), false).unwrap_err();
    assert_eq!(err.code, "S213");
}

#[test]
fn mkdir_existing_with_parents_is_ok() {
    let dir = tmpdir();
    ops::mkdir(dir.path().to_string_lossy().into(), "0755".into(), true).unwrap();
}

#[test]
fn mkdir_bad_mode_returns_s210() {
    let dir = tmpdir();
    let err = ops::mkdir(
        dir.path().join("new").to_string_lossy().into(),
        "not-octal".into(),
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, "S210");
}

// ──────────────────────────────────────────────────────────────────────────────
// rm
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn rm_file() {
    let dir = tmpdir();
    let f = dir.path().join("x");
    std::fs::write(&f, b"y").unwrap();
    ops::rm(f.to_string_lossy().into(), false).unwrap();
    assert!(!f.exists());
}

#[test]
fn rm_empty_dir_without_recursive() {
    let dir = tmpdir();
    let sub = dir.path().join("empty");
    std::fs::create_dir(&sub).unwrap();
    ops::rm(sub.to_string_lossy().into(), false).unwrap();
    assert!(!sub.exists());
}

#[test]
fn rm_non_empty_dir_without_recursive_returns_s214() {
    let dir = tmpdir();
    let sub = dir.path().join("full");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("x"), b"y").unwrap();
    let err = ops::rm(sub.to_string_lossy().into(), false).unwrap_err();
    assert_eq!(err.code, "S214");
    assert!(sub.exists(), "path should survive failed rm");
}

#[test]
fn rm_recursive_removes_tree() {
    let dir = tmpdir();
    let sub = dir.path().join("full");
    std::fs::create_dir_all(sub.join("a/b")).unwrap();
    std::fs::write(sub.join("a/b/c"), b"x").unwrap();
    ops::rm(sub.to_string_lossy().into(), true).unwrap();
    assert!(!sub.exists());
}

// ──────────────────────────────────────────────────────────────────────────────
// chmod
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn chmod_updates_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmpdir();
    let f = dir.path().join("x");
    std::fs::write(&f, b"y").unwrap();
    let res = ops::chmod(f.to_string_lossy().into(), "0600".into(), None, None, false).unwrap();
    let FsResult::Chmod { updated } = res else {
        panic!("expected Chmod result");
    };
    assert_eq!(updated, 1);
    let md = std::fs::metadata(&f).unwrap();
    assert_eq!(md.permissions().mode() & 0o777, 0o600);
}

#[test]
fn chmod_recursive_counts_every_entry() {
    let dir = tmpdir();
    // root + a/ + a/b/ + a/c = 4 entries
    std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
    std::fs::write(dir.path().join("a/c"), b"x").unwrap();
    let res = ops::chmod(
        dir.path().to_string_lossy().into(),
        "0755".into(),
        None,
        None,
        true,
    )
    .unwrap();
    let FsResult::Chmod { updated } = res else {
        panic!("expected Chmod result");
    };
    // root dir + a + a/b + a/c = 4
    assert_eq!(updated, 4);
}

#[test]
fn chmod_missing_returns_s211() {
    let err = ops::chmod("/nonexistent/xyz".into(), "0755".into(), None, None, false).unwrap_err();
    assert_eq!(err.code, "S211");
}

// ──────────────────────────────────────────────────────────────────────────────
// mv
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn mv_same_fs_rename() {
    let dir = tmpdir();
    let src = dir.path().join("a");
    let dst = dir.path().join("b");
    std::fs::write(&src, b"hello").unwrap();
    ops::mv(
        src.to_string_lossy().into(),
        dst.to_string_lossy().into(),
        false,
    )
    .unwrap();
    assert!(!src.exists());
    assert_eq!(std::fs::read(&dst).unwrap(), b"hello");
}

#[test]
fn mv_overwrite_false_on_existing_dst_returns_s213() {
    let dir = tmpdir();
    let src = dir.path().join("a");
    let dst = dir.path().join("b");
    std::fs::write(&src, b"A").unwrap();
    std::fs::write(&dst, b"B").unwrap();
    let err = ops::mv(
        src.to_string_lossy().into(),
        dst.to_string_lossy().into(),
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, "S213");
    assert!(src.exists(), "source should survive failed mv");
    assert_eq!(std::fs::read(&dst).unwrap(), b"B", "dst unchanged");
}

#[test]
fn mv_overwrite_true_replaces_dst() {
    let dir = tmpdir();
    let src = dir.path().join("a");
    let dst = dir.path().join("b");
    std::fs::write(&src, b"A").unwrap();
    std::fs::write(&dst, b"B").unwrap();
    ops::mv(
        src.to_string_lossy().into(),
        dst.to_string_lossy().into(),
        true,
    )
    .unwrap();
    assert!(!src.exists());
    assert_eq!(std::fs::read(&dst).unwrap(), b"A");
}

// ──────────────────────────────────────────────────────────────────────────────
// write streaming
// ──────────────────────────────────────────────────────────────────────────────

fn b64_encode(data: &[u8]) -> String {
    streaming::b64_encode(data)
}

#[test]
fn write_stream_happy_path_atomic() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmpdir();
    let target = dir.path().join("out.bin");
    let (tx, rx) = std::sync::mpsc::sync_channel::<ShellMessage>(8);
    let payload: Vec<u8> = (0u8..=200).collect();

    // Producer: push two chunks then FsEnd.
    let payload_clone = payload.clone();
    std::thread::spawn(move || {
        tx.send(ShellMessage::FsChunk {
            data_b64: b64_encode(&payload_clone[..100]),
        })
        .unwrap();
        tx.send(ShellMessage::FsChunk {
            data_b64: b64_encode(&payload_clone[100..]),
        })
        .unwrap();
        tx.send(ShellMessage::FsEnd).unwrap();
    });

    let res =
        streaming::handle_write_start(target.to_string_lossy().into(), "0600".into(), false, rx)
            .unwrap();
    let FsResult::Write {
        bytes_written,
        path,
    } = res
    else {
        panic!("expected Write result");
    };
    assert_eq!(bytes_written, payload.len() as u64);
    assert_eq!(path, target.to_string_lossy());
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    let md = std::fs::metadata(&target).unwrap();
    assert_eq!(md.permissions().mode() & 0o777, 0o600);
}

#[test]
fn write_stream_early_channel_close_unlinks_temp() {
    let dir = tmpdir();
    let target = dir.path().join("out.bin");
    let (tx, rx) = std::sync::mpsc::sync_channel::<ShellMessage>(4);

    std::thread::spawn(move || {
        tx.send(ShellMessage::FsChunk {
            data_b64: b64_encode(b"partial"),
        })
        .unwrap();
        // Drop tx without sending FsEnd — simulates caller abort.
    });

    let err =
        streaming::handle_write_start(target.to_string_lossy().into(), "0644".into(), false, rx)
            .unwrap_err();
    assert_eq!(err.code, "S218");
    assert!(!target.exists(), "target must not exist");

    // No temp leaks.
    let leaks: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".iii-tmp-"))
        .collect();
    assert!(leaks.is_empty(), "temp file leaked: {leaks:?}");
}

/// REGRESSION: when the host trigger handler stalls (e.g. the
/// caller-side data channel was hard-terminated and the worker's
/// `ChannelReader::next_binary()` hangs), no `FsChunk` or `FsEnd`
/// ever arrives. The dispatcher's per-corr_id sender stays alive
/// in `fs_writes` for the session lifetime, so a blocking `recv()`
/// would wait forever — the temp file would leak and the host's
/// SDK trigger eventually times out with no S-code.
///
/// fs-example.mjs's adversarial section repro:
///   abortCh.writer.stream.write(Buffer.from('partial bytes'))
///   abortCh.writer.stream.destroy(new Error('simulated caller abort'))
///   await iii.trigger('sandbox::fs::write', { content: ref, ... })
///     => Invocation timeout after 30000ms + temp file left behind
///
/// Fix: `handle_write_start` now uses `recv_timeout(idle_timeout)`.
/// On `Timeout` it unlinks the temp and returns S218. The test
/// drives `handle_write_start_with_timeout` with a 100 ms idle
/// timeout and a sender that's held open but silent — so the recv
/// side blocks purely on the timeout (NOT the Disconnected branch
/// that the previous test covers).
#[test]
fn write_stream_idle_timeout_unlinks_temp_and_returns_s218() {
    use std::time::Duration;

    let dir = tmpdir();
    let target = dir.path().join("out.bin");
    let (tx, rx) = std::sync::mpsc::sync_channel::<ShellMessage>(4);

    let started = std::time::Instant::now();
    let err = streaming::handle_write_start_with_timeout(
        target.to_string_lossy().into(),
        "0644".into(),
        false,
        rx,
        Duration::from_millis(100),
    )
    .unwrap_err();
    let elapsed = started.elapsed();

    // Hold tx alive past the recv path so the failure is *Timeout*,
    // not *Disconnected* — that's the new code path under test.
    drop(tx);

    assert_eq!(err.code, "S218", "expected S218, got {err:?}");
    assert!(
        err.message.contains("stalled") || err.message.contains("aborted"),
        "expected timeout-shaped message naming the stall, got {:?}",
        err.message,
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "handler blocked {elapsed:?} — should have timed out near 100ms",
    );
    assert!(!target.exists(), "target must not exist");

    let leaks: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".iii-tmp-"))
        .collect();
    assert!(leaks.is_empty(), "temp leaked on idle timeout: {leaks:?}");
}

#[test]
fn write_stream_parents_true_creates_ancestors() {
    let dir = tmpdir();
    let target = dir.path().join("a/b/c.bin");
    let (tx, rx) = std::sync::mpsc::sync_channel::<ShellMessage>(4);
    std::thread::spawn(move || {
        tx.send(ShellMessage::FsChunk {
            data_b64: b64_encode(b"hi"),
        })
        .unwrap();
        tx.send(ShellMessage::FsEnd).unwrap();
    });
    streaming::handle_write_start(target.to_string_lossy().into(), "0644".into(), true, rx)
        .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"hi");
}

#[test]
fn write_stream_parents_false_missing_parent_returns_s211() {
    let dir = tmpdir();
    let target = dir.path().join("missing/x.bin");
    let (_tx, rx) = std::sync::mpsc::sync_channel::<ShellMessage>(4);
    let err =
        streaming::handle_write_start(target.to_string_lossy().into(), "0644".into(), false, rx)
            .unwrap_err();
    assert_eq!(err.code, "S211");
}

// ──────────────────────────────────────────────────────────────────────────────
// read streaming
// ──────────────────────────────────────────────────────────────────────────────

/// Create a mock Writer that collects encoded frames into a channel.
/// Returns (writer_tx, frame_rx) where the caller can receive Vec<u8>
/// frames and decode them.
fn mock_writer() -> (
    crate::shell_dispatcher::Writer,
    std::sync::mpsc::Receiver<Vec<u8>>,
) {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(256);
    (tx, rx)
}

/// Decode the ShellMessages from a mock writer receiver.
fn collect_messages(rx: std::sync::mpsc::Receiver<Vec<u8>>) -> Vec<ShellMessage> {
    let mut msgs = Vec::new();
    while let Ok(frame_bytes) = rx.try_recv() {
        // frame_bytes is a complete wire frame (4-byte len + corr_id + flags + JSON).
        // Decode it.
        if frame_bytes.len() < 9 {
            continue;
        }
        let body = &frame_bytes[4..]; // skip frame_len u32
        if let Ok((_, _, msg)) = iii_shell_proto::decode_frame_body(body) {
            msgs.push(msg);
        }
    }
    msgs
}

#[test]
fn read_stream_delivers_meta_then_chunks_then_end() {
    let dir = tmpdir();
    let f = dir.path().join("x.bin");
    let payload: Vec<u8> = (0u8..=255).cycle().take(150_000).collect();
    std::fs::write(&f, &payload).unwrap();

    let (writer, frame_rx) = mock_writer();

    // Run read handler in this thread (it's sync).
    streaming::handle_read_start(f.to_string_lossy().into(), &writer, 42).unwrap();
    // Drop writer so frame_rx drains.
    drop(writer);

    let msgs = collect_messages(frame_rx);

    // First message must be FsMeta.
    let first = msgs.first().expect("no messages received");
    let ShellMessage::FsMeta(meta) = first else {
        panic!("expected FsMeta first, got {first:?}");
    };
    assert_eq!(meta.size, payload.len() as u64);

    // Collect all chunk data and find FsEnd.
    let mut collected: Vec<u8> = Vec::new();
    let mut saw_end = false;
    for msg in &msgs[1..] {
        match msg {
            ShellMessage::FsChunk { data_b64 } => {
                use base64::Engine;
                let b = base64::engine::general_purpose::STANDARD
                    .decode(data_b64)
                    .unwrap();
                collected.extend_from_slice(&b);
            }
            ShellMessage::FsEnd => {
                saw_end = true;
                break;
            }
            other => panic!("unexpected frame: {other:?}"),
        }
    }
    assert!(saw_end, "FsEnd never received");
    assert_eq!(collected, payload);
}

#[test]
fn read_stream_missing_file_returns_s211() {
    let (writer, _rx) = mock_writer();
    let err = streaming::handle_read_start("/no/such/file".into(), &writer, 1).unwrap_err();
    assert_eq!(err.code, "S211");
}

#[test]
fn read_stream_on_directory_returns_s212() {
    let dir = tmpdir();
    let (writer, _rx) = mock_writer();
    let err =
        streaming::handle_read_start(dir.path().to_string_lossy().into(), &writer, 2).unwrap_err();
    assert_eq!(err.code, "S212");
}

// ──────────────────────────────────────────────────────────────────────────────
// grep
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn grep_finds_literal_matches() {
    let dir = tmpdir();
    write_file(dir.path(), "a.rs", "fn foo() {}\n// TODO(a): fix\n");
    write_file(dir.path(), "b.rs", "// TODO(b): write\n");

    let res = ops::grep(
        dir.path().to_string_lossy().into(),
        r"TODO\(.+\)".into(),
        true,
        false,
        vec![],
        vec![],
        1000,
        4096,
    )
    .unwrap();
    let FsResult::Grep { matches, truncated } = res else {
        panic!("expected Grep result");
    };
    assert!(!truncated);
    assert_eq!(matches.len(), 2);
    for m in &matches {
        assert!(m.content.contains("TODO("));
        assert!(m.line >= 1);
    }
}

#[test]
fn grep_respects_include_glob() {
    let dir = tmpdir();
    write_file(dir.path(), "a.rs", "hit\n");
    write_file(dir.path(), "a.txt", "hit\n");
    let res = ops::grep(
        dir.path().to_string_lossy().into(),
        "hit".into(),
        true,
        false,
        vec!["*.rs".into()],
        vec![],
        1000,
        4096,
    )
    .unwrap();
    let FsResult::Grep { matches, .. } = res else {
        panic!();
    };
    assert_eq!(matches.len(), 1);
    assert!(matches[0].path.ends_with(".rs"));
}

#[test]
fn grep_truncates_at_max_matches() {
    let dir = tmpdir();
    let body = "x\n".repeat(50);
    write_file(dir.path(), "a.txt", &body);
    let res = ops::grep(
        dir.path().to_string_lossy().into(),
        "x".into(),
        true,
        false,
        vec![],
        vec![],
        10,
        4096,
    )
    .unwrap();
    let FsResult::Grep { matches, truncated } = res else {
        panic!();
    };
    assert_eq!(matches.len(), 10);
    assert!(truncated);
}

#[test]
fn grep_ignore_case_flag_matches() {
    let dir = tmpdir();
    write_file(dir.path(), "a.txt", "HELLO world\n");
    let res = ops::grep(
        dir.path().to_string_lossy().into(),
        "hello".into(),
        true,
        true,
        vec![],
        vec![],
        1000,
        4096,
    )
    .unwrap();
    let FsResult::Grep { matches, .. } = res else {
        panic!();
    };
    assert_eq!(matches.len(), 1);
}

#[test]
fn grep_bad_regex_returns_s217() {
    let dir = tmpdir();
    let err = ops::grep(
        dir.path().to_string_lossy().into(),
        "(".into(), // unbalanced
        true,
        false,
        vec![],
        vec![],
        1000,
        4096,
    )
    .unwrap_err();
    assert_eq!(err.code, "S217");
}

#[test]
fn grep_skips_binary_files() {
    let dir = tmpdir();
    // NUL in first 8 KiB marks the file as binary; it must be skipped.
    let mut bin = vec![0u8; 16];
    bin.extend_from_slice(b"needle");
    std::fs::write(dir.path().join("b.bin"), &bin).unwrap();
    write_file(dir.path(), "t.txt", "needle\n");
    let res = ops::grep(
        dir.path().to_string_lossy().into(),
        "needle".into(),
        true,
        false,
        vec![],
        vec![],
        1000,
        4096,
    )
    .unwrap();
    let FsResult::Grep { matches, .. } = res else {
        panic!();
    };
    assert_eq!(matches.len(), 1);
    assert!(matches[0].path.ends_with(".txt"));
}

// ──────────────────────────────────────────────────────────────────────────────
// grep — gitignore-style glob semantics (Task 1.3)
//
// `*.py` (no `/`) must match against the basename, so `src/main.py` and
// `data/foo.py` both qualify. `src/*.py` (has `/`) must match against
// the full relative path, so it picks `src/main.py` only. `**/*.py`
// keeps its prior any-depth behavior.
// ──────────────────────────────────────────────────────────────────────────────

/// Helper: run grep with the given include/exclude globs, return the
/// list of matched file basenames (sorted) so assertions are stable
/// across walkdir traversal order.
fn grep_basenames(
    root: &std::path::Path,
    pattern: &str,
    include_glob: Vec<String>,
    exclude_glob: Vec<String>,
) -> Vec<String> {
    let res = ops::grep(
        root.to_string_lossy().into(),
        pattern.into(),
        true,
        false,
        include_glob,
        exclude_glob,
        1000,
        4096,
    )
    .unwrap();
    let FsResult::Grep { matches, .. } = res else {
        panic!("expected Grep result");
    };
    let mut names: Vec<String> = matches
        .into_iter()
        .map(|m| {
            std::path::Path::new(&m.path)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or(m.path)
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

fn setup_py_tree() -> tempfile::TempDir {
    let dir = tmpdir();
    write_file(dir.path(), "src/main.py", "hit\n");
    write_file(dir.path(), "src/main.rs", "hit\n");
    write_file(dir.path(), "data/foo.py", "hit\n");
    write_file(dir.path(), "top.py", "hit\n");
    dir
}

#[test]
fn grep_include_glob_basename_matches_at_any_depth() {
    // `*.py` has no slash → match basename → all .py files at any depth.
    let dir = setup_py_tree();
    let names = grep_basenames(dir.path(), "hit", vec!["*.py".into()], vec![]);
    assert_eq!(names, vec!["foo.py", "main.py", "top.py"]);
}

#[test]
fn grep_include_glob_basename_excludes_non_matching_extensions() {
    // `*.py` must not match `main.rs`.
    let dir = setup_py_tree();
    let names = grep_basenames(dir.path(), "hit", vec!["*.py".into()], vec![]);
    assert!(!names.iter().any(|n| n == "main.rs"));
}

#[test]
fn grep_include_glob_with_slash_anchors_to_relative_path() {
    // `src/*.py` has a slash → match against relpath → only `src/main.py`.
    let dir = setup_py_tree();
    let names = grep_basenames(dir.path(), "hit", vec!["src/*.py".into()], vec![]);
    assert_eq!(names, vec!["main.py"]);
}

#[test]
fn grep_include_glob_double_star_matches_at_any_depth_regression() {
    // `**/*.py` already worked before this fix. Lock that in.
    let dir = setup_py_tree();
    let names = grep_basenames(dir.path(), "hit", vec!["**/*.py".into()], vec![]);
    assert_eq!(names, vec!["foo.py", "main.py", "top.py"]);
}

#[test]
fn grep_exclude_glob_basename_excludes_at_any_depth() {
    // `exclude_glob: ['*.py']` excludes every .py file regardless of depth.
    let dir = setup_py_tree();
    let names = grep_basenames(dir.path(), "hit", vec![], vec!["*.py".into()]);
    assert_eq!(names, vec!["main.rs"]);
}

#[test]
fn grep_exclude_glob_with_slash_anchors_to_relative_path() {
    // `exclude_glob: ['src/*.py']` excludes only `src/main.py`, not
    // `data/foo.py` or `top.py`.
    let dir = setup_py_tree();
    let names = grep_basenames(dir.path(), "hit", vec![], vec!["src/*.py".into()]);
    assert_eq!(names, vec!["foo.py", "main.rs", "top.py"]);
}

#[test]
fn grep_glob_matches_path_helper_semantics() {
    // Direct unit-test of the discriminator. Catches regressions in the
    // basename-vs-relpath decision without standing up a tmpdir.
    use super::ops::glob_matches_path;
    // No-slash patterns → basename match.
    assert!(glob_matches_path("*.py", "src/main.py"));
    assert!(glob_matches_path("*.py", "data/foo.py"));
    assert!(glob_matches_path("*.py", "main.py"));
    assert!(!glob_matches_path("*.py", "src/main.rs"));
    // Slash-bearing patterns → relpath match.
    assert!(glob_matches_path("src/*.py", "src/main.py"));
    assert!(!glob_matches_path("src/*.py", "data/foo.py"));
    assert!(!glob_matches_path("src/*.py", "src/sub/deep.py"));
    // `**/` prefix → any-depth match.
    assert!(glob_matches_path("**/*.py", "src/main.py"));
    assert!(glob_matches_path("**/*.py", "main.py"));
}

// ──────────────────────────────────────────────────────────────────────────────
// sed
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn sed_global_literal_replaces_all_occurrences() {
    let dir = tmpdir();
    let f = dir.path().join("x.txt");
    std::fs::write(&f, "foo foo bar foo\nfoo end\n").unwrap();
    let res = ops::sed(
        vec![f.to_string_lossy().into()],
        None,
        true,
        Vec::new(),
        Vec::new(),
        "foo".into(),
        "BAR".into(),
        false, // regex=false (literal)
        false, // first_only
        false,
    )
    .unwrap();
    let FsResult::Sed {
        results,
        total_replacements,
    } = res
    else {
        panic!();
    };
    assert_eq!(total_replacements, 4);
    assert_eq!(results[0].replacements, 4);
    assert!(results[0].success);
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "BAR BAR bar BAR\nBAR end\n"
    );
}

#[test]
fn sed_first_only_replaces_one_per_line() {
    let dir = tmpdir();
    let f = dir.path().join("x.txt");
    std::fs::write(&f, "a a a\nb b\n").unwrap();
    ops::sed(
        vec![f.to_string_lossy().into()],
        None,
        true,
        Vec::new(),
        Vec::new(),
        "a".into(),
        "Z".into(),
        false,
        true,
        false,
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "Z a a\nb b\n");
}

#[test]
fn sed_regex_mode_supports_captures() {
    let dir = tmpdir();
    let f = dir.path().join("x.txt");
    std::fs::write(&f, "name: alice, name: bob\n").unwrap();
    ops::sed(
        vec![f.to_string_lossy().into()],
        None,
        true,
        Vec::new(),
        Vec::new(),
        r"name: (\w+)".into(),
        "user=$1".into(),
        true,
        false,
        false,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "user=alice, user=bob\n"
    );
}

#[test]
fn sed_per_file_failure_does_not_abort_remaining() {
    let dir = tmpdir();
    let good = dir.path().join("good.txt");
    std::fs::write(&good, "x\n").unwrap();
    let missing = dir.path().join("missing.txt");
    let res = ops::sed(
        vec![
            missing.to_string_lossy().into(),
            good.to_string_lossy().into(),
        ],
        None,
        true,
        Vec::new(),
        Vec::new(),
        "x".into(),
        "Y".into(),
        false,
        false,
        false,
    )
    .unwrap();
    let FsResult::Sed {
        results,
        total_replacements,
    } = res
    else {
        panic!();
    };
    assert_eq!(total_replacements, 1);
    assert_eq!(results.len(), 2);
    assert!(!results[0].success);
    assert!(results[1].success);
    assert_eq!(results[1].replacements, 1);
    assert_eq!(std::fs::read_to_string(&good).unwrap(), "Y\n");
}

#[test]
fn sed_bad_regex_returns_s217() {
    let dir = tmpdir();
    let err = ops::sed(
        vec![dir.path().join("any.txt").to_string_lossy().into()],
        None,
        true,
        Vec::new(),
        Vec::new(),
        "(".into(),
        "".into(),
        true,
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, "S217");
}

#[test]
fn sed_atomic_on_write_failure_preserves_original() {
    // Simulate "write failed" by pointing the target into a read-only dir.
    let dir = tmpdir();
    let sub = dir.path().join("ro");
    std::fs::create_dir(&sub).unwrap();
    let f = sub.join("x.txt");
    std::fs::write(&f, "orig\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o500)).unwrap();

    let res = ops::sed(
        vec![f.to_string_lossy().into()],
        None,
        true,
        Vec::new(),
        Vec::new(),
        "orig".into(),
        "new".into(),
        false,
        false,
        false,
    )
    .unwrap();
    let FsResult::Sed { results, .. } = res else {
        panic!();
    };
    assert!(!results[0].success, "expected per-file failure");

    // Restore permissions so tempdir cleanup works.
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o700)).unwrap();

    // Original content preserved.
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "orig\n");
}

// ──────────────────────────────────────────────────────────────────────────────
// sed (path-form: walk a directory tree, mirroring grep)
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn sed_path_recursive_walks_tree_and_rewrites_matching_files() {
    // Layout: tmp/a.py, tmp/b.py, tmp/c.rs.
    // Sed with path=tmp, recursive=true, include_glob=["*.py"] →
    // both .py files rewritten, .rs file untouched.
    let dir = tmpdir();
    write_file(dir.path(), "a.py", "value = old\n");
    write_file(dir.path(), "b.py", "x = old\nold\n");
    write_file(dir.path(), "c.rs", "let s = old;\n");

    let res = ops::sed(
        Vec::new(),
        Some(dir.path().to_string_lossy().into()),
        true,
        vec!["*.py".into()],
        Vec::new(),
        "old".into(),
        "NEW".into(),
        false, // literal
        false,
        false,
    )
    .unwrap();

    let FsResult::Sed {
        results,
        total_replacements,
    } = res
    else {
        panic!("expected Sed result");
    };
    assert!(
        total_replacements > 0,
        "expected at least one replacement, got total={total_replacements}"
    );
    // Only .py files reported; .rs filtered out.
    assert_eq!(
        results.len(),
        2,
        "expected 2 matched files (.py), got {} ({results:?})",
        results.len()
    );
    for r in &results {
        assert!(r.success, "expected per-file success: {r:?}");
        assert!(r.path.ends_with(".py"));
    }

    // Concrete content checks.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
        "value = NEW\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.py")).unwrap(),
        "x = NEW\nNEW\n"
    );
    // .rs file must remain untouched.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("c.rs")).unwrap(),
        "let s = old;\n"
    );
}

#[test]
fn sed_path_with_exclude_glob_skips_matching_files() {
    // include + exclude both apply: include selects .py, exclude
    // strips files under "skip/" → only top-level a.py rewritten.
    let dir = tmpdir();
    write_file(dir.path(), "a.py", "v = old\n");
    write_file(dir.path(), "skip/b.py", "v = old\n");

    let res = ops::sed(
        Vec::new(),
        Some(dir.path().to_string_lossy().into()),
        true,
        vec!["*.py".into()],
        vec!["skip/*".into()],
        "old".into(),
        "NEW".into(),
        false,
        false,
        false,
    )
    .unwrap();
    let FsResult::Sed { results, .. } = res else {
        panic!();
    };
    assert_eq!(
        results.len(),
        1,
        "expected exclude_glob to filter skip/b.py, got {results:?}"
    );
    assert!(results[0].path.ends_with("a.py"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("skip/b.py")).unwrap(),
        "v = old\n",
        "skip/b.py should be untouched"
    );
}

#[test]
fn sed_files_form_still_works_regression() {
    // Belt-and-suspenders: the legacy `files: [...]` call path still
    // works after adding the new fields.
    let dir = tmpdir();
    let f = dir.path().join("x.txt");
    std::fs::write(&f, "abc abc\n").unwrap();
    let res = ops::sed(
        vec![f.to_string_lossy().into()],
        None,
        true,
        Vec::new(),
        Vec::new(),
        "abc".into(),
        "Z".into(),
        false,
        false,
        false,
    )
    .unwrap();
    let FsResult::Sed {
        results,
        total_replacements,
    } = res
    else {
        panic!();
    };
    assert_eq!(total_replacements, 2);
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "Z Z\n");
}

#[test]
fn sed_path_form_on_single_file_rewrites_just_that_file() {
    // path that points at a single regular file is equivalent to
    // files=[path]. include_glob still applies to the basename.
    let dir = tmpdir();
    let f = dir.path().join("only.py");
    std::fs::write(&f, "old\n").unwrap();

    let res = ops::sed(
        Vec::new(),
        Some(f.to_string_lossy().into()),
        true,
        Vec::new(),
        Vec::new(),
        "old".into(),
        "NEW".into(),
        false,
        false,
        false,
    )
    .unwrap();
    let FsResult::Sed {
        results,
        total_replacements,
    } = res
    else {
        panic!();
    };
    assert_eq!(total_replacements, 1);
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "NEW\n");
}

#[test]
fn sed_path_form_symlink_to_dir_walks_target() {
    // Regression: when `path` is a symlink to a directory we must
    // resolve the link and walk it, not treat it as a single file.
    // Previously `is_file() || is_symlink()` short-circuited into
    // single-file rewrite, which then tried to read a directory.
    let dir = tmpdir();
    let real_dir = dir.path().join("real_dir");
    std::fs::create_dir(&real_dir).unwrap();
    write_file(&real_dir, "a.py", "x = old\n");
    write_file(&real_dir, "b.py", "y = old\n");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real_dir, &link).unwrap();

    let res = ops::sed(
        Vec::new(),
        Some(link.to_string_lossy().into()),
        true,
        vec!["*.py".into()],
        Vec::new(),
        "old".into(),
        "NEW".into(),
        false,
        false,
        false,
    )
    .unwrap();
    let FsResult::Sed {
        results,
        total_replacements,
    } = res
    else {
        panic!("expected Sed result");
    };
    assert_eq!(
        results.len(),
        2,
        "expected to walk into the symlink target and rewrite both \
         .py files; got {results:?}"
    );
    for r in &results {
        assert!(r.success, "per-file failure: {r:?}");
    }
    assert_eq!(total_replacements, 2);
    assert_eq!(
        std::fs::read_to_string(real_dir.join("a.py")).unwrap(),
        "x = NEW\n"
    );
    assert_eq!(
        std::fs::read_to_string(real_dir.join("b.py")).unwrap(),
        "y = NEW\n"
    );
}

#[test]
fn sed_path_form_recursive_false_on_dir_returns_s210() {
    // Mirrors grep: `recursive=false` on a directory `path` is rejected
    // with S210. Sed used to silently walk depth-1 here, which diverged
    // from grep without good reason.
    let dir = tmpdir();
    write_file(dir.path(), "a.py", "old\n");
    let err = ops::sed(
        Vec::new(),
        Some(dir.path().to_string_lossy().into()),
        false, // recursive=false on a directory
        Vec::new(),
        Vec::new(),
        "old".into(),
        "NEW".into(),
        false,
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, "S210");
    // File must remain untouched — the rejection is up-front.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
        "old\n"
    );
}

#[test]
fn sed_path_form_recursive_false_on_single_file_is_ok() {
    // Single-file `path` form: `recursive=false` is allowed because the
    // rejection is keyed on `target_is_dir`. Belt-and-suspenders so a
    // future refactor doesn't accidentally over-reject.
    let dir = tmpdir();
    let f = dir.path().join("only.py");
    std::fs::write(&f, "old\n").unwrap();
    let res = ops::sed(
        Vec::new(),
        Some(f.to_string_lossy().into()),
        false,
        Vec::new(),
        Vec::new(),
        "old".into(),
        "NEW".into(),
        false,
        false,
        false,
    )
    .unwrap();
    let FsResult::Sed {
        total_replacements, ..
    } = res
    else {
        panic!();
    };
    assert_eq!(total_replacements, 1);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "NEW\n");
}
