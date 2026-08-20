// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Integration coverage for the durable trace archive.
//!
//! The archive is process-global, so every test is serial. The suite exercises
//! the public hot-store → writer-thread → SQLite boundary through the
//! feature-gated test hooks; production code retains no synchronous archive
//! controls.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use iii::{
    engine::Engine,
    function::FunctionResult,
    workers::observability::{
        HealthCheckInput, ObservabilityWorker,
        config::TraceStorageConfig,
        otel::{
            InMemorySpanExporter, InMemorySpanStorage, StoredSpan, StoredSpanEvent, StoredSpanLink,
            get_span_storage, trace_storage_test_support as trace_store,
        },
    },
};
use serial_test::serial;

const MIB: usize = 1024 * 1024;

struct ResetTraceStorage;

impl Drop for ResetTraceStorage {
    fn drop(&mut self) {
        trace_store::reset();
    }
}

fn config(directory: &Path) -> TraceStorageConfig {
    TraceStorageConfig {
        directory: directory.to_string_lossy().into_owned(),
        max_disk_bytes: 64 * MIB as u64,
        retention_seconds: 30 * 24 * 60 * 60,
        memory_max_bytes: 32 * MIB as u64,
        ..TraceStorageConfig::default()
    }
}

fn span(trace_id: &str, span_id: &str, start_time: u64, payload_bytes: usize) -> StoredSpan {
    let mut attributes = vec![("iii.tag.message".to_string(), trace_id.to_string())];
    if payload_bytes > 0 {
        attributes.push(("payload".to_string(), "x".repeat(payload_bytes)));
    }
    StoredSpan {
        trace_id: trace_id.to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        name: format!("operation-{trace_id}"),
        start_time_unix_nano: start_time,
        end_time_unix_nano: start_time + 1_000_000,
        status: "ok".to_string(),
        status_description: None,
        attributes,
        service_name: "trace-storage-e2e".to_string(),
        events: Vec::<StoredSpanEvent>::new(),
        links: Vec::<StoredSpanLink>::new(),
        instrumentation_scope_name: None,
        instrumentation_scope_version: None,
        flags: None,
        trace_state: None,
        pending: false,
    }
}

fn start_archive(config: TraceStorageConfig) -> Arc<InMemorySpanStorage> {
    trace_store::configure(Some(config));
    assert_eq!(
        trace_store::status()["archive"],
        "healthy",
        "trace archive must initialize"
    );
    let hot = Arc::new(InMemorySpanStorage::new_with_limits(
        10_000,
        128 * MIB as u64,
    ));
    trace_store::attach(&hot);
    hot
}

fn storage_error() -> String {
    trace_store::status()["last_error"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
#[serial]
fn missing_directory_persists_and_restart_reads_completed_trace() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    let archive_dir = root.path().join("nested").join("trace-archive");
    assert!(!archive_dir.exists());
    let archive_config = config(&archive_dir);
    let hot = start_archive(archive_config.clone());

    hot.add_spans(vec![span("restart-trace", "span-1", 1, 0)]);
    trace_store::flush().expect("persist completed trace");
    assert!(archive_dir.join("traces.sqlite3").is_file());

    trace_store::reset();
    let _fresh_hot = start_archive(archive_config);
    let restored = trace_store::read_spans().expect("read archive after restart");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].trace_id, "restart-trace");
}

#[test]
#[serial]
fn reconfigure_same_directory_reports_complete() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    let archive_config = config(root.path());

    let hot = start_archive(archive_config.clone());
    hot.add_spans(vec![span("same-dir-trace", "span-1", 1, 0)]);
    trace_store::flush().expect("persist before reconfigure");

    // An in-process reconfigure over the same directory must observe the
    // previous store's clean shutdown, not its in-service marker.
    trace_store::configure(Some(archive_config));
    let status = trace_store::status();
    assert_eq!(status["archive"], "healthy");
    assert_eq!(
        status["completeness"], "complete",
        "a clean same-directory swap must not report an unclean shutdown"
    );
    let restored = trace_store::read_spans().expect("read after reconfigure");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].trace_id, "same-dir-trace");
}

#[test]
#[serial]
fn invalid_parent_reports_actionable_error_without_modifying_it() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    let parent = root.path().join("not-a-directory");
    let sentinel = b"do not replace this evidence";
    fs::write(&parent, sentinel).expect("write sentinel");
    let archive_dir = parent.join("traces");

    trace_store::configure(Some(config(&archive_dir)));

    assert_eq!(trace_store::status()["archive"], "degraded");
    let error = storage_error();
    assert!(error.contains(archive_dir.to_string_lossy().as_ref()));
    assert!(error.contains("cannot create trace storage directory"));
    assert_eq!(fs::read(&parent).expect("read sentinel"), sentinel);
}

#[test]
#[serial]
fn corrupt_database_is_rejected_without_replacing_the_original_file() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    let database = root.path().join("traces.sqlite3");
    let corrupt = b"not a sqlite database";
    fs::write(&database, corrupt).expect("write corrupt fixture");

    trace_store::configure(Some(config(root.path())));

    assert_eq!(trace_store::status()["archive"], "degraded");
    let error = storage_error();
    assert!(error.contains(database.to_string_lossy().as_ref()));
    assert!(error.contains("initialization"));
    assert_eq!(fs::read(&database).expect("read corrupt fixture"), corrupt);
}

#[test]
#[serial]
fn corrupt_payload_skips_only_the_bad_record_and_keeps_valid_trace_queryable() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    let archive_config = config(root.path());
    let hot = start_archive(archive_config.clone());
    hot.add_spans(vec![
        span("valid-trace", "span-1", 1, 0),
        span("corrupt-trace", "span-1", 2, 0),
    ]);
    trace_store::flush().expect("persist fixtures");
    trace_store::reset();

    let database = root.path().join("traces.sqlite3");
    let connection = rusqlite::Connection::open(&database).expect("open archive fixture");
    connection
        .execute(
            "UPDATE spans SET payload = '{not-json' WHERE trace_id = 'corrupt-trace'",
            [],
        )
        .expect("corrupt one payload");
    drop(connection);

    let _fresh_hot = start_archive(archive_config);
    let restored = trace_store::read_spans().expect("read recoverable records");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].trace_id, "valid-trace");
    assert_eq!(trace_store::status()["archive"], "degraded");
    assert!(storage_error().contains("corrupt trace payload skipped"));
}

#[test]
#[serial]
fn retention_removes_only_expired_completed_traces() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    let mut archive_config = config(root.path());
    archive_config.retention_seconds = 1;
    let hot = start_archive(archive_config);

    hot.add_spans(vec![span("expired-trace", "span-1", 1, 0)]);
    trace_store::flush().expect("persist expired trace");
    thread::sleep(Duration::from_millis(1_200));
    hot.add_spans(vec![span("recent-trace", "span-1", 2, 0)]);
    trace_store::flush().expect("persist recent trace");
    trace_store::retain().expect("run retention");

    let trace_ids: HashSet<_> = trace_store::read_spans()
        .expect("read retained archive")
        .into_iter()
        .map(|stored| stored.trace_id)
        .collect();
    assert!(!trace_ids.contains("expired-trace"));
    assert!(trace_ids.contains("recent-trace"));
}

#[test]
#[serial]
fn capacity_evicts_the_oldest_eligible_trace() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    let mut archive_config = config(root.path());
    // The 8 MiB measurement tolerance is part of the public storage contract.
    // A 3 MiB attribute is stored in payload and attribute index, which makes
    // the second completed trace cross the high watermark deterministically.
    archive_config.max_disk_bytes = 8 * MIB as u64;
    let hot = start_archive(archive_config);

    hot.add_spans(vec![span("oldest-trace", "span-1", 1, 3 * MIB)]);
    trace_store::flush().expect("persist oldest trace");
    hot.add_spans(vec![span("newest-trace", "span-1", 2, 3 * MIB)]);
    trace_store::flush().expect("persist newest trace with eviction");

    let trace_ids: HashSet<_> = trace_store::read_spans()
        .expect("read archive after eviction")
        .into_iter()
        .map(|stored| stored.trace_id)
        .collect();
    assert!(!trace_ids.contains("oldest-trace"));
    assert!(trace_ids.contains("newest-trace"));
}

#[test]
#[serial]
fn protected_active_trace_is_not_partially_committed_when_capacity_is_exhausted() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    let mut archive_config = config(root.path());
    // Keep the configured cap below SQLite's fixed 8 MiB allocation
    // tolerance. Each 3 MiB payload is indexed separately from the serialized
    // span, so the second span must either preserve the completed first span
    // or fail as one protected batch.
    archive_config.max_disk_bytes = 1;
    let hot = start_archive(archive_config);

    hot.add_spans(vec![span("protected-trace", "span-1", 1, 3 * MIB)]);
    trace_store::flush().expect("persist first protected span");
    hot.add_spans(vec![span("protected-trace", "span-2", 2, 3 * MIB)]);
    let error = trace_store::flush().expect_err("reject protected batch at capacity");

    assert!(error.contains("trace storage limit reached"));
    let stored = trace_store::read_spans().expect("read archive after rejected batch");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].span_id, "span-1");
    assert_eq!(
        hot.dirty_spans(16, 20 * MIB as u64).len(),
        1,
        "the rejected span remains eligible for retry"
    );
}

#[test]
#[serial]
fn concurrent_ingest_flush_and_reads_converge_without_duplicates() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    let hot = start_archive(config(root.path()));
    const PRODUCERS: usize = 8;
    const SPANS_PER_PRODUCER: usize = 48;

    thread::scope(|scope| {
        for producer in 0..PRODUCERS {
            let hot = hot.clone();
            scope.spawn(move || {
                for sequence in 0..SPANS_PER_PRODUCER {
                    hot.add_spans(vec![span(
                        &format!("concurrent-{producer}-{sequence}"),
                        "span-1",
                        (producer * SPANS_PER_PRODUCER + sequence) as u64,
                        0,
                    )]);
                }
            });
        }
        scope.spawn(|| {
            for _ in 0..40 {
                trace_store::read_spans().expect("concurrent archive read");
                thread::sleep(Duration::from_millis(5));
            }
        });
    });
    trace_store::flush().expect("flush every producer span");

    let stored = trace_store::read_spans().expect("read converged archive");
    assert_eq!(stored.len(), PRODUCERS * SPANS_PER_PRODUCER);
    let keys: HashSet<_> = stored
        .iter()
        .map(|stored| (stored.trace_id.as_str(), stored.span_id.as_str()))
        .collect();
    assert_eq!(keys.len(), stored.len(), "no duplicate persisted span keys");
    assert_eq!(trace_store::status()["archive"], "healthy");
}

#[tokio::test]
#[serial]
async fn health_reports_archive_initialization_failure_while_memory_remains_available() {
    let _reset = ResetTraceStorage;
    trace_store::reset();
    let storage = match get_span_storage() {
        Some(storage) => storage,
        None => {
            let _ = InMemorySpanExporter::new(1_000, "trace-storage-e2e".to_string());
            get_span_storage().expect("initialize memory span storage")
        }
    };
    storage.clear();
    storage.add_spans(vec![span("memory-survives", "span-1", 1, 0)]);

    let root = tempfile::tempdir().expect("temp root");
    let parent = root.path().join("not-a-directory");
    fs::write(&parent, b"sentinel").expect("write invalid parent");
    let archive_dir = parent.join("traces");
    trace_store::configure(Some(config(&archive_dir)));

    let worker = ObservabilityWorker::for_test(Arc::new(Engine::new()), None).expect("worker");
    let result = worker.health_check(HealthCheckInput {}).await;
    let FunctionResult::Success(health) = result else {
        panic!("health check must succeed while archive is degraded");
    };
    assert_eq!(health.status, "degraded");
    assert_eq!(
        health.components.spans["details"]["archive"]["archive"],
        "degraded"
    );
    assert!(
        health.components.spans["details"]["archive"]["last_error"]
            .as_str()
            .is_some_and(|error| error.contains(archive_dir.to_string_lossy().as_ref()))
    );
    assert_eq!(storage.get_spans_by_trace_id("memory-survives").len(), 1);
}

#[cfg(unix)]
const PERMISSION_CHILD: &str = "III_TRACE_STORAGE_PERMISSION_CHILD";
#[cfg(unix)]
const PERMISSION_DIRECTORY: &str = "III_TRACE_STORAGE_PERMISSION_DIRECTORY";

#[cfg(unix)]
#[test]
#[serial]
fn permission_denied_directory_reports_actionable_error_without_partial_archive() {
    use std::{os::unix::fs::PermissionsExt, process::Command};

    if std::env::var_os(PERMISSION_CHILD).is_some() {
        let directory = PathBuf::from(
            std::env::var_os(PERMISSION_DIRECTORY).expect("child directory is provided"),
        );
        if unsafe { libc::geteuid() } == 0 {
            assert_eq!(unsafe { libc::setgid(65_534) }, 0, "drop child gid");
            assert_eq!(unsafe { libc::setuid(65_534) }, 0, "drop child uid");
        }
        trace_store::reset();
        trace_store::configure(Some(config(&directory)));
        assert_eq!(trace_store::status()["archive"], "degraded");
        let error = storage_error();
        assert!(error.contains(directory.to_string_lossy().as_ref()));
        assert!(
            error.contains("Permission denied")
                || error.contains("permission denied")
                || error.contains("cannot create trace storage directory"),
            "unexpected permission error: {error}"
        );
        return;
    }

    let _reset = ResetTraceStorage;
    trace_store::reset();
    let root = tempfile::tempdir().expect("temp root");
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755))
        .expect("make root traversable by child");
    let locked = root.path().join("locked");
    fs::create_dir(&locked).expect("create locked directory");
    let running_as_root = unsafe { libc::geteuid() } == 0;
    fs::set_permissions(
        &locked,
        fs::Permissions::from_mode(if running_as_root { 0o700 } else { 0o000 }),
    )
    .expect("lock trace directory parent");
    let sentinel = root.path().join("sentinel");
    fs::write(&sentinel, b"preserve me").expect("write sentinel");

    let executable = std::env::current_exe().expect("locate integration test binary");
    let output = Command::new(executable)
        .args([
            "--exact",
            "permission_denied_directory_reports_actionable_error_without_partial_archive",
            "--nocapture",
        ])
        .env(PERMISSION_CHILD, "1")
        .env(PERMISSION_DIRECTORY, locked.join("traces"))
        .output()
        .expect("run permission-denied child");

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))
        .expect("restore locked directory permissions");
    assert!(
        output.status.success(),
        "permission child failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(sentinel).expect("read sentinel"), b"preserve me");
    assert!(!locked.join("traces.sqlite3").exists());
}

#[cfg(unix)]
const FILE_LIMIT_CHILD: &str = "III_TRACE_STORAGE_FILE_LIMIT_CHILD";
#[cfg(unix)]
const FILE_LIMIT_DIRECTORY: &str = "III_TRACE_STORAGE_FILE_LIMIT_DIRECTORY";

#[cfg(unix)]
#[test]
#[serial]
fn disk_full_degrades_without_partial_commit_and_keeps_committed_data() {
    use std::process::Command;

    if std::env::var_os(FILE_LIMIT_CHILD).is_some() {
        let directory = PathBuf::from(
            std::env::var_os(FILE_LIMIT_DIRECTORY).expect("child directory is provided"),
        );
        let _reset = ResetTraceStorage;
        trace_store::reset();
        let hot = start_archive(config(&directory));
        hot.add_spans(vec![span("committed-trace", "span-1", 1, 0)]);
        trace_store::flush().expect("persist baseline before disk-full fault");
        hot.add_spans(vec![span("rejected-trace", "span-1", 2, 3 * MIB)]);

        let deadline = Instant::now() + Duration::from_secs(8);
        while trace_store::status()["archive"] != "degraded" && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(trace_store::status()["archive"], "degraded");
        let error = storage_error();
        assert!(
            error.contains("write trace")
                || error.contains("disk I/O error")
                || error.contains("database or disk is full"),
            "unexpected disk-full error: {error}"
        );
        let trace_ids: HashSet<_> = trace_store::read_spans()
            .expect("read committed archive data")
            .into_iter()
            .map(|stored| stored.trace_id)
            .collect();
        assert!(trace_ids.contains("committed-trace"));
        assert!(!trace_ids.contains("rejected-trace"));
        return;
    }

    let _reset = ResetTraceStorage;
    trace_store::reset();
    let directory = tempfile::tempdir().expect("temp archive directory");
    let executable = std::env::current_exe().expect("locate integration test binary");
    let output = Command::new("bash")
        .args([
            "-c",
            "ulimit -f 1024; trap '' XFSZ; exec \"$1\" --exact \"$2\" --nocapture",
            "trace-storage-file-limit-child",
        ])
        .arg(executable)
        .arg("disk_full_degrades_without_partial_commit_and_keeps_committed_data")
        .env(FILE_LIMIT_CHILD, "1")
        .env(FILE_LIMIT_DIRECTORY, directory.path())
        .output()
        .expect("run disk-full child");
    assert!(
        output.status.success(),
        "disk-full child failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
