// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Durable trace storage.
//!
//! The hot store remains the realtime ingestion path. This module owns the
//! write-behind SQLite connection and reads historical finalized spans without
//! making the application workload wait for disk I/O.

use super::{config::TraceStorageConfig, otel::InMemorySpanStorage};
use rusqlite::{Connection, ToSql, params, params_from_iter};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock, RwLock, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use super::otel::StoredSpan;

const BATCH_MAX_SPANS: usize = 512;
const BATCH_MAX_BYTES: u64 = 4 * 1024 * 1024;
const BATCH_INTERVAL: Duration = Duration::from_millis(250);
const CAP_HIGH_WATERMARK: f64 = 0.90;
const CAP_LOW_WATERMARK: f64 = 0.80;
/// Work one periodic tick may do before the writer serves queued control
/// messages again; a tick that runs out wakes the writer immediately.
const TICK_MAX_BATCHES: usize = 32;
const TICK_TIME_BUDGET: Duration = Duration::from_secs(1);
/// `flush` drains within its caller's 5 s response timeout.
const FLUSH_TIME_BUDGET: Duration = Duration::from_secs(4);
/// Pages returned to the filesystem per `incremental_vacuum` step (4 MiB at
/// the default page size); each step is checkpointed before the next one.
const RECLAIM_BLOCK_PAGES: u32 = 1024;
/// Traces deleted per retention transaction.
const RETENTION_DELETE_CHUNK: usize = 256;
/// The full-VACUUM fallback rewrites the whole file; never more often.
const FULL_VACUUM_MIN_INTERVAL: Duration = Duration::from_secs(600);
const BACKOFF_INITIAL: Duration = Duration::from_secs(1);
/// Retry ceiling while the whole archive is blocked or failing.
const BLOCKED_BACKOFF_MAX: Duration = Duration::from_secs(30);
/// Retry ceiling for one trace that fits an empty archive but not this one.
const TRACE_BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Bound on remembered trace backoffs (entries are small; the dirty set they
/// describe is itself bounded by the hot cache).
const MAX_DEFERRED_TRACES: usize = 65_536;
/// A trace group whose lower bound exceeds `max_disk_bytes / 4` is measured
/// in isolation before anything is evicted to make room for it.
const LARGE_GROUP_DIVISOR: u64 = 4;
/// Largest lower bound measured in the scratch database (64 MiB). Larger
/// groups, and any group while the scratch is unavailable, are measured by a
/// trial in the archive itself, with nothing evicted for them.
const SCRATCH_MAX_LOWER_BOUND: u64 = 64 * 1024 * 1024;
/// After a scratch failure (unusable TMPDIR, no space), skip it this long.
const SCRATCH_RETRY_AFTER: Duration = Duration::from_secs(600);
/// Trials `judge_rejected` may spend shrinking a group that does not fit
/// beside its trace's durable rows.
const JUDGE_ROUNDS: usize = 4;
/// Keep clear of SQLite's default 999 bind-variable limit. One slot is used
/// by the epoch, leaving a margin for future query predicates.
const TRACE_ID_QUERY_CHUNK: usize = 900;

/// A span is a trace root when it has no parent or its parent row is absent.
/// The dangling-parent half covers traces that entered iii from an external
/// caller via an incoming `traceparent`. Requires the enclosing query to
/// alias the spans table as `s`.
const ROOT_SPAN_PREDICATE_SQL: &str = "(s.parent_span_id IS NULL OR NOT EXISTS (
    SELECT 1 FROM spans p
    WHERE p.epoch = s.epoch
      AND p.trace_id = s.trace_id
      AND p.span_id = s.parent_span_id
))";

/// Internal engine spans, matching the in-memory rule used by the list and
/// group handlers. Requires the enclosing query to alias the spans table as
/// `s`.
const INTERNAL_SPAN_PREDICATE_SQL: &str = "EXISTS (
    SELECT 1 FROM span_attributes internal_attr
    WHERE internal_attr.epoch = s.epoch
      AND internal_attr.trace_id = s.trace_id
      AND internal_attr.span_id = s.span_id
      AND (
          (internal_attr.key = 'iii.function.kind' AND internal_attr.value = 'internal')
          OR (internal_attr.key = 'function_id' AND internal_attr.value LIKE 'engine::%')
      )
)";

/// Return freed pages to the filesystem in bounded steps.
///
/// `PRAGMA incremental_vacuum(N)` is lazy (pages are only released as its
/// empty result rows are stepped), so each step is drained and then
/// checkpointed with TRUNCATE so the shrunken image reaches the main file.
/// A single unbounded vacuum writes every page it touches into the WAL before
/// the one checkpoint: a 57.6 MB archive emptied by retention peaked at
/// 110 MB (54 MB of WAL) although nothing new was written. Steps of
/// `RECLAIM_BLOCK_PAGES` keep that transient to about one step. A reader that
/// pins the WAL makes the checkpoint report busy; the loop then stops and
/// leaves the remaining free pages for a later call instead of growing the
/// WAL behind the reader.
fn reclaim_free_pages(connection: &Connection) -> rusqlite::Result<()> {
    let step = format!("PRAGMA incremental_vacuum({RECLAIM_BLOCK_PAGES})");
    loop {
        let free_before: i64 =
            connection.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
        if free_before > 0 {
            let mut vacuum = connection.prepare(&step)?;
            let mut rows = vacuum.query([])?;
            while rows.next()?.is_some() {}
        }
        let busy: i64 =
            connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
        let free_after: i64 =
            connection.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
        // Done, pinned by a reader, or not shrinking (a file that predates
        // incremental auto_vacuum): stop rather than spin.
        if busy != 0 || free_after == 0 || free_after >= free_before {
            return Ok(());
        }
    }
}

fn database_error(database_path: &Path, operation: &str, error: impl std::fmt::Display) -> String {
    format!(
        "trace storage {operation} failed for '{}': {error}",
        database_path.display()
    )
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TraceStorageStatus {
    pub archive: String,
    pub completeness: String,
    pub known_dropped_spans: u64,
    pub physical_bytes: u64,
    pub max_disk_bytes: u64,
    pub last_error: Option<String>,
}

/// Projection used by `engine::traces::group_by`. Keeping this separate from
/// `StoredSpan` avoids decoding events, links and attributes JSON when the
/// aggregation only needs one grouping attribute plus a few indexed columns.
#[derive(Debug, Clone)]
pub(crate) struct TraceGroupRow {
    pub trace_id: String,
    pub span_id: String,
    pub value: String,
    pub label: Option<String>,
    pub start_time_ns: u64,
    pub end_time_ns: u64,
    pub pending: bool,
    pub status: String,
    pub is_internal: bool,
}

impl TraceGroupRow {
    pub fn effective_end_ns(&self, now_ns: u64) -> u64 {
        if self.pending {
            now_ns
        } else {
            self.end_time_ns
        }
    }
}

/// Identity and list order of one trace root, read without its payload: what
/// `engine::traces::list` sorts, dedupes and filters on before it reads the
/// spans of the page it returns.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RootSpanKey {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub service_name: String,
    pub start_time_ns: u64,
    pub is_internal: bool,
}

/// A trace tag attribute with just enough ordering metadata to reconstruct
/// the existing "newest span wins" rule without deserializing span payloads.
#[derive(Debug, Clone)]
pub(crate) struct TraceTagRow {
    pub trace_id: String,
    pub span_id: String,
    pub start_time_ns: u64,
    pub ordinal: usize,
    pub key: String,
    pub value: String,
}

enum ControlMessage {
    Flush {
        response: mpsc::Sender<Result<(), String>>,
    },
    Clear {
        response: mpsc::Sender<Result<(), String>>,
    },
    Retain {
        response: mpsc::Sender<Result<(), String>>,
    },
    Shutdown {
        response: mpsc::Sender<Result<(), String>>,
    },
}

struct RuntimeState {
    degraded: bool,
    last_error: Option<String>,
    /// Spans lost before reaching the archive: persisted plus
    /// `unpersisted_dropped`, which the writer adds to SQLite in its next
    /// transaction (or before answering a flush).
    known_dropped_spans: u64,
    unpersisted_dropped: u64,
    unclean_shutdown: bool,
    warning_at: Option<Instant>,
    suppressed_warnings: u64,
    warnings_emitted: u64,
}

#[derive(Clone, Copy)]
struct CapacityLimits {
    max_bytes: u64,
    tolerance: u64,
    /// `max_bytes + tolerance`: no committed image may exceed it.
    threshold: u64,
}

/// A bounded, asynchronous SQLite archive for finalized spans.
pub struct TraceDiskStore {
    directory: PathBuf,
    database_path: PathBuf,
    max_disk_bytes: AtomicU64,
    retention_seconds: AtomicU64,
    epoch: AtomicU64,
    wake_tx: SyncSender<()>,
    control_tx: mpsc::Sender<ControlMessage>,
    hot: RwLock<Option<Weak<InMemorySpanStorage>>>,
    state: Mutex<RuntimeState>,
    stop: AtomicBool,
    /// Bumped by `update_limits`; the writer then retries deferred work.
    limits_generation: AtomicU64,
    #[cfg(test)]
    fail_full_vacuum: AtomicBool,
    #[cfg(test)]
    full_vacuum_attempts: AtomicU64,
    #[cfg(test)]
    scratch_dir_override: Mutex<Option<PathBuf>>,
    #[cfg(test)]
    scratch_measurements: AtomicU64,
}

const CAPACITY_WARNING_INTERVAL: Duration = Duration::from_secs(60);

/// Return the number of suppressed warnings represented by this log line.
/// `None` means the caller should count this event but emit no log.
fn capacity_warning_due(
    last_emitted: &mut Option<Instant>,
    suppressed: &mut u64,
    now: Instant,
) -> Option<u64> {
    if last_emitted.is_none_or(|last| now.duration_since(last) >= CAPACITY_WARNING_INTERVAL) {
        *last_emitted = Some(now);
        Some(std::mem::take(suppressed))
    } else {
        *suppressed = suppressed.saturating_add(1);
        None
    }
}

impl std::fmt::Debug for TraceDiskStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TraceDiskStore")
            .field("directory", &self.directory)
            .field("database_path", &self.database_path)
            .field(
                "max_disk_bytes",
                &self.max_disk_bytes.load(Ordering::Relaxed),
            )
            .field(
                "retention_seconds",
                &self.retention_seconds.load(Ordering::Relaxed),
            )
            .finish()
    }
}

impl TraceDiskStore {
    pub fn open(config: &TraceStorageConfig) -> Result<Arc<Self>, String> {
        let directory = PathBuf::from(&config.directory);
        fs::create_dir_all(&directory).map_err(|err| {
            format!(
                "cannot create trace storage directory '{}': {err}",
                directory.display()
            )
        })?;
        set_private_permissions(&directory, true)?;

        let database_path = directory.join("traces.sqlite3");
        let mut connection = Connection::open(&database_path).map_err(|err| {
            format!(
                "cannot open trace database '{}': {err}",
                database_path.display()
            )
        })?;
        configure_connection(&mut connection)
            .map_err(|err| database_error(&database_path, "initialization", err))?;
        ensure_incremental_vacuum(&mut connection, &database_path)?;
        set_private_permissions(&database_path, false)?;

        let epoch = read_epoch(&connection).unwrap_or(0);
        let unclean_shutdown = !read_clean_shutdown(&connection).unwrap_or(false);
        let (wake_tx, wake_rx) = mpsc::sync_channel(1);
        let (control_tx, control_rx) = mpsc::channel();
        let store = Arc::new(Self {
            directory,
            database_path,
            max_disk_bytes: AtomicU64::new(config.max_disk_bytes),
            retention_seconds: AtomicU64::new(config.retention_seconds),
            epoch: AtomicU64::new(epoch),
            wake_tx,
            control_tx,
            hot: RwLock::new(None),
            state: Mutex::new(RuntimeState {
                degraded: false,
                last_error: None,
                known_dropped_spans: read_dropped_spans(&connection).unwrap_or(0),
                unpersisted_dropped: 0,
                unclean_shutdown,
                warning_at: None,
                suppressed_warnings: 0,
                warnings_emitted: 0,
            }),
            stop: AtomicBool::new(false),
            limits_generation: AtomicU64::new(0),
            #[cfg(test)]
            fail_full_vacuum: AtomicBool::new(false),
            #[cfg(test)]
            full_vacuum_attempts: AtomicU64::new(0),
            #[cfg(test)]
            scratch_dir_override: Mutex::new(None),
            #[cfg(test)]
            scratch_measurements: AtomicU64::new(0),
        });

        set_clean_shutdown(&connection, false)
            .map_err(|err| database_error(&store.database_path, "activation", err))?;

        let weak = Arc::downgrade(&store);
        thread::Builder::new()
            .name("iii-trace-store".to_string())
            .spawn(move || writer_loop(weak, connection, wake_rx, control_rx))
            .map_err(|err| format!("cannot start trace writer: {err}"))?;

        Ok(store)
    }

    pub fn attach_hot_storage(&self, hot: &Arc<InMemorySpanStorage>) {
        *self
            .hot
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::downgrade(hot));
        self.notify();
    }

    pub fn notify(&self) {
        let _ = self.wake_tx.try_send(());
    }

    /// Explicit durability boundary, used by tests and lifecycle code instead
    /// of waiting for the periodic writer tick. `Ok` means every finalized
    /// span ingested before the call is durable or was dropped as
    /// irrecoverable and counted in `known_dropped_spans`. `Err` means some of
    /// them are still dirty: deferred by capacity or a storage error (archive
    /// degraded), or the writer's 4 s budget ran out with backlog left
    /// (progress made, archive not degraded). Later spans are left to the
    /// periodic writer.
    pub fn flush(&self) -> Result<(), String> {
        let (response_tx, response_rx) = mpsc::channel();
        self.control_tx
            .send(ControlMessage::Flush {
                response: response_tx,
            })
            .map_err(|_| "trace writer is not running".to_string())?;
        let _ = self.wake_tx.try_send(());
        response_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "timed out flushing trace archive".to_string())?
    }

    pub fn update_limits(&self, config: &TraceStorageConfig) {
        self.max_disk_bytes
            .store(config.max_disk_bytes, Ordering::Relaxed);
        self.retention_seconds
            .store(config.retention_seconds, Ordering::Relaxed);
        self.limits_generation.fetch_add(1, Ordering::AcqRel);
        self.notify();
    }

    fn capacity_limits(&self) -> CapacityLimits {
        let max_bytes = self.max_disk_bytes.load(Ordering::Acquire);
        let tolerance = (max_bytes / 100).max(8 * 1024 * 1024);
        CapacityLimits {
            max_bytes,
            tolerance,
            threshold: max_bytes.saturating_add(tolerance),
        }
    }

    /// The `-shm` index SQLite keeps beside the database while it is open:
    /// always on disk, so it counts against the cap like the pages do.
    fn sidecar_bytes(&self) -> u64 {
        let mut path = self.database_path.clone().into_os_string();
        path.push("-shm");
        fs::metadata(PathBuf::from(path)).map_or(0, |metadata| metadata.len())
    }

    /// Where size measurements run: the system temporary directory.
    fn scratch_directory(&self) -> PathBuf {
        #[cfg(test)]
        if let Some(directory) = self
            .scratch_dir_override
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            return directory;
        }
        std::env::temp_dir()
    }

    fn hot_storage(&self) -> Option<Arc<InMemorySpanStorage>> {
        self.hot
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .and_then(Weak::upgrade)
    }

    pub fn get_spans(&self) -> Result<Vec<StoredSpan>, String> {
        self.read_spans(None)
    }

    /// Read a chronological archive window without decoding rows outside the
    /// requested page. Callers merge the page with the hot cache afterwards.
    pub fn get_spans_page_by_start_time(
        &self,
        offset: usize,
        limit: usize,
        ascending: bool,
    ) -> Result<Vec<StoredSpan>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| database_error(&self.database_path, "read open", err))?;
        configure_read_connection(&mut connection)
            .map_err(|err| database_error(&self.database_path, "read initialization", err))?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let order = if ascending { "ASC" } else { "DESC" };
        let query = format!(
            "SELECT payload FROM spans WHERE epoch = ?1\n             ORDER BY start_time_ns {order}, trace_id {order}, span_id {order}\n             LIMIT ?2 OFFSET ?3"
        );
        let mut statement = connection
            .prepare(&query)
            .map_err(|err| format!("cannot prepare paged trace read: {err}"))?;
        let rows = statement
            .query_map(params![epoch, limit as i64, offset as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|err| format!("cannot query paged trace archive: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("cannot read paged trace archive row: {err}"))?;
        Ok(self.decode_payloads(rows))
    }

    pub fn span_count(&self) -> Result<usize, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for counting: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        connection
            .query_row(
                "SELECT COUNT(*) FROM spans WHERE epoch = ?1",
                params![epoch],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count.max(0) as usize)
            .map_err(|err| format!("cannot count trace archive rows: {err}"))
    }

    /// Count keys already present on disk so a hot overlay can report an exact
    /// merged total without loading archive payloads.
    pub fn existing_span_key_count(&self, keys: &[(String, String)]) -> Result<usize, String> {
        if keys.is_empty() {
            return Ok(0);
        }
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for counting: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut unique = HashSet::with_capacity(keys.len());
        let keys: Vec<_> = keys
            .iter()
            .filter(|key| unique.insert((key.0.as_str(), key.1.as_str())))
            .collect();
        let mut total = 0usize;

        // Two bind variables per key plus the epoch.
        for chunk in keys.chunks(TRACE_ID_QUERY_CHUNK / 2) {
            let conditions = std::iter::repeat_n("(trace_id = ? AND span_id = ?)", chunk.len())
                .collect::<Vec<_>>()
                .join(" OR ");
            let query = format!("SELECT COUNT(*) FROM spans WHERE epoch = ? AND ({conditions})");
            let mut statement = connection
                .prepare(&query)
                .map_err(|err| format!("cannot prepare trace key count: {err}"))?;
            let mut parameters: Vec<&dyn ToSql> = Vec::with_capacity(chunk.len() * 2 + 1);
            parameters.push(&epoch);
            for (trace_id, span_id) in chunk {
                parameters.push(trace_id as &dyn ToSql);
                parameters.push(span_id as &dyn ToSql);
            }
            let count = statement
                .query_row(params_from_iter(parameters), |row| row.get::<_, i64>(0))
                .map_err(|err| format!("cannot count trace archive keys: {err}"))?;
            total = total.saturating_add(count.max(0) as usize);
        }

        Ok(total)
    }

    pub fn get_root_spans(&self) -> Result<Vec<StoredSpan>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for reading: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let query = format!(
            "SELECT s.payload FROM spans s
             WHERE s.epoch = ?1
               AND {ROOT_SPAN_PREDICATE_SQL}
             ORDER BY s.start_time_ns ASC, s.trace_id ASC, s.span_id ASC"
        );
        let mut statement = connection
            .prepare(&query)
            .map_err(|err| format!("cannot prepare root trace read: {err}"))?;
        let rows = statement
            .query_map(params![epoch], |row| row.get::<_, String>(0))
            .map_err(|err| format!("cannot query root trace archive: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("cannot read root trace archive row: {err}"))?;

        Ok(self.decode_payloads(rows))
    }

    /// Every archived root's key, with root and internal detection in SQL.
    /// Selects only columns stored ahead of `payload`, so no JSON is decoded
    /// and no payload overflow page is read: the cost tracks the number of
    /// roots, not the size of the history.
    pub(crate) fn get_root_span_keys(&self) -> Result<Vec<RootSpanKey>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| database_error(&self.database_path, "root keys open", err))?;
        configure_read_connection(&mut connection)
            .map_err(|err| database_error(&self.database_path, "root keys initialization", err))?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let query = format!(
            "SELECT s.trace_id, s.span_id, s.parent_span_id, s.name, s.service_name,
                    s.start_time_ns, {INTERNAL_SPAN_PREDICATE_SQL} AS is_internal
             FROM spans s
             WHERE s.epoch = ?1
               AND {ROOT_SPAN_PREDICATE_SQL}"
        );
        let mut statement = connection
            .prepare(&query)
            .map_err(|err| format!("cannot prepare root key read: {err}"))?;
        statement
            .query_map(params![epoch], |row| {
                Ok(RootSpanKey {
                    trace_id: row.get(0)?,
                    span_id: row.get(1)?,
                    parent_span_id: row.get(2)?,
                    name: row.get(3)?,
                    service_name: row.get(4)?,
                    start_time_ns: row.get::<_, i64>(5)?.max(0) as u64,
                    is_internal: row.get::<_, i64>(6)? != 0,
                })
            })
            .map_err(|err| format!("cannot query root keys: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("cannot read root key row: {err}"))
    }

    /// `(trace_id, span_id)` of every archived span carrying `key = value`.
    /// `span_attributes_lookup_idx` answers it without reading a payload.
    pub(crate) fn span_keys_with_attribute(
        &self,
        key: &str,
        value: &str,
    ) -> Result<HashSet<(String, String)>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| database_error(&self.database_path, "attribute lookup open", err))?;
        configure_read_connection(&mut connection).map_err(|err| {
            database_error(&self.database_path, "attribute lookup initialization", err)
        })?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut statement = connection
            .prepare(
                "SELECT trace_id, span_id FROM span_attributes
                 WHERE epoch = ?1 AND key = ?2 AND value = ?3",
            )
            .map_err(|err| format!("cannot prepare attribute lookup: {err}"))?;
        statement
            .query_map(params![epoch, key, value], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| format!("cannot query attribute lookup: {err}"))?
            .collect::<Result<HashSet<_>, _>>()
            .map_err(|err| format!("cannot read attribute lookup row: {err}"))
    }

    /// Trace ids of the archived spans whose name satisfies `matches`, read
    /// from the `name` column ahead of `payload`, so no JSON is decoded and no
    /// overflow page is read. Each distinct name is tested once.
    pub(crate) fn trace_ids_with_span_name(
        &self,
        matches: impl Fn(&str) -> bool,
    ) -> Result<HashSet<String>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| database_error(&self.database_path, "span name scan open", err))?;
        configure_read_connection(&mut connection).map_err(|err| {
            database_error(&self.database_path, "span name scan initialization", err)
        })?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut statement = connection
            .prepare("SELECT trace_id, name FROM spans WHERE epoch = ?1")
            .map_err(|err| format!("cannot prepare span name scan: {err}"))?;
        let mut rows = statement
            .query(params![epoch])
            .map_err(|err| format!("cannot query span name scan: {err}"))?;
        let mut verdicts = HashMap::<String, bool>::new();
        let mut trace_ids = HashSet::new();
        while let Some(row) = rows
            .next()
            .map_err(|err| format!("cannot read span name scan row: {err}"))?
        {
            let name = row
                .get_ref(1)
                .and_then(|value| value.as_str().map_err(Into::into))
                .map_err(|err| format!("cannot read span name: {err}"))?;
            let matched = match verdicts.get(name) {
                Some(matched) => *matched,
                None => {
                    let matched = matches(name);
                    verdicts.insert(name.to_string(), matched);
                    matched
                }
            };
            if matched {
                trace_ids.insert(
                    row.get::<_, String>(0)
                        .map_err(|err| format!("cannot read span name trace id: {err}"))?,
                );
            }
        }
        Ok(trace_ids)
    }

    /// Read a chronological window of trace roots without decoding rows
    /// outside the window. Root and internal filtering happen in SQL so the
    /// default list view never materializes the full archive; callers merge
    /// the window with the hot cache afterwards.
    pub fn get_root_spans_page_by_start_time(
        &self,
        offset: usize,
        limit: usize,
        ascending: bool,
        include_internal: bool,
    ) -> Result<Vec<StoredSpan>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| database_error(&self.database_path, "root page open", err))?;
        configure_read_connection(&mut connection)
            .map_err(|err| database_error(&self.database_path, "root page initialization", err))?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let order = if ascending { "ASC" } else { "DESC" };
        let query = format!(
            "SELECT s.payload FROM spans s
             WHERE s.epoch = ?1
               AND {ROOT_SPAN_PREDICATE_SQL}
               AND (?2 OR NOT {INTERNAL_SPAN_PREDICATE_SQL})
             ORDER BY s.start_time_ns {order}, s.trace_id {order}, s.span_id {order}
             LIMIT ?3 OFFSET ?4"
        );
        let mut statement = connection
            .prepare(&query)
            .map_err(|err| format!("cannot prepare paged root trace read: {err}"))?;
        let rows = statement
            .query_map(
                params![epoch, include_internal, limit as i64, offset as i64],
                |row| row.get::<_, String>(0),
            )
            .map_err(|err| format!("cannot query paged root trace archive: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("cannot read paged root trace archive row: {err}"))?;
        Ok(self.decode_payloads(rows))
    }

    /// Count trace roots without decoding any payload.
    pub fn root_span_count(&self, include_internal: bool) -> Result<usize, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for counting: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let query = format!(
            "SELECT COUNT(*) FROM spans s
             WHERE s.epoch = ?1
               AND {ROOT_SPAN_PREDICATE_SQL}
               AND (?2 OR NOT {INTERNAL_SPAN_PREDICATE_SQL})"
        );
        connection
            .query_row(&query, params![epoch, include_internal], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count.max(0) as usize)
            .map_err(|err| format!("cannot count trace archive roots: {err}"))
    }

    /// Count keys that `root_span_count` counted and a hot overlay shadows,
    /// so the merged root total can subtract them without decoding payloads.
    pub fn existing_root_span_key_count(
        &self,
        keys: &[(String, String)],
        include_internal: bool,
    ) -> Result<usize, String> {
        if keys.is_empty() {
            return Ok(0);
        }
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for counting: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut unique = HashSet::with_capacity(keys.len());
        let keys: Vec<_> = keys
            .iter()
            .filter(|key| unique.insert((key.0.as_str(), key.1.as_str())))
            .collect();
        let mut total = 0usize;

        // Two bind variables per key plus the epoch and the internal flag.
        for chunk in keys.chunks(TRACE_ID_QUERY_CHUNK / 2) {
            let conditions = std::iter::repeat_n("(s.trace_id = ? AND s.span_id = ?)", chunk.len())
                .collect::<Vec<_>>()
                .join(" OR ");
            let query = format!(
                "SELECT COUNT(*) FROM spans s
                 WHERE s.epoch = ?
                   AND {ROOT_SPAN_PREDICATE_SQL}
                   AND (? OR NOT {INTERNAL_SPAN_PREDICATE_SQL})
                   AND ({conditions})"
            );
            let mut statement = connection
                .prepare(&query)
                .map_err(|err| format!("cannot prepare root key count: {err}"))?;
            let mut parameters: Vec<&dyn ToSql> = Vec::with_capacity(chunk.len() * 2 + 2);
            parameters.push(&epoch);
            parameters.push(&include_internal);
            for (trace_id, span_id) in chunk {
                parameters.push(trace_id as &dyn ToSql);
                parameters.push(span_id as &dyn ToSql);
            }
            let count = statement
                .query_row(params_from_iter(parameters), |row| row.get::<_, i64>(0))
                .map_err(|err| format!("cannot count trace archive root keys: {err}"))?;
            total = total.saturating_add(count.max(0) as usize);
        }

        Ok(total)
    }

    /// Return which of the given keys exist on disk. Used to decide whether a
    /// hot span's parent lives in the archive (and thus the hot span is not a
    /// root) without decoding any payload.
    pub fn existing_span_keys(
        &self,
        keys: &[(String, String)],
    ) -> Result<HashSet<(String, String)>, String> {
        if keys.is_empty() {
            return Ok(HashSet::new());
        }
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for key lookup: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut unique = HashSet::with_capacity(keys.len());
        let keys: Vec<_> = keys
            .iter()
            .filter(|key| unique.insert((key.0.as_str(), key.1.as_str())))
            .collect();
        let mut present = HashSet::new();

        for chunk in keys.chunks(TRACE_ID_QUERY_CHUNK / 2) {
            let conditions = std::iter::repeat_n("(trace_id = ? AND span_id = ?)", chunk.len())
                .collect::<Vec<_>>()
                .join(" OR ");
            let query =
                format!("SELECT trace_id, span_id FROM spans WHERE epoch = ? AND ({conditions})");
            let mut statement = connection
                .prepare(&query)
                .map_err(|err| format!("cannot prepare span key lookup: {err}"))?;
            let mut parameters: Vec<&dyn ToSql> = Vec::with_capacity(chunk.len() * 2 + 1);
            parameters.push(&epoch);
            for (trace_id, span_id) in chunk {
                parameters.push(trace_id as &dyn ToSql);
                parameters.push(span_id as &dyn ToSql);
            }
            let chunk_keys = statement
                .query_map(params_from_iter(parameters), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|err| format!("cannot query span key lookup: {err}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| format!("cannot read span key lookup row: {err}"))?;
            present.extend(chunk_keys);
        }

        Ok(present)
    }

    pub fn get_spans_by_trace_id(&self, trace_id: &str) -> Result<Vec<StoredSpan>, String> {
        self.read_spans(Some(trace_id))
    }

    /// Read several traces with one SQLite connection per bounded chunk. This
    /// is used by list/tag enrichment and grouped expansions; one query per
    /// trace turns a 500-row page into hundreds of connection opens and JSON
    /// decodes.
    pub fn get_spans_by_trace_ids(&self, trace_ids: &[String]) -> Result<Vec<StoredSpan>, String> {
        if trace_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut unique_ids = HashSet::with_capacity(trace_ids.len());
        let trace_ids: Vec<&str> = trace_ids
            .iter()
            .filter_map(|id| unique_ids.insert(id.as_str()).then_some(id.as_str()))
            .collect();

        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for reading: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut rows = Vec::new();

        for chunk in trace_ids.chunks(TRACE_ID_QUERY_CHUNK) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let query = format!(
                "SELECT payload FROM spans WHERE epoch = ? AND trace_id IN ({placeholders})\n                 ORDER BY start_time_ns ASC, trace_id ASC, span_id ASC"
            );
            let mut statement = connection
                .prepare(&query)
                .map_err(|err| format!("cannot prepare batched trace read: {err}"))?;
            let mut parameters: Vec<&dyn ToSql> = Vec::with_capacity(chunk.len() + 1);
            parameters.push(&epoch);
            parameters.extend(chunk.iter().map(|id| id as &dyn ToSql));
            let chunk_rows = statement
                .query_map(params_from_iter(parameters), |row| row.get::<_, String>(0))
                .map_err(|err| format!("cannot query batched trace archive: {err}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| format!("cannot read batched trace archive row: {err}"))?;
            rows.extend(chunk_rows);
        }

        Ok(self.decode_payloads(rows))
    }

    /// Read every trace-level tag for a set of traces in bounded SQLite
    /// queries. This is deliberately narrower than `get_spans_by_trace_ids`:
    /// list rows need the union of `iii.tag.*` and identity attributes, not
    /// each span's events, links and resource payload.
    pub(crate) fn get_trace_tag_rows_by_trace_ids(
        &self,
        trace_ids: &[String],
    ) -> Result<Vec<TraceTagRow>, String> {
        if trace_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut unique_ids = HashSet::with_capacity(trace_ids.len());
        let trace_ids: Vec<&str> = trace_ids
            .iter()
            .filter_map(|id| unique_ids.insert(id.as_str()).then_some(id.as_str()))
            .collect();

        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for tag reading: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut tags = Vec::new();

        for chunk in trace_ids.chunks(TRACE_ID_QUERY_CHUNK) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let query = format!(
                "SELECT s.trace_id, s.span_id, s.start_time_ns, a.ordinal, a.key, a.value
                 FROM spans s
                 INNER JOIN span_attributes a
                   ON a.epoch = s.epoch
                  AND a.trace_id = s.trace_id
                  AND a.span_id = s.span_id
                 WHERE s.epoch = ?
                   AND s.trace_id IN ({placeholders})
                   AND (a.key LIKE 'iii.tag.%' OR a.key IN ('iii.session.id', 'iii.session.name', 'iii.message.id'))
                 ORDER BY s.trace_id ASC, s.start_time_ns ASC, s.span_id ASC, a.ordinal ASC"
            );
            let mut statement = connection
                .prepare(&query)
                .map_err(|err| format!("cannot prepare batched trace tag read: {err}"))?;
            let mut parameters: Vec<&dyn ToSql> = Vec::with_capacity(chunk.len() + 1);
            parameters.push(&epoch);
            parameters.extend(chunk.iter().map(|id| id as &dyn ToSql));
            let chunk_tags = statement
                .query_map(params_from_iter(parameters), |row| {
                    Ok(TraceTagRow {
                        trace_id: row.get(0)?,
                        span_id: row.get(1)?,
                        start_time_ns: row.get::<_, i64>(2)?.max(0) as u64,
                        ordinal: row.get::<_, i64>(3)?.max(0) as usize,
                        key: row.get(4)?,
                        value: row.get(5)?,
                    })
                })
                .map_err(|err| format!("cannot query batched trace tags: {err}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| format!("cannot read batched trace tag row: {err}"))?;
            tags.extend(chunk_tags);
        }

        Ok(tags)
    }

    pub fn get_spans_by_attribute(&self, attribute: &str) -> Result<Vec<StoredSpan>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for reading: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT s.payload FROM spans s
                 INNER JOIN span_attributes a
                   ON a.epoch = s.epoch
                  AND a.trace_id = s.trace_id
                  AND a.span_id = s.span_id
                 WHERE s.epoch = ?1 AND a.key = ?2
                 ORDER BY s.start_time_ns ASC, s.trace_id ASC, s.span_id ASC",
            )
            .map_err(|err| format!("cannot prepare attribute trace read: {err}"))?;
        let rows = statement
            .query_map(params![epoch, attribute], |row| row.get::<_, String>(0))
            .map_err(|err| format!("cannot query attribute trace archive: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("cannot read attribute trace archive row: {err}"))?;

        Ok(self.decode_payloads(rows))
    }

    /// Return the narrow projection required for grouping an archive by an
    /// attribute. The `span_attributes_lookup_idx` drives the predicate and
    /// avoids decoding the complete `payload` column for every match.
    pub(crate) fn get_group_rows_by_attribute(
        &self,
        attribute: &str,
        label_attribute: Option<&str>,
    ) -> Result<Vec<TraceGroupRow>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for grouping: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let query = format!(
            "SELECT
                s.trace_id,
                s.span_id,
                (SELECT a.value FROM span_attributes a
                 WHERE a.epoch = s.epoch
                   AND a.trace_id = s.trace_id
                   AND a.span_id = s.span_id
                   AND a.key = ?2
                 ORDER BY a.ordinal ASC
                 LIMIT 1) AS group_value,
                (SELECT a.value FROM span_attributes a
                 WHERE a.epoch = s.epoch
                   AND a.trace_id = s.trace_id
                   AND a.span_id = s.span_id
                   AND a.key = ?3
                 ORDER BY a.ordinal ASC
                 LIMIT 1) AS label_value,
                s.start_time_ns,
                s.end_time_ns,
                s.pending,
                s.status,
                {INTERNAL_SPAN_PREDICATE_SQL} AS is_internal
             FROM spans s
             WHERE s.epoch = ?1
               AND EXISTS (
                   SELECT 1 FROM span_attributes grouped
                   WHERE grouped.epoch = s.epoch
                     AND grouped.trace_id = s.trace_id
                     AND grouped.span_id = s.span_id
                     AND grouped.key = ?2
               )"
        );
        let mut statement = connection
            .prepare(&query)
            .map_err(|err| format!("cannot prepare grouped trace read: {err}"))?;
        let rows = statement
            .query_map(params![epoch, attribute, label_attribute], |row| {
                Ok(TraceGroupRow {
                    trace_id: row.get(0)?,
                    span_id: row.get(1)?,
                    value: row.get(2)?,
                    label: row.get(3)?,
                    start_time_ns: row.get::<_, i64>(4)? as u64,
                    end_time_ns: row.get::<_, i64>(5)? as u64,
                    pending: row.get::<_, i64>(6)? != 0,
                    status: row.get(7)?,
                    is_internal: row.get::<_, i64>(8)? != 0,
                })
            })
            .map_err(|err| format!("cannot query grouped trace archive: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("cannot read grouped trace archive row: {err}"))?;

        Ok(rows)
    }

    pub fn clear(&self) -> Result<(), String> {
        let (response_tx, response_rx) = mpsc::channel();
        self.control_tx
            .send(ControlMessage::Clear {
                response: response_tx,
            })
            .map_err(|_| "trace writer is not running".to_string())?;
        response_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "timed out clearing trace archive".to_string())?
    }

    pub fn retain(&self) -> Result<(), String> {
        let (response_tx, response_rx) = mpsc::channel();
        self.control_tx
            .send(ControlMessage::Retain {
                response: response_tx,
            })
            .map_err(|_| "trace writer is not running".to_string())?;
        response_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "timed out retaining trace archive".to_string())?
    }

    pub fn shutdown(&self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        let (response_tx, response_rx) = mpsc::channel();
        let _ = self.control_tx.send(ControlMessage::Shutdown {
            response: response_tx,
        });
        let _ = self.wake_tx.try_send(());
        if let Ok(Err(error)) = response_rx.recv_timeout(Duration::from_secs(5)) {
            self.record_error(error);
        }
    }

    pub fn status(&self) -> TraceStorageStatus {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let physical_bytes = directory_size(&self.directory).unwrap_or(0);
        TraceStorageStatus {
            archive: if state.degraded {
                "degraded".to_string()
            } else {
                "healthy".to_string()
            },
            completeness: if state.known_dropped_spans > 0 {
                "partial".to_string()
            } else if state.unclean_shutdown {
                "unknown".to_string()
            } else {
                "complete".to_string()
            },
            known_dropped_spans: state.known_dropped_spans,
            physical_bytes,
            max_disk_bytes: self.max_disk_bytes.load(Ordering::Relaxed),
            last_error: state.last_error.clone(),
        }
    }

    fn read_spans(&self, trace_id: Option<&str>) -> Result<Vec<StoredSpan>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for reading: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut statement = if trace_id.is_some() {
            connection
                .prepare(
                    "SELECT payload FROM spans WHERE epoch = ?1 AND trace_id = ?2\n                     ORDER BY start_time_ns ASC, span_id ASC",
                )
                .map_err(|err| database_error(&self.database_path, "read preparation", err))?
        } else {
            connection
                .prepare(
                    "SELECT payload FROM spans WHERE epoch = ?1\n                     ORDER BY start_time_ns ASC, trace_id ASC, span_id ASC",
                )
                .map_err(|err| database_error(&self.database_path, "read preparation", err))?
        };

        let rows = if let Some(trace_id) = trace_id {
            statement
                .query_map(params![epoch, trace_id], |row| row.get::<_, String>(0))
                .map_err(|err| database_error(&self.database_path, "read query", err))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| database_error(&self.database_path, "read row", err))?
        } else {
            statement
                .query_map(params![epoch], |row| row.get::<_, String>(0))
                .map_err(|err| database_error(&self.database_path, "read query", err))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| database_error(&self.database_path, "read row", err))?
        };

        Ok(self.decode_payloads(rows))
    }

    fn decode_payloads(&self, rows: Vec<String>) -> Vec<StoredSpan> {
        let mut spans = Vec::with_capacity(rows.len());
        for payload in rows {
            match serde_json::from_str::<StoredSpan>(&payload) {
                Ok(span) => spans.push(span),
                Err(err) => {
                    self.record_error(format!("corrupt trace payload skipped: {err}"));
                }
            }
        }
        spans
    }

    fn record_error(&self, error: String) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.degraded = true;
        state.last_error = Some(error);
    }

    fn record_success(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.degraded = false;
        state.last_error = None;
    }

    pub fn mark_degraded(&self, error: impl Into<String>) {
        self.record_error(error.into());
    }

    fn record_capacity_error(&self, error: String) {
        self.warn_limited("capacity", &error);
        self.record_error(error);
    }

    /// Every writer warning (capacity, irrecoverable drops, VACUUM fallback
    /// failures) shares one limiter: at most one line per minute per archive,
    /// carrying how many were suppressed since the previous line.
    fn warn_limited(&self, event: &'static str, detail: &str) {
        let suppressed = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let RuntimeState {
                warning_at,
                suppressed_warnings,
                warnings_emitted,
                ..
            } = &mut *state;
            let due = capacity_warning_due(warning_at, suppressed_warnings, Instant::now());
            if due.is_some() {
                *warnings_emitted = warnings_emitted.saturating_add(1);
            }
            due
        };
        if let Some(suppressed) = suppressed {
            tracing::warn!(
                event,
                detail,
                suppressed_warnings = suppressed,
                "trace archive problem; further archive warnings are limited to one per minute"
            );
        }
    }

    /// Count spans lost before reaching the archive (hot-cache eviction of
    /// unwritten finals). Visible in `status` at once; the writer persists the
    /// delta in its next transaction.
    pub(crate) fn record_dropped_spans(&self, count: u64) {
        if count == 0 {
            return;
        }
        self.add_dropped_spans(count);
        self.notify();
    }

    fn add_dropped_spans(&self, count: u64) {
        if count == 0 {
            return;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.known_dropped_spans = state.known_dropped_spans.saturating_add(count);
        state.unpersisted_dropped = state.unpersisted_dropped.saturating_add(count);
    }

    fn unpersisted_dropped(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .unpersisted_dropped
    }

    /// Called only after the transaction that added `count` committed.
    fn confirm_dropped_persisted(&self, count: u64) {
        if count == 0 {
            return;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.unpersisted_dropped = state.unpersisted_dropped.saturating_sub(count);
    }

    #[cfg(test)]
    fn warnings_emitted(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .warnings_emitted
    }

    #[cfg(test)]
    pub(crate) fn is_degraded(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .degraded
    }
}

/// Writer-thread scheduling state. Owned by the writer loop, never shared.
#[derive(Default)]
struct WriterState {
    /// Traces that fit an empty archive but not the current one, with their
    /// retry backoff. Periodic ticks skip them until due; `flush` does not.
    deferred: HashMap<String, Deferral>,
    /// Whole-archive backoff after a capacity block or a storage error.
    blocked_until: Option<Instant>,
    blocked_delay: Duration,
    last_full_vacuum: Option<Instant>,
    limits_generation: u64,
    /// Bytes still on disk once every unprotected trace was evicted, and the
    /// protected traces holding them: a measured floor for later feasibility
    /// checks, valid while all of those traces stay protected.
    held_floor: Option<HeldFloor>,
    /// The scratch measurement failed; measure in the archive until then.
    scratch_disabled_until: Option<Instant>,
}

struct HeldFloor {
    traces: HashSet<String>,
    /// Bytes in use (pages plus `-shm`) when these traces were alone.
    bytes: u64,
    /// Their attributed sizes then: later growth (or shrinkage) of the same
    /// traces is added as it is attributed (`now`).
    attributed: u64,
}

impl HeldFloor {
    /// The floor today, given what `traces` are attributed now.
    fn now(&self, attributed_now: u64) -> u64 {
        self.bytes
            .saturating_add(attributed_now)
            .saturating_sub(self.attributed)
    }
}

/// Identifies one set of dirty rows of a trace: `(rows, payload bytes,
/// lower bound)`. A measurement is reused only for the same rows.
type GroupFingerprint = (u64, u64, u64);

struct Deferral {
    until: Instant,
    delay: Duration,
    /// Real size of the trace's dirty rows from the scratch database or a
    /// rejected trial, so a retry does not measure the same rows again.
    measured: Option<(GroupFingerprint, u64)>,
}

fn next_backoff(previous: Duration, max: Duration) -> Duration {
    if previous.is_zero() {
        BACKOFF_INITIAL
    } else {
        previous.saturating_mul(2).min(max)
    }
}

impl WriterState {
    /// New limits, a clear or a retention pass can make room: retry at once.
    fn reset(&mut self) {
        self.deferred.clear();
        self.held_floor = None;
        self.unblock();
    }

    /// Retention may have made room: retry deferred traces now. Their
    /// measurements and the measured floor stay; the floor is dropped only
    /// when rows of one of its traces are deleted (`forget_floor_of`), when
    /// limits change or on `clear`.
    fn retry_deferred_now(&mut self) {
        let now = Instant::now();
        for deferral in self.deferred.values_mut() {
            deferral.until = now;
        }
        self.unblock();
    }

    /// Rows of these traces were deleted: a floor that counted them is stale.
    fn forget_floor_of<'a>(&mut self, deleted: impl IntoIterator<Item = &'a str>) {
        if let Some(floor) = &self.held_floor
            && deleted
                .into_iter()
                .any(|trace_id| floor.traces.contains(trace_id))
        {
            self.held_floor = None;
        }
    }

    fn scratch_available(&self, now: Instant) -> bool {
        self.scratch_disabled_until.is_none_or(|until| until <= now)
    }

    fn observe_limits(&mut self, store: &TraceDiskStore) {
        let generation = store.limits_generation.load(Ordering::Acquire);
        if generation != self.limits_generation {
            self.limits_generation = generation;
            self.reset();
        }
    }

    fn block(&mut self, now: Instant) {
        self.blocked_delay = next_backoff(self.blocked_delay, BLOCKED_BACKOFF_MAX);
        self.blocked_until = Some(now + self.blocked_delay);
    }

    fn unblock(&mut self) {
        self.blocked_until = None;
        self.blocked_delay = Duration::ZERO;
    }

    fn is_blocked(&self, now: Instant) -> bool {
        self.blocked_until.is_some_and(|until| until > now)
    }

    fn deferral_entry(&mut self, trace_id: &str, now: Instant) -> &mut Deferral {
        if self.deferred.len() >= MAX_DEFERRED_TRACES && !self.deferred.contains_key(trace_id) {
            self.deferred.retain(|_, deferral| deferral.until > now);
            if self.deferred.len() >= MAX_DEFERRED_TRACES {
                self.deferred.clear();
            }
        }
        self.deferred
            .entry(trace_id.to_string())
            .or_insert(Deferral {
                until: now,
                delay: Duration::ZERO,
                measured: None,
            })
    }

    fn defer_trace(&mut self, trace_id: &str, now: Instant) {
        let deferral = self.deferral_entry(trace_id, now);
        deferral.delay = next_backoff(deferral.delay, TRACE_BACKOFF_MAX);
        deferral.until = now + deferral.delay;
    }

    fn remember_measurement(&mut self, trace_id: &str, fingerprint: GroupFingerprint, bytes: u64) {
        self.deferral_entry(trace_id, Instant::now()).measured = Some((fingerprint, bytes));
    }

    fn measurement(&self, trace_id: &str, fingerprint: GroupFingerprint) -> Option<u64> {
        self.deferred
            .get(trace_id)?
            .measured
            .filter(|(measured, _)| *measured == fingerprint)
            .map(|(_, bytes)| bytes)
    }

    fn deferred_now(&self, now: Instant) -> HashSet<String> {
        self.deferred
            .iter()
            .filter(|(_, deferral)| deferral.until > now)
            .map(|(trace_id, _)| trace_id.clone())
            .collect()
    }
}

fn writer_loop(
    weak: Weak<TraceDiskStore>,
    mut connection: Connection,
    wake_rx: mpsc::Receiver<()>,
    control_rx: mpsc::Receiver<ControlMessage>,
) {
    let mut writer = WriterState::default();
    loop {
        let Some(store) = weak.upgrade() else { break };
        while let Ok(control) = control_rx.try_recv() {
            if handle_control(&store, &mut connection, &mut writer, control) {
                return;
            }
        }

        match wake_rx.recv_timeout(BATCH_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {
                archive_tick(&store, &mut connection, &mut writer);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn handle_control(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    writer: &mut WriterState,
    control: ControlMessage,
) -> bool {
    match control {
        ControlMessage::Clear { response } => {
            let result = clear_database(store, connection);
            writer.reset();
            let _ = response.send(result);
            false
        }
        ControlMessage::Flush { response } => {
            let _ = response.send(flush_archive(store, connection, writer));
            false
        }
        ControlMessage::Retain { response } => {
            writer.retry_deferred_now();
            let result = retain_database(store, connection, writer);
            let _ = response.send(result);
            false
        }
        ControlMessage::Shutdown { response } => {
            let result = flush_archive(store, connection, writer).and_then(|()| {
                set_clean_shutdown(connection, true)
                    .map(|_| ())
                    .map_err(|err| err.to_string())
            });
            let _ = response.send(result);
            true
        }
    }
}

/// Periodic write-behind pass. Running out of budget with backlog left is
/// progress, not an error: the writer wakes itself and continues right after
/// serving queued control messages.
fn archive_tick(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    writer: &mut WriterState,
) {
    match drain(store, connection, writer, DrainMode::Tick) {
        Ok(report) => {
            if let Some(error) = report.capacity {
                store.record_capacity_error(error);
            } else if let Some(error) = report.storage_error {
                store.record_error(error);
            } else if report.progressed {
                store.record_success();
            }
            if report.budget_exhausted {
                store.notify();
            }
        }
        Err(error) => store.record_error(error),
    }
}

/// The writer side of `TraceDiskStore::flush` (see its contract).
fn flush_archive(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    writer: &mut WriterState,
) -> Result<(), String> {
    let report = match drain(store, connection, writer, DrainMode::Flush) {
        Ok(report) => report,
        Err(error) => {
            store.record_error(error.clone());
            return Err(error);
        }
    };
    if let Some(error) = report.capacity {
        store.record_capacity_error(error.clone());
        return Err(error);
    }
    if let Some(error) = report.storage_error {
        store.record_error(error.clone());
        return Err(error);
    }
    if report.progressed {
        store.record_success();
    }
    if report.budget_exhausted {
        return Err(format!(
            "trace archive flush incomplete: {} s work budget exhausted with eligible spans still dirty (progress made; archive not degraded)",
            FLUSH_TIME_BUDGET.as_secs()
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DrainMode {
    /// Periodic: honors backoffs, bounded by `TICK_MAX_BATCHES` and
    /// `TICK_TIME_BUDGET`.
    Tick,
    /// Explicit: every span ingested before the call, ignoring backoffs,
    /// bounded by `FLUSH_TIME_BUDGET`.
    Flush,
}

#[derive(Default)]
struct DrainReport {
    /// At least one span was written or settled as dropped.
    progressed: bool,
    /// The budget ran out while eligible dirty spans remained.
    budget_exhausted: bool,
    /// Capacity deferred some traces or blocked the archive.
    capacity: Option<String>,
    /// A storage error deferred the traces that hit it; others went on.
    storage_error: Option<String>,
}

fn persist_dropped(store: &TraceDiskStore, connection: &Connection) -> Result<(), String> {
    let pending = store.unpersisted_dropped();
    if pending == 0 {
        return Ok(());
    }
    connection
        .execute(DROPPED_INCREMENT_SQL, params![pending as i64])
        .map_err(|err| database_error(&store.database_path, "dropped-count update", err))?;
    store.confirm_dropped_persisted(pending);
    Ok(())
}

fn drain(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    writer: &mut WriterState,
    mode: DrainMode,
) -> Result<DrainReport, String> {
    persist_dropped(store, connection)?;
    writer.observe_limits(store);
    let mut report = DrainReport::default();
    let Some(hot) = store.hot_storage() else {
        return Ok(report);
    };
    let now = Instant::now();
    let (boundary, budget, max_batches, mut skip) = match mode {
        DrainMode::Tick => {
            if writer.is_blocked(now) {
                return Ok(report);
            }
            (
                u64::MAX,
                TICK_TIME_BUDGET,
                TICK_MAX_BATCHES,
                writer.deferred_now(now),
            )
        }
        DrainMode::Flush => (
            hot.archive_watermark(),
            FLUSH_TIME_BUDGET,
            usize::MAX,
            HashSet::new(),
        ),
    };
    let started = Instant::now();
    let mut batches = 0usize;
    loop {
        if batches >= max_batches || started.elapsed() >= budget {
            report.budget_exhausted = hot.has_archive_backlog(boundary, &skip);
            break;
        }
        let batch = hot.begin_archive_batch(BATCH_MAX_SPANS, BATCH_MAX_BYTES, boundary, &skip);
        if batch.is_empty() {
            break;
        }
        batches += 1;
        let outcome = archive_batch(store, connection, writer, &hot, batch);
        // Settle every claimed seq exactly once before anything can fail.
        let lost = hot.finish_archive_batch(&outcome.durable, &outcome.dropped, &outcome.deferred);
        store.add_dropped_spans(lost);
        let now = Instant::now();
        for trace_id in &outcome.settled_traces {
            writer.deferred.remove(trace_id);
        }
        for trace_id in outcome.deferred_traces {
            writer.defer_trace(&trace_id, now);
            skip.insert(trace_id);
        }
        if let Some(reason) = &outcome.drop_reason {
            store.warn_limited("irrecoverable spans dropped from the archive", reason);
        }
        if !outcome.durable.is_empty() {
            writer.unblock();
        }
        // Writing or settling a span as dropped is progress either way.
        if !outcome.durable.is_empty() || !outcome.dropped.is_empty() {
            report.progressed = true;
        }
        // Persist the exact loss count before answering any caller.
        persist_dropped(store, connection)?;
        if let Some(error) = outcome.storage_error {
            report.storage_error.get_or_insert(error);
        }
        if let Some(error) = outcome.error {
            writer.block(now);
            return Err(error);
        }
        if let Some(blocked) = outcome.blocked {
            writer.block(now);
            report.capacity = Some(blocked);
            return Ok(report);
        }
        if let Some(capacity) = outcome.capacity {
            report.capacity = Some(capacity);
        }
    }
    Ok(report)
}

/// One trace's spans in a claimed batch: the unit of every archive decision.
struct TraceGroup {
    trace_id: String,
    /// Newest version of each span id, in arrival order.
    rows: Vec<GroupRow>,
    lower_bound: u64,
    /// Bytes reserved for the group when making room: the lower bound, or a
    /// measurement when one was taken.
    estimate: u64,
    /// Large and not measured (scratch unavailable or too large): learn its
    /// size by a trial in the archive, evicting nothing for it.
    trial_first: bool,
}

impl TraceGroup {
    fn rows(&self) -> Vec<(&StoredSpan, &str)> {
        self.rows
            .iter()
            .map(|row| (&row.span, row.payload.as_str()))
            .collect()
    }

    /// Every claimed record of the trace, superseded versions included.
    fn seqs(&self) -> impl Iterator<Item = u64> + '_ {
        self.rows.iter().flat_map(|row| row.seqs.iter().copied())
    }

    fn fingerprint(&self) -> GroupFingerprint {
        let payload_bytes = self
            .rows
            .iter()
            .map(|row| row.payload.len() as u64)
            .fold(0_u64, u64::saturating_add);
        (self.rows.len() as u64, payload_bytes, self.lower_bound)
    }

    fn measure_lower_bound(&mut self) {
        self.lower_bound = self
            .rows
            .iter()
            .map(|row| span_lower_bound(&row.span, &row.payload))
            .fold(0_u64, u64::saturating_add);
        self.estimate = self.lower_bound;
    }

    /// Move the rows from `at` on into a new group of the same trace.
    fn split_off(&mut self, at: usize) -> TraceGroup {
        let mut tail = TraceGroup {
            trace_id: self.trace_id.clone(),
            rows: self.rows.split_off(at),
            lower_bound: 0,
            estimate: 0,
            trial_first: false,
        };
        tail.measure_lower_bound();
        self.measure_lower_bound();
        tail
    }
}

/// One span to archive, with every claimed record it stands for.
struct GroupRow {
    span: StoredSpan,
    payload: String,
    seqs: Vec<u64>,
}

#[derive(Default)]
struct BatchOutcome {
    durable: Vec<u64>,
    dropped: Vec<u64>,
    deferred: Vec<u64>,
    /// Committed or dropped: any trace backoff ends.
    settled_traces: Vec<String>,
    /// Fit an empty archive but not the current one: backoff.
    deferred_traces: Vec<String>,
    drop_reason: Option<String>,
    capacity: Option<String>,
    /// Nothing can be written until protected traces finish, limits change
    /// or retention frees space.
    blocked: Option<String>,
    /// Archive-wide failure: every group is left dirty and the archive backs
    /// off.
    error: Option<String>,
    /// A storage error that only the deferred groups hit.
    storage_error: Option<String>,
}

impl BatchOutcome {
    fn defer_after_error(&mut self, group: TraceGroup, error: String) {
        self.deferred.extend(group.seqs());
        self.deferred_traces.push(group.trace_id);
        self.storage_error.get_or_insert(error);
    }

    fn commit(&mut self, group: TraceGroup) {
        self.durable.extend(group.seqs());
        self.settled_traces.push(group.trace_id);
    }

    fn drop_irrecoverable(&mut self, group: TraceGroup, reason: String) {
        self.dropped.extend(group.seqs());
        self.settled_traces.push(group.trace_id);
        self.drop_reason.get_or_insert(reason);
    }

    fn defer(&mut self, group: TraceGroup, reason: String) {
        self.deferred.extend(group.seqs());
        self.deferred_traces.push(group.trace_id);
        self.capacity.get_or_insert(reason);
    }

    /// Leave groups dirty without a per-trace backoff (the whole archive
    /// backs off instead).
    fn release(&mut self, groups: impl IntoIterator<Item = TraceGroup>) {
        for group in groups {
            self.deferred.extend(group.seqs());
        }
    }
}

/// Minimum bytes a span occupies once archived: its JSON payload plus the two
/// other copies of every attribute (attribute table and covering lookup
/// index). Page, overflow-chain and B-tree overhead come on top.
fn span_lower_bound(span: &StoredSpan, payload: &str) -> u64 {
    let attributes = span
        .attributes
        .iter()
        .map(|(key, value)| key.len().saturating_add(value.len()) as u64)
        .fold(0_u64, u64::saturating_add);
    (payload.len() as u64).saturating_add(attributes.saturating_mul(2))
}

fn group_batch(batch: Vec<(u64, StoredSpan)>, outcome: &mut BatchOutcome) -> Vec<TraceGroup> {
    let mut groups: Vec<TraceGroup> = Vec::new();
    let mut by_trace: HashMap<String, usize> = HashMap::new();
    for (seq, span) in batch {
        let payload = match serde_json::to_string(&span) {
            Ok(payload) => payload,
            Err(err) => {
                // Not representable as an archive row, now or later.
                outcome.dropped.push(seq);
                outcome.drop_reason.get_or_insert_with(|| {
                    format!("cannot serialize a span of trace {}: {err}", span.trace_id)
                });
                continue;
            }
        };
        let index = *by_trace.entry(span.trace_id.clone()).or_insert_with(|| {
            groups.push(TraceGroup {
                trace_id: span.trace_id.clone(),
                rows: Vec::new(),
                lower_bound: 0,
                estimate: 0,
                trial_first: false,
            });
            groups.len() - 1
        });
        let group = &mut groups[index];
        // The archive keeps one row per span id and the newest version wins.
        match group
            .rows
            .iter_mut()
            .find(|row| row.span.span_id == span.span_id)
        {
            Some(row) => {
                row.span = span;
                row.payload = payload;
                row.seqs.push(seq);
            }
            None => group.rows.push(GroupRow {
                span,
                payload,
                seqs: vec![seq],
            }),
        }
    }
    for group in &mut groups {
        group.measure_lower_bound();
    }
    groups
}

/// Archive one claimed batch. Archive policy: rows already durable are
/// never deleted to make room for their own trace, and spans that can never
/// fit beside them are dropped and counted, so a later span of the same trace
/// is judged on its own and archived if it fits.
///
/// 1. Irrecoverable groups are recognised before anything is evicted: by the
///    lower bound, or by a measured size (scratch database for groups large
///    relative to the cap, or the growth of an earlier rejected trial) plus
///    the measured footprint of the trace's durable rows.
/// 2. Groups that fit as estimated are written as one transaction after
///    making room. Nothing is evicted for a group that would not fit even
///    with every unprotected trace gone, nor for a group not yet measured.
/// 3. A rejected trial yields the group's real growth, which decides between
///    drop (cannot fit beside its own durable rows) and backoff; a storage
///    error defers only the groups that hit it.
fn archive_batch(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    writer: &mut WriterState,
    hot: &InMemorySpanStorage,
    batch: Vec<(u64, StoredSpan)>,
) -> BatchOutcome {
    let mut outcome = BatchOutcome::default();
    let limits = store.capacity_limits();
    let mut writable = Vec::new();
    for mut group in group_batch(batch, &mut outcome) {
        if outcome.error.is_some() {
            writable.push(group);
            continue;
        }
        match classify_group(store, connection, writer, limits, &mut group) {
            Ok(None) => writable.push(group),
            Ok(Some(reason)) => outcome.drop_irrecoverable(group, reason),
            Err(error) => {
                outcome.error = Some(error);
                writable.push(group);
            }
        }
    }
    if outcome.error.is_some() || writable.is_empty() {
        outcome.release(writable);
        return outcome;
    }

    let mut protected: HashSet<String> = writable
        .iter()
        .map(|group| group.trace_id.clone())
        .collect();
    protected.extend(hot.pending_trace_ids());
    let capacity = {
        let incoming: Vec<(&str, u64)> = writable
            .iter()
            .filter(|group| !group.trial_first)
            .map(|group| (group.trace_id.as_str(), group.estimate))
            .collect();
        ensure_capacity(store, connection, writer, &incoming, &protected)
    };
    match capacity {
        Ok(cannot_fit_now) => {
            let (later, now): (Vec<_>, Vec<_>) = writable
                .into_iter()
                .partition(|group| group.trial_first || cannot_fit_now.contains(&group.trace_id));
            if !now.is_empty() {
                let groups: Vec<&TraceGroup> = now.iter().collect();
                let trial = try_commit(store, connection, &groups);
                match trial {
                    Ok(Trial::Committed) => {
                        for group in now {
                            outcome.commit(group);
                        }
                    }
                    Ok(Trial::Rejected { growth, room }) if now.len() == 1 => {
                        for group in now {
                            judge_rejected(
                                store,
                                connection,
                                writer,
                                limits,
                                group,
                                (growth, room),
                                &mut outcome,
                            );
                        }
                    }
                    Err(error) if now.len() == 1 => {
                        for group in now {
                            outcome.defer_after_error(group, error.clone());
                        }
                    }
                    // Several traces in one transaction did not fit or hit a
                    // storage error: isolate them.
                    Ok(Trial::Rejected { .. }) | Err(_) => {
                        try_without_eviction(store, connection, writer, limits, now, &mut outcome);
                    }
                }
            }
            try_without_eviction(store, connection, writer, limits, later, &mut outcome);
        }
        Err(CapacityError::Blocked(message)) => {
            // Nothing fits now. A group whose trace already holds too much in
            // durable rows can never fit beside them: drop it, so the trace
            // stops being protected by its own batch and becomes evictable.
            // Everything else waits for the archive backoff.
            for group in writable {
                if outcome.error.is_some() {
                    outcome.release([group]);
                    continue;
                }
                match terminal_room(store, connection, &group, writer.held_floor.as_ref(), None) {
                    Ok(room) if group.estimate > room => {
                        let reason = format!(
                            "trace {}: {} bytes incoming exceed the {room} bytes it could ever use beside its own archived rows",
                            group.trace_id, group.estimate
                        );
                        outcome.drop_irrecoverable(group, reason);
                    }
                    Ok(_) => outcome.release([group]),
                    Err(error) => {
                        outcome.error = Some(error);
                        outcome.release([group]);
                    }
                }
            }
            outcome.blocked = Some(message);
        }
        Err(CapacityError::Insufficient(_)) => {
            try_without_eviction(store, connection, writer, limits, writable, &mut outcome);
        }
        Err(CapacityError::Storage(error)) => {
            outcome.error = Some(error);
            outcome.release(writable);
        }
    }
    outcome
}

fn deferred_reason(group: &TraceGroup, needed: u64, limits: CapacityLimits) -> String {
    format!(
        "trace storage limit reached: trace {} needs {needed} bytes and is deferred until the archive has room (limit {} bytes, tolerance {} bytes)",
        group.trace_id, limits.max_bytes, limits.tolerance
    )
}

/// Try each group alone as one transaction, without evicting anything. A
/// group already measured that cannot fit in the current archive is deferred
/// without writing it; an unmeasured one is measured by the trial itself.
fn try_without_eviction(
    store: &TraceDiskStore,
    connection: &mut Connection,
    writer: &mut WriterState,
    limits: CapacityLimits,
    groups: Vec<TraceGroup>,
    outcome: &mut BatchOutcome,
) {
    for group in groups {
        if outcome.error.is_some() {
            outcome.release([group]);
            continue;
        }
        if !group.trial_first {
            // Measured (or bounded below) and known not to fit in the pages
            // in use now: judge it without writing anything.
            let used = page_stats(connection)
                .map(|stats| {
                    stats
                        .pages
                        .saturating_sub(stats.free)
                        .saturating_mul(stats.page_size)
                        .saturating_add(store.sidecar_bytes())
                })
                .unwrap_or(u64::MAX);
            if used.saturating_add(group.estimate) > limits.threshold {
                let room = limits.threshold.saturating_sub(used);
                let estimate = group.estimate;
                judge_rejected(
                    store,
                    connection,
                    writer,
                    limits,
                    group,
                    (estimate, room),
                    outcome,
                );
                continue;
            }
        }
        match try_commit(store, connection, &[&group]) {
            Ok(Trial::Committed) => outcome.commit(group),
            Ok(Trial::Rejected { growth, room }) => {
                judge_rejected(
                    store,
                    connection,
                    writer,
                    limits,
                    group,
                    (growth, room),
                    outcome,
                );
            }
            Err(error) => outcome.defer_after_error(group, error),
        }
    }
}

/// `Some(reason)` when the group can never be archived beside its trace's
/// durable rows. Never evicts anything. Large groups are measured in the
/// scratch database when it is usable; the scratch is an optimization, so a
/// failure only disables it for a while and the group is measured by a trial
/// in the archive instead.
fn classify_group(
    store: &TraceDiskStore,
    connection: &Connection,
    writer: &mut WriterState,
    limits: CapacityLimits,
    group: &mut TraceGroup,
) -> Result<Option<String>, String> {
    if group.lower_bound > limits.threshold {
        if group.rows.len() > 1 {
            // Some of its spans may fit: the trial and `judge_rejected`
            // keep what does and drop only what never can.
            group.trial_first = true;
            return Ok(None);
        }
        return Ok(Some(format!(
            "trace {}: minimum archive image {} bytes exceeds limit plus tolerance {} bytes",
            group.trace_id, group.lower_bound, limits.threshold
        )));
    }
    let fingerprint = group.fingerprint();
    let measured = match writer.measurement(&group.trace_id, fingerprint) {
        Some(bytes) => bytes,
        None if group.lower_bound <= limits.max_bytes / LARGE_GROUP_DIVISOR => return Ok(None),
        None if group.lower_bound > SCRATCH_MAX_LOWER_BOUND
            || !writer.scratch_available(Instant::now()) =>
        {
            group.trial_first = true;
            return Ok(None);
        }
        None => match measure_in_scratch(store, group) {
            Ok(bytes) => {
                writer.remember_measurement(&group.trace_id, fingerprint, bytes);
                bytes
            }
            Err(error) => {
                writer.scratch_disabled_until = Some(Instant::now() + SCRATCH_RETRY_AFTER);
                store.warn_limited(
                    "trace size measurement unavailable; measuring in the archive instead",
                    &error,
                );
                group.trial_first = true;
                return Ok(None);
            }
        },
    };
    let room = terminal_room(store, connection, group, writer.held_floor.as_ref(), None)?;
    if measured > room {
        if group.rows.len() > 1 {
            group.trial_first = true;
            return Ok(None);
        }
        return Ok(Some(format!(
            "trace {}: measured image {measured} bytes exceeds the {room} bytes it could ever use beside its own archived rows",
            group.trace_id
        )));
    }
    group.estimate = measured.max(group.lower_bound);
    Ok(None)
}

fn measure_in_scratch(store: &TraceDiskStore, group: &TraceGroup) -> Result<u64, String> {
    #[cfg(test)]
    store.scratch_measurements.fetch_add(1, Ordering::Relaxed);
    isolated_image_bytes_in(&store.scratch_directory(), &group.rows())
}

/// A trial of `group` alone was rejected: its rows took `growth` bytes where
/// the archive had `room`. The room the trace could ever have beside its own
/// durable rows is `room` plus what other traces hold (`own_room`). If the
/// group fits in that, it waits for others to leave (backoff; the measurement
/// is kept for the retry). If not, its rows are split by their measured share:
/// the longest prefix that fits now is written, rows that fit beside the
/// trace's own rows but not now wait, and only the rows past that, which can
/// never fit beside them, are dropped and counted. With no other trace left
/// both rooms are the same measured space, so nothing waits forever.
/// Bounded by `JUDGE_ROUNDS` trials.
fn judge_rejected(
    store: &TraceDiskStore,
    connection: &mut Connection,
    writer: &mut WriterState,
    limits: CapacityLimits,
    mut group: TraceGroup,
    (mut growth, mut room): (u64, u64),
    outcome: &mut BatchOutcome,
) {
    for _ in 0..JUDGE_ROUNDS {
        let ever = match terminal_room(
            store,
            connection,
            &group,
            writer.held_floor.as_ref(),
            Some(room),
        ) {
            Ok(ever) => ever,
            Err(error) => {
                outcome.error = Some(error);
                outcome.release([group]);
                return;
            }
        };
        if growth <= ever {
            writer.remember_measurement(&group.trace_id, group.fingerprint(), growth);
            let reason = deferred_reason(&group, growth, limits);
            outcome.defer(group, reason);
            return;
        }
        let reason = format!(
            "trace {}: measured growth {growth} bytes exceeds the {ever} bytes it could ever use beside its own archived rows",
            group.trace_id
        );
        let (fits_own, own_growth) = fitting_prefix(&group, growth, ever);
        if fits_own == 0 {
            outcome.drop_irrecoverable(group, reason);
            return;
        }
        let never = group.split_off(fits_own);
        if !never.rows.is_empty() {
            outcome.drop_irrecoverable(never, reason);
        }
        let (fits_now, _) = fitting_prefix(&group, own_growth, room);
        if fits_now == 0 {
            let reason = deferred_reason(&group, own_growth, limits);
            outcome.defer(group, reason);
            return;
        }
        let later = group.split_off(fits_now);
        if !later.rows.is_empty() {
            let reason = deferred_reason(&later, own_growth, limits);
            outcome.defer(later, reason);
        }
        match try_commit(store, connection, &[&group]) {
            Ok(Trial::Committed) => {
                outcome.commit(group);
                return;
            }
            Ok(Trial::Rejected {
                growth: retried,
                room: now,
            }) => {
                growth = retried;
                room = now;
            }
            Err(error) => {
                outcome.defer_after_error(group, error);
                return;
            }
        }
    }
    let reason = deferred_reason(&group, growth, limits);
    outcome.defer(group, reason);
}

/// Fixed bytes of one attribute row and its two index entries beyond the
/// ids, key and value (cell headers, rowid, epoch, ordinal).
const ATTRIBUTE_ROW_OVERHEAD: u64 = 32;
/// Fixed bytes of one span row and its three index entries beyond the ids.
const SPAN_ROW_OVERHEAD: u64 = 96;

/// Relative storage weight of one span row, used only to share a commit's
/// measured page delta among the traces it wrote. Unlike the lower bound it
/// counts the per-row cost that dominates spans with many small attributes:
/// each attribute is a table row plus a primary-key and a lookup index entry,
/// each carrying the trace and span ids.
fn row_storage_weight(span: &StoredSpan, payload: &str) -> u64 {
    let trace = span.trace_id.len() as u64;
    let span_id = span.span_id.len() as u64;
    let per_attribute = 3 * trace + 2 * span_id + ATTRIBUTE_ROW_OVERHEAD;
    let attributes = span
        .attributes
        .iter()
        .map(|(key, value)| 2 * (key.len() + value.len()) as u64 + per_attribute)
        .fold(0_u64, u64::saturating_add);
    (payload.len() as u64)
        .saturating_add(attributes)
        .saturating_add(4 * (trace + span_id) + SPAN_ROW_OVERHEAD)
}

/// How many leading rows of `group` fit in `room`, sharing the measured
/// `growth` of the whole group by each row's lower bound, and their share.
fn fitting_prefix(group: &TraceGroup, growth: u64, room: u64) -> (usize, u64) {
    let total = group.lower_bound.max(1) as u128;
    let mut used = 0_u128;
    for (index, row) in group.rows.iter().enumerate() {
        let share =
            growth as u128 * span_lower_bound(&row.span, &row.payload).max(1) as u128 / total;
        if used + share > room as u128 {
            return (index, used as u64);
        }
        used += share;
    }
    (group.rows.len(), used as u64)
}

/// Bytes in use now as the cap measures them: used pages plus the -shm index.
fn used_bytes(store: &TraceDiskStore, connection: &Connection) -> Result<u64, String> {
    let stats = page_stats(connection)
        .map_err(|err| database_error(&store.database_path, "page count", err))?;
    Ok(stats
        .pages
        .saturating_sub(stats.free)
        .saturating_mul(stats.page_size)
        .saturating_add(store.sidecar_bytes()))
}

/// The most `group` could ever be given beside its trace's own durable rows,
/// as a terminal (drop) bound:
/// - exact when no other trace holds rows: the room the archive has now
///   (`room_now`, or limit plus tolerance minus the bytes in use), so a group
///   that does not fit beside its own rows is settled (N4);
/// - otherwise permissive, because the sizes of other traces are only
///   attributed (or unknown, `image_bytes = 0` in archives from older
///   versions): limit plus tolerance minus the lower bound of the trace's own
///   rows, the fixed schema pages and the -shm index. Groups below it wait,
///   the others are evicted when unprotected, and the group is judged again,
///   exactly, once they are gone;
/// - at most the measured floor allows when only this trace held rows once
///   every unprotected trace was gone: those bytes, plus the trace's
///   attributed change since, stay beside any group of it.
fn terminal_room(
    store: &TraceDiskStore,
    connection: &Connection,
    group: &TraceGroup,
    floor: Option<&HeldFloor>,
    room_now: Option<u64>,
) -> Result<u64, String> {
    let limits = store.capacity_limits();
    let epoch = store.epoch.load(Ordering::Acquire) as i64;
    let others: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM trace_meta WHERE epoch = ?1 AND trace_id <> ?2)",
            params![epoch, group.trace_id],
            |row| row.get(0),
        )
        .map_err(|err| database_error(&store.database_path, "other traces lookup", err))?;
    if !others {
        return match room_now {
            Some(room) => Ok(room),
            None => Ok(limits
                .threshold
                .saturating_sub(used_bytes(store, connection)?)),
        };
    }
    let permissive = limits
        .threshold
        .saturating_sub(own_lower_bound(store, connection, group)?)
        .saturating_sub(schema_base_bytes())
        .saturating_sub(store.sidecar_bytes());
    let Some(floor) = floor.filter(|floor| {
        floor
            .traces
            .iter()
            .all(|trace_id| *trace_id == group.trace_id)
    }) else {
        return Ok(permissive);
    };
    let attributed: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(MAX(image_bytes, approx_bytes)), 0) FROM trace_meta
             WHERE epoch = ?1 AND trace_id = ?2",
            params![epoch, group.trace_id],
            |row| row.get(0),
        )
        .map_err(|err| database_error(&store.database_path, "trace size lookup", err))?;
    Ok(permissive.min(
        limits
            .threshold
            .saturating_sub(floor.now(attributed.max(0) as u64)),
    ))
}

/// Lower bound of the trace's durable rows that `group` does not replace,
/// computed in SQL without reading payloads into memory (`length` counts
/// characters, never more than bytes).
fn own_lower_bound(
    store: &TraceDiskStore,
    connection: &Connection,
    group: &TraceGroup,
) -> Result<u64, String> {
    let epoch = store.epoch.load(Ordering::Acquire) as i64;
    let replaced: HashSet<&str> = group
        .rows
        .iter()
        .map(|row| row.span.span_id.as_str())
        .collect();
    let mut statement = connection
        .prepare_cached(
            "SELECT s.span_id, length(s.payload) + 2 * COALESCE((
                SELECT SUM(length(a.key) + length(a.value)) FROM span_attributes a
                WHERE a.epoch = s.epoch AND a.trace_id = s.trace_id AND a.span_id = s.span_id
             ), 0)
             FROM spans s WHERE s.epoch = ?1 AND s.trace_id = ?2",
        )
        .map_err(|err| database_error(&store.database_path, "archived size preparation", err))?;
    let rows = statement
        .query_map(params![epoch, group.trace_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| database_error(&store.database_path, "archived size query", err))?;
    let mut kept = 0_u64;
    for row in rows {
        let (span_id, bytes) =
            row.map_err(|err| database_error(&store.database_path, "archived size row", err))?;
        if !replaced.contains(span_id.as_str()) {
            kept = kept.saturating_add(bytes.max(0) as u64);
        }
    }
    Ok(kept)
}

/// Bytes an empty archive already occupies (schema root pages, pointer map),
/// measured once on an in-memory copy of the schema.
fn schema_base_bytes() -> u64 {
    static BASE: OnceLock<u64> = OnceLock::new();
    *BASE.get_or_init(|| {
        Connection::open_in_memory()
            .and_then(|connection| {
                connection.execute_batch("PRAGMA auto_vacuum=INCREMENTAL;")?;
                connection.execute_batch(SCHEMA_SQL)?;
                page_stats(&connection)
            })
            .map(|stats| stats.pages.saturating_mul(stats.page_size))
            .unwrap_or(64 * 1024)
    })
}

/// A private scratch file, removed on drop.
struct ScratchFile(PathBuf);

impl ScratchFile {
    fn create(path: PathBuf) -> Result<Self, String> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path).map_err(|err| {
            format!(
                "cannot create trace measurement file '{}': {err}",
                path.display()
            )
        })?;
        Ok(Self(path))
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Real SQLite footprint of `rows` in an empty archive, measured in a private
/// scratch database under the system temporary directory. The lower bound
/// misses page, overflow and index overhead (1,500 attributes of 5 KiB: bound
/// 22.9 MB, image 29.2 MB), and judging such a trace on the live archive would
/// evict unrelated history before learning it can never fit.
#[cfg(test)]
fn isolated_image_bytes(rows: &[(&StoredSpan, &str)]) -> Result<u64, String> {
    isolated_image_bytes_in(&std::env::temp_dir(), rows)
}

/// The scratch file lives outside the capped archive directory: it is mode
/// 0600, removed before returning (also on error), written with a 2 MiB page
/// cache, and holds at most one group's image. Only groups with a lower bound
/// in (cap/4, 64 MiB] are measured, once per set of rows (the result is kept
/// for retries); a failure disables the scratch for 10 minutes. On tmpfs the
/// file is memory for the duration of one measurement.
fn isolated_image_bytes_in(directory: &Path, rows: &[(&StoredSpan, &str)]) -> Result<u64, String> {
    static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);
    let path = directory.join(format!(
        "iii-trace-measure-{}-{}.sqlite3",
        std::process::id(),
        NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
    ));
    let scratch = ScratchFile::create(path)?;
    let context = |operation: &str, err: rusqlite::Error| {
        format!("trace size measurement {operation} failed: {err}")
    };
    let mut connection = Connection::open(&scratch.0).map_err(|err| context("open", err))?;
    connection
        .execute_batch(
            "PRAGMA auto_vacuum=INCREMENTAL; PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
             PRAGMA cache_size=-2048;",
        )
        .map_err(|err| context("configuration", err))?;
    connection
        .execute_batch(SCHEMA_SQL)
        .map_err(|err| context("schema", err))?;
    let before = page_stats(&connection).map_err(|err| context("page count", err))?;
    let tx = connection
        .transaction()
        .map_err(|err| context("transaction", err))?;
    insert_rows(&tx, &scratch.0, 0, 0, rows)?;
    let after = page_stats(&tx).map_err(|err| context("page count", err))?;
    // journal_mode=OFF cannot roll back; the file is discarded instead.
    tx.commit().map_err(|err| context("commit", err))?;
    drop(connection);
    Ok(after
        .pages
        .saturating_sub(before.pages)
        .saturating_mul(after.page_size))
}

#[derive(Clone, Copy)]
struct PageStats {
    pages: u64,
    free: u64,
    page_size: u64,
}

fn page_stats(connection: &Connection) -> rusqlite::Result<PageStats> {
    let pages: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let free: i64 = connection.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    let page_size: i64 = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    Ok(PageStats {
        pages: pages.max(0) as u64,
        free: free.max(0) as u64,
        page_size: page_size.max(0) as u64,
    })
}

const DROPPED_INCREMENT_SQL: &str =
    "INSERT INTO storage_meta(key, value) VALUES ('known_dropped_spans', ?1)
     ON CONFLICT(key) DO UPDATE SET value = CAST(value AS INTEGER) + ?1";

const INSERT_SPAN_SQL: &str = "INSERT INTO spans (
        epoch, trace_id, span_id, parent_span_id, name, start_time_ns,
        end_time_ns, status, status_description, service_name, payload,
        pending, ingest_time_ns, approx_bytes
     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?13)
     ON CONFLICT(epoch, trace_id, span_id) DO UPDATE SET
        parent_span_id = excluded.parent_span_id,
        name = excluded.name,
        start_time_ns = excluded.start_time_ns,
        end_time_ns = excluded.end_time_ns,
        status = excluded.status,
        status_description = excluded.status_description,
        service_name = excluded.service_name,
        payload = excluded.payload,
        pending = 0,
        ingest_time_ns = excluded.ingest_time_ns,
        approx_bytes = excluded.approx_bytes";

/// One aggregate per trace touched by a write: the subqueries walk the
/// trace's rows once per batch instead of once per inserted span, which for a
/// 30k-span trace was the difference between milliseconds and minutes.
const TRACE_META_SQL: &str = "INSERT INTO trace_meta (epoch, trace_id, first_start_ns, last_end_ns, last_ingest_ns, span_count, approx_bytes)
     VALUES (
        ?1, ?2, ?3, ?4, ?5,
        (SELECT COUNT(*) FROM spans WHERE epoch = ?1 AND trace_id = ?2),
        (SELECT COALESCE(SUM(approx_bytes), 0) FROM spans WHERE epoch = ?1 AND trace_id = ?2)
     )
     ON CONFLICT(epoch, trace_id) DO UPDATE SET
        first_start_ns = MIN(trace_meta.first_start_ns, excluded.first_start_ns),
        last_end_ns = MAX(trace_meta.last_end_ns, excluded.last_end_ns),
        last_ingest_ns = excluded.last_ingest_ns,
        span_count = excluded.span_count,
        approx_bytes = excluded.approx_bytes";

/// Write span rows, their attribute rows and the per-trace aggregates. Shared
/// by the archive transaction and the isolated measurement so both build the
/// same pages.
fn insert_rows(
    connection: &Connection,
    database_path: &Path,
    epoch: i64,
    ingest_time_ns: i64,
    rows: &[(&StoredSpan, &str)],
) -> Result<(), String> {
    let context =
        |operation: String, err: rusqlite::Error| database_error(database_path, &operation, err);
    let mut insert_span = connection
        .prepare_cached(INSERT_SPAN_SQL)
        .map_err(|err| context("span insert preparation".to_string(), err))?;
    let mut delete_attributes = connection
        .prepare_cached(
            "DELETE FROM span_attributes WHERE epoch = ?1 AND trace_id = ?2 AND span_id = ?3",
        )
        .map_err(|err| context("attribute delete preparation".to_string(), err))?;
    let mut insert_attribute = connection
        .prepare_cached(
            "INSERT INTO span_attributes (epoch, trace_id, span_id, ordinal, key, value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(|err| context("attribute insert preparation".to_string(), err))?;
    // trace_id -> (first_start_ns, last_end_ns) over these rows.
    let mut touched: BTreeMap<&str, (i64, i64)> = BTreeMap::new();
    for (span, payload) in rows {
        let start = span.start_time_unix_nano as i64;
        let end = span.end_time_unix_nano as i64;
        touched
            .entry(span.trace_id.as_str())
            .and_modify(|(first, last)| {
                *first = (*first).min(start);
                *last = (*last).max(end);
            })
            .or_insert((start, end));
        insert_span
            .execute(params![
                epoch,
                span.trace_id,
                span.span_id,
                span.parent_span_id,
                span.name,
                start,
                end,
                span.status,
                span.status_description,
                span.service_name,
                payload,
                ingest_time_ns,
                payload.len() as i64,
            ])
            .map_err(|err| context(format!("write trace '{}'", span.trace_id), err))?;
        delete_attributes
            .execute(params![epoch, span.trace_id, span.span_id])
            .map_err(|err| {
                context(
                    format!("replace attributes for trace '{}'", span.trace_id),
                    err,
                )
            })?;
        for (ordinal, (key, value)) in span.attributes.iter().enumerate() {
            insert_attribute
                .execute(params![
                    epoch,
                    span.trace_id,
                    span.span_id,
                    ordinal as i64,
                    key,
                    value
                ])
                .map_err(|err| {
                    context(
                        format!("write attribute for trace '{}'", span.trace_id),
                        err,
                    )
                })?;
        }
    }
    let mut update_meta = connection
        .prepare_cached(TRACE_META_SQL)
        .map_err(|err| context("metadata preparation".to_string(), err))?;
    for (trace_id, (first_start_ns, last_end_ns)) in touched {
        update_meta
            .execute(params![
                epoch,
                trace_id,
                first_start_ns,
                last_end_ns,
                ingest_time_ns
            ])
            .map_err(|err| context(format!("update metadata for trace '{trace_id}'"), err))?;
    }
    Ok(())
}

enum Trial {
    Committed,
    /// Rolled back: the image would exceed limit plus tolerance. `growth` is
    /// the real space the rows took (new pages plus reused free pages);
    /// `room` is what the archive had for them before the trial.
    Rejected {
        growth: u64,
        room: u64,
    },
}

/// Write `groups` in one transaction; commit only if the real SQLite image
/// stays within limit plus tolerance. The pending loss count rides in the
/// same transaction, and so does each trace's share of the measured page
/// delta (`trace_meta.image_bytes`). Never evicts.
fn try_commit(
    store: &TraceDiskStore,
    connection: &mut Connection,
    groups: &[&TraceGroup],
) -> Result<Trial, String> {
    let rows: Vec<(&StoredSpan, &str)> = groups.iter().flat_map(|group| group.rows()).collect();
    let rows = rows.as_slice();
    let limits = store.capacity_limits();
    let path = &store.database_path;
    let before = page_stats(connection).map_err(|err| database_error(path, "page count", err))?;
    let unpersisted = store.unpersisted_dropped();
    let epoch = store.epoch.load(Ordering::Acquire) as i64;
    let tx = connection
        .transaction()
        .map_err(|err| database_error(path, "write transaction start", err))?;
    insert_rows(&tx, path, epoch, super::otel::now_unix_nanos() as i64, rows)?;
    if unpersisted > 0 {
        tx.execute(DROPPED_INCREMENT_SQL, params![unpersisted as i64])
            .map_err(|err| database_error(path, "dropped-count update", err))?;
    }
    let after = page_stats(&tx).map_err(|err| database_error(path, "page count", err))?;
    // The directory once the WAL is checkpointed: pages plus the -shm index.
    let sidecars = store.sidecar_bytes();
    let image = after
        .pages
        .saturating_mul(after.page_size)
        .saturating_add(sidecars);
    if image > limits.threshold {
        tx.rollback()
            .map_err(|err| database_error(path, "oversized batch rollback", err))?;
        // A rollback can leave spilled WAL frames behind; return them.
        let _ = reclaim_free_pages(connection);
        let growth = after
            .pages
            .saturating_add(before.free)
            .saturating_sub(before.pages.saturating_add(after.free))
            .saturating_mul(after.page_size);
        let room = limits
            .threshold
            .saturating_add(before.free.saturating_mul(after.page_size))
            .saturating_sub(before.pages.saturating_mul(after.page_size))
            .saturating_sub(sidecars);
        return Ok(Trial::Rejected { growth, room });
    }
    // Pages this transaction consumed (new pages plus reused free pages,
    // minus pages freed by replaced versions), shared by storage weight when
    // several traces were written together. Summed over the traces this is
    // the file's used size; one trace written alone gets its exact delta.
    let delta =
        (after.pages as i128 + before.free as i128 - before.pages as i128 - after.free as i128)
            * after.page_size as i128;
    let group_weights: Vec<i128> = groups
        .iter()
        .map(|group| {
            group
                .rows
                .iter()
                .map(|row| row_storage_weight(&row.span, &row.payload))
                .fold(0_u64, u64::saturating_add)
                .max(1) as i128
        })
        .collect();
    let weights: i128 = group_weights.iter().sum();
    for (group, weight) in groups.iter().zip(&group_weights) {
        let share = delta * weight / weights.max(1);
        tx.execute(
            "UPDATE trace_meta SET image_bytes = MAX(0, image_bytes + ?3)
             WHERE epoch = ?1 AND trace_id = ?2",
            params![epoch, group.trace_id, share as i64],
        )
        .map_err(|err| database_error(path, "image accounting", err))?;
    }
    tx.commit()
        .map_err(|err| database_error(path, "write transaction commit", err))?;
    store.confirm_dropped_persisted(unpersisted);
    let _ = connection.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
    // PASSIVE copies pages but keeps the WAL file. Do not leave the last
    // batch's duplicate image on disk until a later insert or retention tick.
    if directory_size(&store.directory)
        .is_ok_and(|bytes| bytes > (limits.max_bytes as f64 * CAP_HIGH_WATERMARK) as u64)
    {
        let _ = reclaim_free_pages(connection);
    }
    Ok(Trial::Committed)
}

enum CapacityError {
    /// Above limit plus tolerance with no incoming bytes, after evicting every
    /// unprotected trace: nothing fits until protected traces finish, limits
    /// change or retention frees space.
    Blocked(String),
    /// There is room, but not for everything incoming.
    Insufficient(String),
    Storage(String),
}

impl CapacityError {
    fn into_message(self) -> String {
        match self {
            Self::Blocked(message) | Self::Insufficient(message) | Self::Storage(message) => {
                message
            }
        }
    }
}

fn delete_trace(connection: &Connection, epoch: i64, trace_id: &str) -> rusqlite::Result<()> {
    connection.execute(
        "DELETE FROM span_attributes WHERE epoch = ?1 AND trace_id = ?2",
        params![epoch, trace_id],
    )?;
    connection.execute(
        "DELETE FROM spans WHERE epoch = ?1 AND trace_id = ?2",
        params![epoch, trace_id],
    )?;
    connection.execute(
        "DELETE FROM trace_meta WHERE epoch = ?1 AND trace_id = ?2",
        params![epoch, trace_id],
    )?;
    Ok(())
}

/// The full-VACUUM fallback for evictions that return no space (a file that
/// predates incremental auto_vacuum). It rewrites the whole database, so it
/// runs at most once per `FULL_VACUUM_MIN_INTERVAL` per archive, and its
/// failure warning goes through the shared one-per-minute limiter.
fn full_vacuum_fallback(
    store: &TraceDiskStore,
    connection: &Connection,
    writer: &mut WriterState,
) -> bool {
    let now = Instant::now();
    if writer
        .last_full_vacuum
        .is_some_and(|at| now.duration_since(at) < FULL_VACUUM_MIN_INTERVAL)
    {
        return false;
    }
    writer.last_full_vacuum = Some(now);
    if let Err(error) = run_full_vacuum(store, connection) {
        store.warn_limited(
            "trace eviction full-VACUUM fallback failed",
            &error.to_string(),
        );
    }
    true
}

fn run_full_vacuum(store: &TraceDiskStore, connection: &Connection) -> rusqlite::Result<()> {
    #[cfg(test)]
    {
        store.full_vacuum_attempts.fetch_add(1, Ordering::Relaxed);
        if store.fail_full_vacuum.load(Ordering::Relaxed) {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
                Some("injected VACUUM failure".to_string()),
            ));
        }
    }
    #[cfg(not(test))]
    let _ = store;
    connection.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")
}

/// Make room for `incoming` `(trace_id, estimated bytes)` by evicting the
/// oldest unprotected traces. Returns the incoming traces that would not fit
/// even if every unprotected trace were evicted; nothing is evicted on their
/// behalf. What protected traces hold is their commit-measured
/// `image_bytes` (never less than `approx_bytes`), raised to the measured
/// floor left by an earlier exhaustive eviction while the same traces stay
/// protected. While the archive is above limit plus tolerance every candidate
/// stays evictable to restore the cap.
fn ensure_capacity(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    writer: &mut WriterState,
    incoming: &[(&str, u64)],
    protected: &HashSet<String>,
) -> Result<HashSet<String>, CapacityError> {
    let mut incoming_bytes = incoming
        .iter()
        .map(|(_, bytes)| *bytes)
        .fold(0_u64, u64::saturating_add);
    let mut cannot_fit_now = HashSet::new();
    let limits = store.capacity_limits();
    let high_watermark = ((limits.max_bytes as f64) * CAP_HIGH_WATERMARK) as u64;
    let measure = || {
        directory_size(&store.directory).map_err(|err| {
            CapacityError::Storage(format!(
                "cannot measure trace storage directory '{}': {err}",
                store.directory.display()
            ))
        })
    };
    let current = measure()?;
    if current.saturating_add(incoming_bytes) <= high_watermark {
        return Ok(cannot_fit_now);
    }

    // Reclaim the write-ahead log before deleting anything: accumulated WAL
    // frames count toward the measured directory but represent no history,
    // and reclaiming them alone can put the directory back under the cap.
    let _ = reclaim_free_pages(connection);
    let current = measure().unwrap_or(current);
    if current.saturating_add(incoming_bytes) <= high_watermark {
        return Ok(cannot_fit_now);
    }

    let target = ((limits.max_bytes as f64) * CAP_LOW_WATERMARK) as u64;
    let epoch = store.epoch.load(Ordering::Acquire) as i64;
    let storage_error = |operation: &str, err: rusqlite::Error| {
        CapacityError::Storage(database_error(&store.database_path, operation, err))
    };
    let mut candidate_statement = connection
        .prepare(
            "SELECT trace_id, approx_bytes, image_bytes FROM trace_meta
             WHERE epoch = ?1 ORDER BY last_ingest_ns ASC, trace_id ASC",
        )
        .map_err(|err| storage_error("eviction preparation", err))?;
    let traces: Vec<(String, u64)> = candidate_statement
        .query_map(params![epoch], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|err| storage_error("eviction candidate query", err))?
        .map(|row| row.map(|(trace_id, approx, image)| (trace_id, approx.max(image).max(0) as u64)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| storage_error("eviction candidate read", err))?;
    drop(candidate_statement);
    let (held, candidates): (Vec<_>, Vec<_>) = traces
        .into_iter()
        .partition(|(trace_id, _)| protected.contains(trace_id));
    if current <= limits.threshold && !incoming.is_empty() {
        let held_total = held
            .iter()
            .map(|(_, bytes)| *bytes)
            .fold(0_u64, u64::saturating_add);
        let held_by_trace: HashMap<&str, u64> = held
            .iter()
            .map(|(trace_id, bytes)| (trace_id.as_str(), *bytes))
            .collect();
        // A floor trace that lost its protection can finish or be evicted:
        // the measurement no longer bounds what the protected traces hold.
        if writer.held_floor.as_ref().is_some_and(|floor| {
            !floor
                .traces
                .iter()
                .all(|trace_id| protected.contains(trace_id))
        }) {
            writer.held_floor = None;
        }
        // With a measured floor: the floor today plus what the protected
        // traces outside it are attributed.
        let floor_estimate = writer.held_floor.as_ref().map(|floor| {
            let (inside, outside) =
                held.iter()
                    .fold((0_u64, 0_u64), |(inside, outside), (trace_id, bytes)| {
                        if floor.traces.contains(trace_id) {
                            (inside.saturating_add(*bytes), outside)
                        } else {
                            (inside, outside.saturating_add(*bytes))
                        }
                    });
            floor.now(inside).saturating_add(outside)
        });
        for (trace_id, bytes) in incoming {
            // Without one, a group's own durable rows are left out: their
            // attributed size may be too high. A group that still does not
            // fit once the unprotected traces are gone is judged exactly
            // after that one eviction, which measures the floor.
            let held_estimate = floor_estimate.unwrap_or_else(|| {
                held_total.saturating_sub(held_by_trace.get(trace_id).copied().unwrap_or(0))
            });
            if held_estimate.saturating_add(*bytes) > limits.threshold {
                cannot_fit_now.insert((*trace_id).to_string());
                incoming_bytes = incoming_bytes.saturating_sub(*bytes);
            }
        }
        if incoming_bytes == 0 {
            return Ok(cannot_fit_now);
        }
    }

    // Evict oldest-first in batches sized by the estimated excess,
    // re-measuring the DIRECTORY after each batch. `approx_bytes` undercounts
    // real file usage (indexes, attribute rows, free pages), so it only
    // SIZES a batch — never decides success: an accounting-driven pass can
    // delete the entire archive while the file never shrinks. A batch that
    // frees no bytes (even after the rate-limited full-VACUUM fallback) means
    // deleting more history cannot help, so the remaining traces are kept.
    let mut next_candidate = 0usize;
    let mut remaining_approx: u64 = candidates.iter().map(|(_, bytes)| *bytes).sum();
    let mut measured = current;
    let mut evicted_any = false;
    while next_candidate < candidates.len() {
        let occupied = measured.saturating_add(incoming_bytes);
        if occupied <= target {
            break;
        }
        // Aim each batch at HALF the remaining excess, translated into
        // `approx_bytes` currency: the file carries a real-bytes-per-
        // approx-byte overhead (attribute rows, indexes, page slack), so the
        // measured excess is scaled down by the current physical/approx
        // ratio before sizing the batch. Halving converges from above in a
        // few measured iterations instead of overshooting the low watermark
        // and destroying more history than the cap requires.
        let excess = occupied.saturating_sub(target);
        let scale = if remaining_approx > 0 {
            (measured as f64 / remaining_approx as f64).max(1.0)
        } else {
            f64::MAX
        };
        let batch_goal = ((excess as f64 / 2.0 / scale) as u64).max(64 * 1024);
        let mut chunk: Vec<&str> = Vec::new();
        let mut chunk_approx = 0u64;
        while next_candidate < candidates.len() {
            let (candidate, approx_bytes) = &candidates[next_candidate];
            chunk.push(candidate.as_str());
            chunk_approx = chunk_approx.saturating_add(*approx_bytes);
            next_candidate += 1;
            if chunk_approx >= batch_goal {
                break;
            }
        }
        remaining_approx = remaining_approx.saturating_sub(chunk_approx);
        let tx = connection
            .transaction()
            .map_err(|err| storage_error("eviction transaction start", err))?;
        for trace_id in &chunk {
            delete_trace(&tx, epoch, trace_id)
                .map_err(|err| storage_error("eviction delete", err))?;
        }
        tx.commit()
            .map_err(|err| storage_error("eviction transaction commit", err))?;
        writer.forget_floor_of(chunk.iter().copied());
        evicted_any = true;
        // Vacuum BEFORE the checkpoint (reclaim_free_pages does both, in
        // steps): in WAL mode the freed pages land in the write-ahead log and
        // only the checkpoint that follows returns them to the filesystem.
        let _ = reclaim_free_pages(connection);
        let mut now_measured = measure().unwrap_or(measured);
        if now_measured >= measured {
            // No physical progress: free pages are not returning to the
            // filesystem. Spend the rate-limited full VACUUM; if the file
            // still does not shrink, stop evicting instead of wiping history.
            if !full_vacuum_fallback(store, connection, writer) {
                measured = now_measured;
                break;
            }
            now_measured = measure().unwrap_or(now_measured);
            if now_measured >= measured {
                measured = now_measured;
                break;
            }
        }
        measured = now_measured;
    }

    // Every unprotected trace is gone: what remains is held by protected
    // traces plus fixed overhead. Remember it, so a group that still does
    // not fit is not the reason for another eviction while they stay.
    if next_candidate >= candidates.len() {
        writer.held_floor = Some(HeldFloor {
            traces: held.iter().map(|(trace_id, _)| trace_id.clone()).collect(),
            bytes: used_bytes(store, connection).unwrap_or_else(|_| measure().unwrap_or(measured)),
            attributed: held
                .iter()
                .map(|(_, bytes)| *bytes)
                .fold(0_u64, u64::saturating_add),
        });
    }

    // Deleting rows can show phantom "progress" (a truncated WAL) while the
    // main file never shrinks. If eviction ran and the directory is still
    // over the limit, spend the rate-limited full VACUUM before concluding.
    if evicted_any
        && measured.saturating_add(incoming_bytes) > limits.threshold
        && full_vacuum_fallback(store, connection, writer)
    {
        measured = measure().unwrap_or(measured);
    }

    let measured = measure().unwrap_or(measured);
    if measured.saturating_add(incoming_bytes) > limits.threshold {
        let message = format!(
            "trace storage limit reached: {measured} bytes used, {incoming_bytes} bytes incoming, limit {} bytes, tolerance {} bytes",
            limits.max_bytes, limits.tolerance
        );
        return Err(if measured > limits.threshold {
            CapacityError::Blocked(message)
        } else {
            CapacityError::Insufficient(message)
        });
    }
    Ok(cannot_fit_now)
}

fn clear_database(store: &Arc<TraceDiskStore>, connection: &mut Connection) -> Result<(), String> {
    let tx = connection
        .transaction()
        .map_err(|err| database_error(&store.database_path, "clear transaction start", err))?;
    tx.execute("DELETE FROM span_attributes", [])
        .map_err(|err| database_error(&store.database_path, "clear attributes", err))?;
    tx.execute("DELETE FROM spans", [])
        .map_err(|err| database_error(&store.database_path, "clear spans", err))?;
    tx.execute("DELETE FROM trace_meta", [])
        .map_err(|err| database_error(&store.database_path, "clear metadata", err))?;
    let next_epoch = store.epoch.fetch_add(1, Ordering::AcqRel) + 1;
    tx.execute(
        "INSERT INTO storage_meta (key, value) VALUES ('epoch', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![next_epoch.to_string()],
    )
    .map_err(|err| database_error(&store.database_path, "clear epoch update", err))?;
    tx.execute(
        "INSERT INTO storage_meta(key, value) VALUES ('known_dropped_spans', '0')
         ON CONFLICT(key) DO UPDATE SET value = '0'",
        [],
    )
    .map_err(|err| database_error(&store.database_path, "clear dropped-count update", err))?;
    tx.commit()
        .map_err(|err| database_error(&store.database_path, "clear transaction commit", err))?;
    {
        let mut state = store
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.known_dropped_spans = 0;
        state.unpersisted_dropped = 0;
    }
    let _ = reclaim_free_pages(connection);
    store.record_success();
    Ok(())
}

fn retain_database(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    writer: &mut WriterState,
) -> Result<(), String> {
    let retention_seconds = store.retention_seconds.load(Ordering::Acquire);
    if retention_seconds > 0 {
        let cutoff = super::otel::now_unix_nanos()
            .saturating_sub(retention_seconds.saturating_mul(1_000_000_000));
        let epoch = store.epoch.load(Ordering::Acquire) as i64;
        let mut statement = connection
            .prepare("SELECT trace_id FROM trace_meta WHERE epoch = ?1 AND last_ingest_ns < ?2")
            .map_err(|err| database_error(&store.database_path, "retention preparation", err))?;
        let traces = statement
            .query_map(params![epoch, cutoff as i64], |row| row.get::<_, String>(0))
            .map_err(|err| database_error(&store.database_path, "retention query", err))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| database_error(&store.database_path, "retention row read", err))?;
        drop(statement);
        for chunk in traces.chunks(RETENTION_DELETE_CHUNK) {
            let tx = connection.transaction().map_err(|err| {
                database_error(&store.database_path, "retention transaction start", err)
            })?;
            for trace_id in chunk {
                delete_trace(&tx, epoch, trace_id)
                    .map_err(|err| database_error(&store.database_path, "retention delete", err))?;
            }
            tx.commit().map_err(|err| {
                database_error(&store.database_path, "retention transaction commit", err)
            })?;
        }
        writer.forget_floor_of(traces.iter().map(String::as_str));
        // Return the space in bounded steps; see `reclaim_free_pages`.
        let _ = reclaim_free_pages(connection);
    }
    let protected = store
        .hot_storage()
        .map(|hot| hot.pending_trace_ids())
        .unwrap_or_default();
    let result = ensure_capacity(store, connection, writer, &[], &protected)
        .map(|_| ())
        .map_err(CapacityError::into_message);
    if result.is_ok() {
        store.record_success();
    }
    result
}

/// `PRAGMA auto_vacuum` only takes effect on a database created under it; on
/// a pre-existing file it stays a header wish until a full VACUUM rewrites
/// the database. Without this migration `PRAGMA incremental_vacuum` is a
/// silent no-op: evictions delete rows but the file never shrinks, so a
/// lowered disk cap deletes ever more history while `directory_size` stays
/// over the limit. The mode is probed through a FRESH connection — on a
/// connection that already issued the pragma, `PRAGMA auto_vacuum` reports
/// the pending wish, not the file header. Runs once per database; a VACUUM
/// of a large legacy file can take a while, which is still better than
/// unbounded history loss.
fn ensure_incremental_vacuum(
    connection: &mut Connection,
    database_path: &Path,
) -> Result<(), String> {
    const INCREMENTAL: i64 = 2;
    let mode: i64 = Connection::open(database_path)
        .and_then(|probe| probe.query_row("PRAGMA auto_vacuum", [], |row| row.get(0)))
        .map_err(|err| format!("cannot read trace database auto_vacuum mode: {err}"))?;
    if mode == INCREMENTAL {
        return Ok(());
    }
    tracing::info!(
        previous_mode = mode,
        "migrating trace database to incremental auto_vacuum (one-time VACUUM)"
    );
    connection
        .execute_batch("PRAGMA auto_vacuum=INCREMENTAL; VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|err| format!("cannot migrate trace database to incremental vacuum: {err}"))
}

/// Archive tables and indexes. The isolated size measurement builds the same
/// schema so both databases produce the same pages.
const SCHEMA_SQL: &str =
    "CREATE TABLE IF NOT EXISTS storage_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS spans (
        epoch INTEGER NOT NULL,
        trace_id TEXT NOT NULL,
        span_id TEXT NOT NULL,
        parent_span_id TEXT,
        name TEXT NOT NULL,
        start_time_ns INTEGER NOT NULL,
        end_time_ns INTEGER NOT NULL,
        status TEXT NOT NULL,
        status_description TEXT,
        service_name TEXT NOT NULL,
        payload TEXT NOT NULL,
        pending INTEGER NOT NULL DEFAULT 0,
        ingest_time_ns INTEGER NOT NULL,
        approx_bytes INTEGER NOT NULL,
        PRIMARY KEY (epoch, trace_id, span_id)
    );
    CREATE INDEX IF NOT EXISTS spans_trace_idx ON spans(epoch, trace_id);
    CREATE INDEX IF NOT EXISTS spans_start_idx ON spans(epoch, start_time_ns, trace_id);
    CREATE TABLE IF NOT EXISTS span_attributes (
        epoch INTEGER NOT NULL,
        trace_id TEXT NOT NULL,
        span_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        PRIMARY KEY (epoch, trace_id, span_id, ordinal)
    );
    CREATE INDEX IF NOT EXISTS span_attributes_lookup_idx
        ON span_attributes(epoch, key, value, trace_id);
    CREATE TABLE IF NOT EXISTS trace_meta (
        epoch INTEGER NOT NULL,
        trace_id TEXT NOT NULL,
        first_start_ns INTEGER NOT NULL,
        last_end_ns INTEGER NOT NULL,
        last_ingest_ns INTEGER NOT NULL,
        span_count INTEGER NOT NULL,
        approx_bytes INTEGER NOT NULL,
        image_bytes INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (epoch, trace_id)
    );
    CREATE INDEX IF NOT EXISTS trace_meta_retention_idx
        ON trace_meta(epoch, last_ingest_ns, trace_id);";

/// `trace_meta.image_bytes` (pages a trace occupies, attributed at commit)
/// is an additive column: older archives gain it with 0 ("not measured",
/// the lower bound is used until the trace is written again), and older
/// binaries ignore it, so the schema version stays 1.
fn add_image_bytes_column(connection: &Connection) -> rusqlite::Result<()> {
    let present = connection
        .prepare("SELECT 1 FROM pragma_table_info('trace_meta') WHERE name = 'image_bytes'")?
        .exists([])?;
    if !present {
        connection.execute_batch(
            "ALTER TABLE trace_meta ADD COLUMN image_bytes INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    Ok(())
}

// `auto_vacuum` MUST precede `journal_mode=WAL`: switching the journal mode
// initializes the database header, and auto_vacuum declared after that point
// is a silent no-op on the file (while reading the pragma back on the same
// connection still reports the wished value).
fn configure_connection(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "PRAGMA auto_vacuum=INCREMENTAL;
             PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=1000;",
        )
        .and_then(|()| connection.execute_batch(SCHEMA_SQL))
        .and_then(|()| add_image_bytes_column(connection))
        .and_then(|()| {
            connection.execute_batch(
                "INSERT INTO storage_meta(key, value) VALUES ('epoch', '0')
                ON CONFLICT(key) DO NOTHING;
             INSERT INTO storage_meta(key, value) VALUES ('clean_shutdown', '1')
                ON CONFLICT(key) DO NOTHING;
             INSERT INTO storage_meta(key, value) VALUES ('known_dropped_spans', '0')
                ON CONFLICT(key) DO NOTHING;",
            )
        })
        .map_err(|err| format!("cannot initialize trace database: {err}"))?;

    let schema_version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|err| format!("cannot read trace database schema version: {err}"))?;
    const CURRENT_SCHEMA_VERSION: i64 = 1;
    if schema_version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "trace database schema version {schema_version} is newer than supported version {CURRENT_SCHEMA_VERSION}"
        ));
    }
    if schema_version == 0 {
        connection
            .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
            .map_err(|err| format!("cannot persist trace database schema version: {err}"))?;
    }
    Ok(())
}

fn configure_read_connection(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch("PRAGMA query_only=ON; PRAGMA busy_timeout=1000;")
        .map_err(|err| format!("cannot configure trace read connection: {err}"))
}

fn read_epoch(connection: &Connection) -> Option<u64> {
    connection
        .query_row(
            "SELECT value FROM storage_meta WHERE key = 'epoch'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|value| value.parse().ok())
}

fn read_clean_shutdown(connection: &Connection) -> Option<bool> {
    connection
        .query_row(
            "SELECT value FROM storage_meta WHERE key = 'clean_shutdown'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .map(|value| value == "1")
}

fn read_dropped_spans(connection: &Connection) -> Option<u64> {
    connection
        .query_row(
            "SELECT value FROM storage_meta WHERE key = 'known_dropped_spans'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|value| value.parse().ok())
}

fn set_clean_shutdown(connection: &Connection, clean: bool) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO storage_meta(key, value) VALUES ('clean_shutdown', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![if clean { "1" } else { "0" }],
    )
}

fn directory_size(directory: &Path) -> std::io::Result<u64> {
    let mut total: u64 = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

fn set_private_permissions(path: &Path, directory: bool) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if directory { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|err| format!("cannot set permissions on '{}': {err}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[cfg(unix)]
    use std::{process::Command, time::Instant};

    /// Held by a child that runs under `ulimit -S -f`: on drop it raises the
    /// soft file-size limit back to the hard one. The limit is for the test
    /// body only; a coverage build writes its multi-MiB profile when the
    /// child exits, and a capped write leaves a truncated profile that fails
    /// the coverage merge.
    #[cfg(unix)]
    struct LiftFileSizeLimitOnDrop;

    #[cfg(unix)]
    impl Drop for LiftFileSizeLimitOnDrop {
        fn drop(&mut self) {
            let mut limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: getrlimit/setrlimit only read and write the owned struct.
            unsafe {
                if libc::getrlimit(libc::RLIMIT_FSIZE, &mut limit) == 0 {
                    limit.rlim_cur = limit.rlim_max;
                    libc::setrlimit(libc::RLIMIT_FSIZE, &limit);
                }
            }
        }
    }

    fn test_config(directory: &Path) -> TraceStorageConfig {
        TraceStorageConfig {
            directory: directory.to_string_lossy().into_owned(),
            max_disk_bytes: 64 * 1024 * 1024,
            retention_seconds: 30 * 24 * 60 * 60,
            memory_max_bytes: 16 * 1024 * 1024,
            ..TraceStorageConfig::default()
        }
    }

    fn test_span(trace_id: &str, span_id: &str, name: &str) -> StoredSpan {
        StoredSpan {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            parent_span_id: None,
            name: name.to_string(),
            start_time_unix_nano: 1_000_000_000,
            end_time_unix_nano: 1_500_000_000,
            status: "ok".to_string(),
            status_description: None,
            attributes: vec![("authorization".to_string(), "secret".to_string())],
            service_name: "test".to_string(),
            events: Vec::new(),
            links: Vec::new(),
            instrumentation_scope_name: None,
            instrumentation_scope_version: None,
            flags: None,
            trace_state: None,
            pending: false,
            observe_depth: 0,
        }
    }

    /// A database created before the incremental auto_vacuum pragma keeps
    /// mode NONE forever unless a full VACUUM rewrites it; without the
    /// migration every `incremental_vacuum` is a silent no-op and evicted
    /// space never returns to the filesystem.
    #[test]
    fn legacy_database_without_incremental_vacuum_is_migrated_on_open() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let database_path = directory.path().join("traces.sqlite3");
        {
            let legacy = Connection::open(&database_path).expect("create legacy database");
            legacy
                .execute_batch(
                    "CREATE TABLE storage_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
                )
                .expect("create legacy schema");
            let mode: i64 = legacy
                .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
                .expect("read legacy vacuum mode");
            assert_eq!(mode, 0, "legacy fixture must start without auto_vacuum");
        }

        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).expect("open trace store over legacy file");
        let probe = Connection::open(&database_path).expect("open probe connection");
        let mode: i64 = probe
            .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
            .expect("read migrated vacuum mode");
        assert_eq!(mode, 2, "open must migrate the database to incremental");
        store.shutdown();
    }

    #[test]
    fn repeated_capacity_warnings_are_rate_limited_to_one_per_minute() {
        let start = Instant::now();
        let mut last = None;
        let mut suppressed = 0;
        let emitted: Vec<_> = (0..100)
            .filter_map(|_| capacity_warning_due(&mut last, &mut suppressed, start))
            .collect();
        assert_eq!(emitted, vec![0]);
        assert_eq!(suppressed, 99);
        assert_eq!(
            capacity_warning_due(&mut last, &mut suppressed, start + Duration::from_secs(59)),
            None
        );
        assert_eq!(
            capacity_warning_due(&mut last, &mut suppressed, start + Duration::from_secs(60)),
            Some(100)
        );
    }

    #[test]
    fn oversized_trace_is_dropped_and_following_normal_spans_are_archived() {
        let directory = tempfile::tempdir().unwrap();
        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).unwrap();
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(32, 128 * 1024 * 1024));
        store.attach_hot_storage(&hot);
        let mut oversized = test_span("wide-trace", "wide-span", "wide");
        oversized.attributes = (0..400)
            .map(|i| (format!("field-{i}"), "x".repeat(64 * 1024)))
            .collect();
        let normal_spans: Vec<_> = (0..12)
            .map(|i| test_span(&format!("normal-{i:02}"), "span", "normal"))
            .collect();
        let mut incoming = vec![oversized];
        incoming.extend(normal_spans);
        hot.add_spans(incoming);

        store
            .flush()
            .expect("unfittable trace is dropped and normal spans continue");
        assert!(
            store
                .get_spans_by_trace_id("wide-trace")
                .unwrap()
                .is_empty()
        );
        let archived = store.get_spans().unwrap();
        assert_eq!(archived.len(), 12, "all normal traces must make progress");
        assert_eq!(hot.dirty_len(), 0, "rejected trace is not retried forever");
        let status = store.status();
        assert_eq!(status.known_dropped_spans, 1);
        assert_eq!(status.completeness, "partial");
        eprintln!(
            "OVERSIZED_FOLLOWED_EVIDENCE archived_normal={} dropped={} physical={}",
            archived.len(),
            status.known_dropped_spans,
            status.physical_bytes
        );
        store.shutdown();
    }

    #[test]
    fn sustained_archive_writes_stay_within_physical_disk_cap() {
        let directory = tempfile::tempdir().unwrap();
        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).unwrap();
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(512, 128 * 1024 * 1024));
        store.attach_hot_storage(&hot);
        let mut maximum = 0;
        for index in 0..100 {
            let spans = (0..63)
                .map(|part| {
                    let mut span =
                        test_span(&format!("cap-{index:04}-{part:02}"), "span", "cap-load");
                    span.attributes = vec![("iii.payload.json".to_string(), "x".repeat(64 * 1024))];
                    span
                })
                .collect();
            hot.add_spans(spans);
            store.flush().expect("bounded batch persists");
            let bytes = store.status().physical_bytes;
            maximum = maximum.max(bytes);
            assert!(
                bytes <= config.max_disk_bytes + 8 * 1024 * 1024,
                "{bytes} bytes at batch {index}"
            );
        }
        assert_eq!(store.get_spans_by_trace_id("cap-0099-62").unwrap().len(), 1);
        assert!(
            store
                .get_spans_by_trace_id("cap-0000-00")
                .unwrap()
                .is_empty()
        );
        eprintln!(
            "TRACE_CAP_EVIDENCE max_disk_bytes={} max_observed_bytes={maximum} final_bytes={}",
            config.max_disk_bytes,
            store.status().physical_bytes
        );
        store.shutdown();
    }

    #[test]
    fn age_retention_removes_old_ingested_trace_and_keeps_recent_trace() {
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&test_config(directory.path())).unwrap();
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(8, 1024 * 1024));
        store.attach_hot_storage(&hot);
        hot.add_spans(vec![
            test_span("old", "span", "old"),
            test_span("new", "span", "new"),
        ]);
        store.flush().unwrap();
        let conn = Connection::open(directory.path().join("traces.sqlite3")).unwrap();
        conn.execute(
            "UPDATE trace_meta SET last_ingest_ns=1 WHERE trace_id='old'",
            [],
        )
        .unwrap();
        drop(conn);
        store.retain().unwrap();
        assert!(store.get_spans_by_trace_id("old").unwrap().is_empty());
        assert_eq!(store.get_spans_by_trace_id("new").unwrap().len(), 1);
        store.shutdown();
    }

    /// Shrinking the disk cap below the current physical size must evict
    /// oldest traces until the FILE fits again — measured on disk, not by
    /// row accounting — keep the newest history, and leave the archive
    /// healthy for the next write.
    #[test]
    fn cap_shrink_evicts_oldest_shrinks_file_and_recovers() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let mut config = test_config(directory.path());
        config.max_disk_bytes = 512 * 1024 * 1024;
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(512, 64 * 1024 * 1024));
        // These exercise the DISK cap with multi-MiB payloads: keep the
        // ingest-time attribute cap out of the way.
        hot.set_max_attribute_bytes(0);
        store.attach_hot_storage(&hot);

        // ~24 MiB of history across distinct traces, oldest ingested first.
        for index in 0..24 {
            let mut span = test_span(&format!("trace-{index:02}"), "span", "bulk");
            span.attributes = vec![("payload".to_string(), "x".repeat(1024 * 1024))];
            hot.add_spans(vec![span]);
            store.flush().expect("persist bulk trace");
        }
        let before = directory_size(directory.path()).expect("measure populated directory");
        assert!(
            before > 16 * 1024 * 1024,
            "fixture must outgrow the new cap"
        );

        // Shrink the cap far below the populated size, then land a fresh
        // write: capacity enforcement must evict old traces and succeed.
        let shrunk = TraceStorageConfig {
            max_disk_bytes: 8 * 1024 * 1024,
            ..test_config(directory.path())
        };
        store.update_limits(&shrunk);
        let mut fresh = test_span("trace-fresh", "span", "fresh");
        fresh.attributes = vec![("payload".to_string(), "y".repeat(64 * 1024))];
        hot.add_spans(vec![fresh]);
        store
            .flush()
            .expect("write after cap shrink must evict and succeed");

        let after = directory_size(directory.path()).expect("measure evicted directory");
        assert!(
            after < before / 2,
            "eviction must return space to the filesystem ({before} -> {after})"
        );
        assert_eq!(store.status().archive, "healthy");

        let survivors = store.get_spans().expect("read survivors");
        assert!(
            survivors.iter().any(|span| span.trace_id == "trace-fresh"),
            "the fresh trace must survive"
        );
        assert!(
            !survivors.is_empty(),
            "eviction must not wipe the entire archive"
        );
        assert!(
            !survivors.iter().any(|span| span.trace_id == "trace-00"),
            "the oldest trace must be evicted first"
        );
        assert!(
            survivors.iter().any(|span| span.trace_id == "trace-23"),
            "the newest historical trace must be preserved, not overshot"
        );
        store.shutdown();
    }

    #[test]
    fn flush_persists_spans_and_reopen_reads_them() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(32, 1_000_000));
        store.attach_hot_storage(&hot);

        hot.add_spans(vec![test_span("trace-1", "span-1", "first")]);
        store.flush().expect("flush trace store");

        let stored = store.get_spans_by_trace_id("trace-1").expect("read trace");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name, "first");
        assert_eq!(stored[0].attributes[0].1, "[REDACTED]");
        store.record_dropped_spans(2);
        store.flush().expect("flush dropped count");
        assert_eq!(store.status().known_dropped_spans, 2);
        store.shutdown();
        drop(store);

        let reopened = TraceDiskStore::open(&config).expect("reopen trace store");
        let stored = reopened.get_spans().expect("read after restart");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].trace_id, "trace-1");
        assert_eq!(reopened.status().known_dropped_spans, 2);
        reopened.shutdown();
    }

    #[test]
    fn clear_removes_archive_rows_and_advances_epoch() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(32, 1_000_000));
        store.attach_hot_storage(&hot);
        hot.add_spans(vec![test_span("trace-1", "span-1", "first")]);
        store.flush().expect("flush trace store");

        let old_epoch = store.epoch.load(Ordering::Acquire);
        store.clear().expect("clear trace store");
        assert!(store.get_spans().expect("read cleared archive").is_empty());
        assert!(store.epoch.load(Ordering::Acquire) > old_epoch);
        store.shutdown();
    }

    #[test]
    fn root_query_does_not_materialize_child_spans() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(32, 1_000_000));
        store.attach_hot_storage(&hot);
        let root = test_span("trace-1", "root", "root");
        let mut child = test_span("trace-1", "child", "child");
        child.parent_span_id = Some("root".to_string());
        hot.add_spans(vec![root, child]);
        store.flush().expect("flush trace store");

        let roots = store.get_root_spans().expect("read trace roots");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].span_id, "root");
        assert_eq!(
            store
                .get_spans_by_attribute("authorization")
                .expect("read attribute spans")
                .len(),
            2
        );
        store.shutdown();
    }

    #[test]
    fn batched_reads_and_paged_reads_preserve_archive_results() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(32, 1_000_000));
        store.attach_hot_storage(&hot);

        let mut first = test_span("trace-1", "span-1", "first");
        first.start_time_unix_nano = 1_000;
        first
            .attributes
            .push(("iii.tag.session".to_string(), "earlier".to_string()));
        let mut second = test_span("trace-2", "span-2", "second");
        second.start_time_unix_nano = 3_000;
        second
            .attributes
            .push(("iii.tag.message".to_string(), "group-me".to_string()));
        second
            .attributes
            .push(("iii.session.name".to_string(), "trace two".to_string()));
        let mut third = test_span("trace-3", "span-3", "third");
        third.start_time_unix_nano = 2_000;
        hot.add_spans(vec![first, second, third]);
        store.flush().expect("flush trace store");

        let batched = store
            .get_spans_by_trace_ids(&[
                "trace-1".to_string(),
                "trace-2".to_string(),
                "trace-1".to_string(),
            ])
            .expect("read trace batch");
        assert_eq!(batched.len(), 2, "duplicate ids must not duplicate rows");
        assert_eq!(store.span_count().expect("count archive"), 3);
        assert_eq!(
            store
                .existing_span_key_count(&[
                    ("trace-1".to_string(), "span-1".to_string()),
                    ("missing".to_string(), "span-0".to_string()),
                ])
                .expect("count archive keys"),
            1
        );

        let page = store
            .get_spans_page_by_start_time(1, 1, false)
            .expect("read descending page");
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].trace_id, "trace-3");

        let groups = store
            .get_group_rows_by_attribute("iii.tag.message", None)
            .expect("read group projection");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].trace_id, "trace-2");
        assert_eq!(groups[0].value, "group-me");

        let tags = store
            .get_trace_tag_rows_by_trace_ids(&[
                "trace-1".to_string(),
                "trace-2".to_string(),
                "trace-1".to_string(),
            ])
            .expect("read batched trace tags");
        assert_eq!(tags.len(), 3, "only trace-tag attributes are projected");
        assert_eq!(tags[0].trace_id, "trace-1");
        assert_eq!(tags[0].key, "iii.tag.session");
        assert_eq!(tags[0].value, "earlier");
        assert_eq!(tags[2].trace_id, "trace-2");
        assert_eq!(tags[2].key, "iii.session.name");
        store.shutdown();
    }

    #[test]
    fn root_keys_match_the_full_root_query_without_payloads() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(32, 1_000_000));
        store.attach_hot_storage(&hot);

        let mut root_a = test_span("trace-a", "root-a", "a");
        root_a.start_time_unix_nano = 1_000;
        let mut child_a = test_span("trace-a", "child-a", "a-child");
        child_a.parent_span_id = Some("root-a".to_string());
        child_a.start_time_unix_nano = 1_500;
        // A second, dangling root of the same distributed trace.
        let mut remote_a = test_span("trace-a", "remote-a", "a-remote");
        remote_a.parent_span_id = Some("remote-span".to_string());
        remote_a.start_time_unix_nano = 1_700;
        let mut internal = test_span("trace-c", "root-c", "c");
        internal.start_time_unix_nano = 3_000;
        internal
            .attributes
            .push(("iii.function.kind".to_string(), "internal".to_string()));
        hot.add_spans(vec![root_a, child_a, remote_a, internal]);
        store.flush().expect("flush trace store");

        let mut keys = store.get_root_span_keys().expect("read root keys");
        keys.sort_by(|a, b| a.span_id.cmp(&b.span_id));
        assert_eq!(
            keys,
            vec![
                RootSpanKey {
                    trace_id: "trace-a".to_string(),
                    span_id: "remote-a".to_string(),
                    parent_span_id: Some("remote-span".to_string()),
                    name: "a-remote".to_string(),
                    service_name: "test".to_string(),
                    start_time_ns: 1_700,
                    is_internal: false,
                },
                RootSpanKey {
                    trace_id: "trace-a".to_string(),
                    span_id: "root-a".to_string(),
                    parent_span_id: None,
                    name: "a".to_string(),
                    service_name: "test".to_string(),
                    start_time_ns: 1_000,
                    is_internal: false,
                },
                RootSpanKey {
                    trace_id: "trace-c".to_string(),
                    span_id: "root-c".to_string(),
                    parent_span_id: None,
                    name: "c".to_string(),
                    service_name: "test".to_string(),
                    start_time_ns: 3_000,
                    is_internal: true,
                },
            ],
            "the same roots as the payload query, the child excluded"
        );
        assert_eq!(
            keys.len(),
            store.get_root_spans().expect("read full roots").len()
        );
    }

    #[test]
    fn attribute_and_name_lookups_read_no_payload() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(32, 1_000_000));
        store.attach_hot_storage(&hot);

        let mut root = test_span("trace-a", "root-a", "Turn");
        root.attributes = vec![("iii.session.id".to_string(), "s1".to_string())];
        let mut child = test_span("trace-a", "child-a", "Call Tool");
        child.parent_span_id = Some("root-a".to_string());
        child.attributes = vec![("iii.session.id".to_string(), "s1".to_string())];
        let mut other = test_span("trace-b", "root-b", "Call Tool");
        other.attributes = vec![("iii.session.id".to_string(), "s2".to_string())];
        hot.add_spans(vec![root, child, other]);
        store.flush().expect("flush trace store");

        assert_eq!(
            store
                .span_keys_with_attribute("iii.session.id", "s1")
                .expect("attribute lookup"),
            HashSet::from([
                ("trace-a".to_string(), "root-a".to_string()),
                ("trace-a".to_string(), "child-a".to_string()),
            ])
        );
        assert!(
            store
                .span_keys_with_attribute("iii.session.id", "missing")
                .expect("attribute lookup")
                .is_empty()
        );
        assert_eq!(
            store
                .trace_ids_with_span_name(|name| name.to_lowercase().contains("call"))
                .expect("name scan"),
            HashSet::from(["trace-a".to_string(), "trace-b".to_string()])
        );
        assert_eq!(
            store
                .trace_ids_with_span_name(|name| name == "Turn")
                .expect("name scan"),
            HashSet::from(["trace-a".to_string()])
        );
    }

    #[test]
    fn paged_root_reads_match_the_full_root_query() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let config = test_config(directory.path());
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(32, 1_000_000));
        store.attach_hot_storage(&hot);

        let mut root_a = test_span("trace-a", "root-a", "a");
        root_a.start_time_unix_nano = 1_000;
        let mut child_a = test_span("trace-a", "child-a", "a-child");
        child_a.parent_span_id = Some("root-a".to_string());
        child_a.start_time_unix_nano = 1_500;
        // Dangling parent: the remote caller's span is never stored → root.
        let mut dangling = test_span("trace-b", "root-b", "b");
        dangling.parent_span_id = Some("remote-span".to_string());
        dangling.start_time_unix_nano = 2_000;
        let mut internal_kind = test_span("trace-c", "root-c", "c");
        internal_kind.start_time_unix_nano = 3_000;
        internal_kind
            .attributes
            .push(("iii.function.kind".to_string(), "internal".to_string()));
        let mut internal_fn = test_span("trace-d", "root-d", "d");
        internal_fn.start_time_unix_nano = 4_000;
        internal_fn.attributes.push((
            "function_id".to_string(),
            "engine::traces::list".to_string(),
        ));
        hot.add_spans(vec![root_a, child_a, dangling, internal_kind, internal_fn]);
        store.flush().expect("flush trace store");

        assert_eq!(store.root_span_count(true).expect("count all roots"), 4);
        assert_eq!(
            store.root_span_count(false).expect("count external roots"),
            2
        );

        let all_roots = store
            .get_root_spans_page_by_start_time(0, 10, true, true)
            .expect("page all roots");
        assert_eq!(
            all_roots
                .iter()
                .map(|span| span.span_id.as_str())
                .collect::<Vec<_>>(),
            vec!["root-a", "root-b", "root-c", "root-d"],
            "paged roots must match the full root query, children excluded"
        );

        let external = store
            .get_root_spans_page_by_start_time(0, 10, false, false)
            .expect("page external roots descending");
        assert_eq!(
            external
                .iter()
                .map(|span| span.span_id.as_str())
                .collect::<Vec<_>>(),
            vec!["root-b", "root-a"],
            "internal roots excluded, descending order"
        );

        let window = store
            .get_root_spans_page_by_start_time(1, 1, true, true)
            .expect("offset/limit window");
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].span_id, "root-b");

        assert_eq!(
            store
                .existing_root_span_key_count(
                    &[
                        ("trace-a".to_string(), "root-a".to_string()),
                        ("trace-a".to_string(), "child-a".to_string()),
                        ("trace-c".to_string(), "root-c".to_string()),
                        ("missing".to_string(), "missing".to_string()),
                    ],
                    false,
                )
                .expect("count shadowed external roots"),
            1,
            "children, internal roots and absent keys must not count"
        );
        assert_eq!(
            store
                .existing_span_keys(&[
                    ("trace-a".to_string(), "child-a".to_string()),
                    ("missing".to_string(), "missing".to_string()),
                ])
                .expect("look up span keys"),
            HashSet::from([("trace-a".to_string(), "child-a".to_string())])
        );
        store.shutdown();
    }

    /// The per-trace aggregate runs once per trace touched by a batch, not
    /// once per span; the counts it leaves behind must still be exact.
    #[test]
    fn trace_meta_aggregates_once_per_trace_and_stays_correct() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let store = TraceDiskStore::open(&test_config(directory.path())).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(64, 16 * 1024 * 1024));
        store.attach_hot_storage(&hot);

        hot.add_spans(vec![
            test_span("trace-a", "span-1", "a1"),
            test_span("trace-a", "span-2", "a2"),
            test_span("trace-b", "span-1", "b1"),
            test_span("trace-a", "span-3", "a3"),
        ]);
        store.flush().expect("persist first batch");
        // A later batch touches trace-a again: the aggregate must see all four rows.
        hot.add_spans(vec![test_span("trace-a", "span-4", "a4")]);
        store.flush().expect("persist second batch");

        let probe =
            Connection::open(directory.path().join("traces.sqlite3")).expect("probe connection");
        let (count, bytes): (i64, i64) = probe
            .query_row(
                "SELECT span_count, approx_bytes FROM trace_meta WHERE trace_id = 'trace-a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("trace-a meta");
        let expected_bytes: i64 = probe
            .query_row(
                "SELECT COALESCE(SUM(approx_bytes), 0) FROM spans WHERE trace_id = 'trace-a'",
                [],
                |row| row.get(0),
            )
            .expect("trace-a byte sum");
        assert_eq!(count, 4);
        assert_eq!(bytes, expected_bytes);
        let count_b: i64 = probe
            .query_row(
                "SELECT span_count FROM trace_meta WHERE trace_id = 'trace-b'",
                [],
                |row| row.get(0),
            )
            .expect("trace-b meta");
        assert_eq!(count_b, 1);
        store.shutdown();
    }

    #[test]
    fn hot_cache_preserves_the_existing_memory_only_eviction_contract() {
        let storage = InMemorySpanStorage::new_with_limits_and_watermark(1, 1_000, 0.75);
        let mut pending = test_span("trace-pending", "span-pending", "pending");
        pending.pending = true;
        pending.end_time_unix_nano = 0;
        storage.add_pending_span(pending);
        storage.add_spans(vec![test_span("trace-final", "span-final", "final")]);

        // With durable storage disabled, keep the legacy circular-buffer
        // contract: pending snapshots are eligible for memory pressure
        // eviction. Durable mode enables the protection path.
        assert!(storage.get_spans_by_trace_id("trace-pending").is_empty());
        assert_eq!(storage.get_spans_by_trace_id("trace-final").len(), 1);
    }

    #[test]
    fn corrupt_archive_is_rejected_without_replacing_the_original_file() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let database_path = directory.path().join("traces.sqlite3");
        let corrupt_contents = b"this is not a sqlite database";
        fs::write(&database_path, corrupt_contents).expect("write corrupt archive fixture");

        let error = TraceDiskStore::open(&test_config(directory.path()))
            .expect_err("a corrupt archive must not start a trace writer");

        assert!(
            error.contains("cannot initialize trace database"),
            "unexpected corrupt archive error: {error}"
        );
        assert_eq!(
            fs::read(&database_path).expect("read corrupt archive fixture"),
            corrupt_contents,
            "opening a corrupt archive must not replace the original evidence"
        );
    }

    #[test]
    fn disk_cap_drops_an_irrecoverable_span_and_keeps_its_trace_durable_rows() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let mut config = test_config(directory.path());
        // The capacity guard has an 8 MiB SQLite allocation tolerance. Keep
        // the configured limit below it, then write 1 MiB and 3 MiB spans from
        // one trace. SQLite stores each attribute in the span payload, its
        // attribute table and lookup index, so the 3 MiB span alone needs more
        // than the allowance: it is dropped, while the durable 1 MiB row of the
        // same trace is kept.
        config.max_disk_bytes = 1;
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(16, 20 * 1024 * 1024));
        // These exercise the DISK cap with multi-MiB payloads: keep the
        // ingest-time attribute cap out of the way.
        hot.set_max_attribute_bytes(0);
        store.attach_hot_storage(&hot);

        let mut committed = test_span("trace-protected", "span-1", "large");
        committed.attributes = vec![("payload".to_string(), "x".repeat(1024 * 1024))];
        hot.add_spans(vec![committed]);
        store
            .flush()
            .expect("persist the first span before the cap is reached");

        let mut rejected = test_span("trace-protected", "span-2", "large");
        rejected.attributes = vec![("payload".to_string(), "x".repeat(3 * 1024 * 1024))];
        hot.add_spans(vec![rejected]);
        store
            .flush()
            .expect("the irrecoverable span is settled without blocking the writer");
        assert_eq!(
            archived_span_ids(directory.path(), "trace-protected"),
            vec!["span-1"],
            "the durable row is kept and nothing is partially written"
        );
        assert_eq!(hot.dirty_len(), 0, "the span must not be retried forever");
        assert_eq!(store.status().known_dropped_spans, 1);
        assert_eq!(store.status().completeness, "partial");

        store.shutdown();
        assert_eq!(store.status().archive, "healthy");
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_write_limit_degrades_archive_without_partial_commit() {
        const CHILD_MARKER: &str = "III_TRACE_STORE_FILE_LIMIT_CHILD";
        const CHILD_DIRECTORY: &str = "III_TRACE_STORE_FILE_LIMIT_DIRECTORY";

        if std::env::var_os(CHILD_MARKER).is_some() {
            let _lift = LiftFileSizeLimitOnDrop;
            let directory = PathBuf::from(
                std::env::var_os(CHILD_DIRECTORY).expect("child trace directory must be provided"),
            );
            let store = TraceDiskStore::open(&test_config(&directory)).expect("open trace store");
            let hot = Arc::new(InMemorySpanStorage::new_with_limits(16, 20 * 1024 * 1024));
            // These exercise the DISK cap with multi-MiB payloads: keep the
            // ingest-time attribute cap out of the way.
            hot.set_max_attribute_bytes(0);
            store.attach_hot_storage(&hot);

            let mut span = test_span("trace-file-limit", "span-1", "large");
            span.attributes = vec![("payload".to_string(), "x".repeat(3 * 1024 * 1024))];
            hot.add_spans(vec![span]);
            store.notify();

            let deadline = Instant::now() + Duration::from_secs(5);
            while !store.is_degraded() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }

            let status = store.status();
            assert_eq!(status.archive, "degraded");
            assert!(
                status.last_error.as_deref().is_some_and(|error| {
                    error.contains("cannot persist trace")
                        || error.contains("disk I/O error")
                        || error.contains("database or disk is full")
                }),
                "unexpected filesystem-limit error: {:?}",
                status.last_error
            );
            assert_eq!(
                store
                    .span_count()
                    .expect("count archive after failed write"),
                0,
                "a filesystem write failure must not commit a partial span"
            );
            store.shutdown();
            return;
        }

        let directory = tempfile::tempdir().expect("temp trace directory");
        let executable = std::env::current_exe().expect("locate trace-store test binary");
        let output = Command::new("bash")
            .args([
                "-c",
                // `ulimit -S -f` applies only to the isolated child process,
                // which lifts it again before exiting (soft limit only).
                // Ignore SIGXFSZ so SQLite receives EFBIG and the archive can
                // report degradation instead of terminating the process.
                "ulimit -S -f 1024; trap '' XFSZ; exec \"$1\" --exact \"$2\" --nocapture",
                "trace-store-file-limit-child",
            ])
            .arg(executable)
            .arg("workers::observability::trace_store::tests::filesystem_write_limit_degrades_archive_without_partial_commit")
            .env(CHILD_MARKER, "1")
            .env(CHILD_DIRECTORY, directory.path())
            .output()
            .expect("run filesystem-limit child test");

        assert!(
            output.status.success(),
            "filesystem-limit child failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    const MIB: u64 = 1024 * 1024;

    fn capped_config(directory: &Path, max_disk_bytes: u64) -> TraceStorageConfig {
        TraceStorageConfig {
            max_disk_bytes,
            ..test_config(directory)
        }
    }

    fn span_with(trace_id: &str, span_id: &str, attributes: Vec<(String, String)>) -> StoredSpan {
        let mut span = test_span(trace_id, span_id, "regression");
        span.attributes = attributes;
        span
    }

    fn wide_attributes(count: usize, len: usize) -> Vec<(String, String)> {
        (0..count)
            .map(|index| (format!("field-{index:05}"), "x".repeat(len)))
            .collect()
    }

    fn payload_attribute(len: usize) -> Vec<(String, String)> {
        vec![("iii.payload.json".to_string(), "x".repeat(len))]
    }

    fn hot_cache(max_spans: usize, max_bytes: u64) -> Arc<InMemorySpanStorage> {
        Arc::new(InMemorySpanStorage::new_with_limits(max_spans, max_bytes))
    }

    fn archived_span_ids(directory: &Path, trace_id: &str) -> Vec<String> {
        let connection = Connection::open(directory.join("traces.sqlite3")).unwrap();
        let mut statement = connection
            .prepare("SELECT span_id FROM spans WHERE trace_id = ?1 ORDER BY span_id")
            .unwrap();
        statement
            .query_map(params![trace_id], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap()
    }

    fn image_bytes(directory: &Path) -> u64 {
        let connection = Connection::open(directory.join("traces.sqlite3")).unwrap();
        let stats = page_stats(&connection).unwrap();
        stats.pages * stats.page_size
    }

    fn lower_bound_of(span: &StoredSpan) -> u64 {
        span_lower_bound(span, &serde_json::to_string(span).unwrap())
    }

    /// Process-wide /proc counters include every concurrent libtest case.
    /// Run resource regressions alone in a fresh process, not merely behind a
    /// serial_test lock (unannotated tests would still run alongside them).
    /// Inherit LLVM_PROFILE_FILE and the rest of the environment so coverage
    /// from the child is collected just like the existing subprocess tests.
    /// Returns true in the parent, which must return without running the body.
    fn run_resource_test_in_child(test_name: &str) -> bool {
        const CHILD_TEST: &str = "III_TRACE_STORE_RESOURCE_CHILD";
        if std::env::var(CHILD_TEST).as_deref() == Ok(test_name) {
            return false;
        }
        let full_name = format!("workers::observability::trace_store::tests::{test_name}");
        let output = std::process::Command::new(
            std::env::current_exe().expect("locate resource test binary"),
        )
        .args(["--exact", &full_name, "--test-threads=1", "--nocapture"])
        .env(CHILD_TEST, test_name)
        .output()
        .expect("run isolated archive resource test");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stdout.contains("1 passed;"),
            "isolated resource test {full_name} failed or did not run: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status,
        );
        eprint!("{stderr}");
        true
    }

    /// `wchar` of this process: bytes passed to write syscalls.
    fn process_write_chars() -> u64 {
        fs::read_to_string("/proc/self/io")
            .unwrap_or_default()
            .lines()
            .find_map(|line| line.strip_prefix("wchar:"))
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    }

    fn peak_resident_kib() -> u64 {
        fs::read_to_string("/proc/self/status")
            .unwrap_or_default()
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))
            .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
            .unwrap_or(0)
    }

    /// Largest directory size seen by a 1 ms poller until `finish`.
    struct DirectoryPeak {
        stop: Arc<AtomicBool>,
        handle: thread::JoinHandle<u64>,
    }

    impl DirectoryPeak {
        fn watch(directory: &Path) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let directory = directory.to_path_buf();
            let handle = thread::spawn(move || {
                let mut peak = 0;
                while !flag.load(Ordering::Relaxed) {
                    peak = peak.max(directory_size(&directory).unwrap_or(0));
                    thread::sleep(Duration::from_millis(1));
                }
                peak
            });
            Self { stop, handle }
        }

        fn finish(self) -> u64 {
            self.stop.store(true, Ordering::Relaxed);
            self.handle.join().unwrap()
        }
    }

    /// A live trace whose finished children already fill the archive: it is
    /// protected while its root is pending.
    fn archive_live_trace(
        store: &TraceDiskStore,
        hot: &InMemorySpanStorage,
        spans: usize,
    ) -> StoredSpan {
        for chunk in 0..spans.div_ceil(50) {
            let count = 50.min(spans - chunk * 50);
            hot.add_spans(
                (0..count)
                    .map(|index| {
                        span_with(
                            "live",
                            &format!("done-{chunk}-{index:02}"),
                            payload_attribute(64 * 1024),
                        )
                    })
                    .collect(),
            );
            store.flush().unwrap();
        }
        let mut root = test_span("live", "root-live", "live");
        root.pending = true;
        root.end_time_unix_nano = 0;
        hot.add_pending_span(root.clone());
        root
    }

    /// Review R1: a span that can never fit is dropped before anything is
    /// evicted for it; unrelated durable history survives.
    #[test]
    fn irrecoverable_trace_is_dropped_without_evicting_unrelated_history() {
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), 64 * MIB)).unwrap();
        let hot = hot_cache(4096, 1024 * MIB);
        store.attach_hot_storage(&hot);
        hot.add_spans(
            (0..20)
                .map(|index| {
                    span_with(
                        &format!("keep-{index:02}"),
                        "s",
                        payload_attribute(64 * 1024),
                    )
                })
                .collect(),
        );
        store.flush().unwrap();
        hot.add_spans(vec![span_with(
            "wide-trace",
            "wide",
            wide_attributes(400, 64 * 1024),
        )]);

        store
            .flush()
            .expect("an irrecoverable span is settled, not retried");

        let survivors = store.get_spans().unwrap();
        let status = store.status();
        eprintln!(
            "R1_EVIDENCE unrelated_after={} dropped={} dirty={} physical={}",
            survivors.len(),
            status.known_dropped_spans,
            hot.dirty_len(),
            status.physical_bytes
        );
        assert_eq!(
            survivors.len(),
            20,
            "unrelated durable history must survive"
        );
        assert_eq!(status.known_dropped_spans, 1);
        assert_eq!(hot.dirty_len(), 0);
        store.shutdown();
    }

    /// Review R2: the lower bound fits but the real SQLite image does not.
    /// The trace is dropped once, normal traces stay archived, and an idle
    /// writer stops rewriting the file.
    #[test]
    fn trace_whose_real_image_exceeds_the_limit_is_dropped_although_its_lower_bound_fits() {
        if run_resource_test_in_child(
            "trace_whose_real_image_exceeds_the_limit_is_dropped_although_its_lower_bound_fits",
        ) {
            return;
        }
        let max = 16 * MIB;
        let threshold = max + 8 * MIB;
        let wide = span_with("aaa-wide", "wide", wide_attributes(1500, 5085));
        let lower_bound = lower_bound_of(&wide);
        let real_image = {
            let directory = tempfile::tempdir().unwrap();
            let store = TraceDiskStore::open(&capped_config(directory.path(), 4096 * MIB)).unwrap();
            let hot = hot_cache(16, 1024 * MIB);
            store.attach_hot_storage(&hot);
            hot.add_spans(vec![wide.clone()]);
            store.flush().unwrap();
            store.shutdown();
            image_bytes(directory.path())
        };
        assert!(
            lower_bound <= threshold,
            "fixture: bound {lower_bound} fits"
        );
        assert!(
            real_image > threshold,
            "fixture: image {real_image} does not"
        );

        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), max)).unwrap();
        let hot = hot_cache(8192, 1024 * MIB);
        store.attach_hot_storage(&hot);
        hot.add_spans(vec![wide]);
        hot.add_spans(
            (0..12)
                .map(|index| test_span(&format!("zzz-normal-{index:02}"), "s", "normal"))
                .collect(),
        );
        let peak = DirectoryPeak::watch(directory.path());
        store
            .flush()
            .expect("the impossible trace is settled in one flush");
        let archived_after_flush = store.get_spans().unwrap().len();
        hot.add_spans(vec![test_span("zzz-late", "s", "late")]);
        thread::sleep(Duration::from_millis(1500));
        let late = store.get_spans_by_trace_id("zzz-late").unwrap().len();
        let writes_before = process_write_chars();
        thread::sleep(Duration::from_secs(2));
        let idle_writes = process_write_chars().saturating_sub(writes_before);
        let max_directory = peak.finish();
        let archived = store.get_spans().unwrap().len();
        let status = store.status();
        eprintln!(
            "R2_EVIDENCE lower_bound={lower_bound} real_image={real_image} threshold={threshold} archived_after_flush={archived_after_flush} late={late} archived={archived} dirty={} dropped={} archive={} max_dir={max_directory} idle_wchar_2s={idle_writes}",
            hot.dirty_len(),
            status.known_dropped_spans,
            status.archive
        );
        assert_eq!(
            archived_after_flush, 12,
            "normal traces progress in the same flush"
        );
        assert_eq!(late, 1, "a later span is archived by the periodic writer");
        assert_eq!(archived, 13, "normal traces are not evicted afterwards");
        assert_eq!(hot.dirty_len(), 0);
        assert_eq!(status.known_dropped_spans, 1);
        assert_eq!(status.archive, "healthy");
        assert!(
            max_directory <= threshold,
            "directory peaked at {max_directory}"
        );
        assert!(
            idle_writes < 16 * MIB,
            "idle writer wrote {idle_writes} bytes in 2 s"
        );
        store.shutdown();
    }

    /// Review R6: the newest version of a span decides. An impossible update
    /// of an archived span is dropped and counted; the durable version stays.
    #[test]
    fn updated_span_is_judged_by_its_newest_version() {
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), 64 * MIB)).unwrap();
        let hot = hot_cache(4096, 1024 * MIB);
        store.attach_hot_storage(&hot);
        hot.add_spans(vec![test_span("upd-trace", "same-span", "small")]);
        store.flush().unwrap();
        hot.add_spans(vec![span_with(
            "upd-trace",
            "same-span",
            wide_attributes(400, 64 * 1024),
        )]);

        store.flush().expect("the impossible update is settled");

        let status = store.status();
        let kept = store.get_spans_by_trace_id("upd-trace").unwrap();
        eprintln!(
            "R6_EVIDENCE dirty={} dropped={} archived={:?}",
            hot.dirty_len(),
            status.known_dropped_spans,
            archived_span_ids(directory.path(), "upd-trace")
        );
        assert_eq!(hot.dirty_len(), 0, "the update is not retried forever");
        assert_eq!(status.known_dropped_spans, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].attributes.len(), 1, "the durable version is kept");

        // Two dirty versions: the newest one is archived, the superseded
        // impossible one is not counted as lost.
        hot.add_spans(vec![span_with(
            "upd-2",
            "s",
            wide_attributes(400, 64 * 1024),
        )]);
        hot.add_spans(vec![test_span("upd-2", "s", "small")]);
        store.flush().unwrap();
        assert_eq!(archived_span_ids(directory.path(), "upd-2"), vec!["s"]);
        assert_eq!(store.status().known_dropped_spans, 1);
        assert_eq!(hot.dirty_len(), 0);
        store.shutdown();
    }

    /// Review R7: dropping an irrecoverable span keeps the trace's durable
    /// rows, and later spans of the same trace are still archived.
    #[test]
    fn irrecoverable_span_keeps_durable_rows_and_later_spans_of_its_trace() {
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), 64 * MIB)).unwrap();
        let hot = hot_cache(4096, 1024 * MIB);
        store.attach_hot_storage(&hot);
        hot.add_spans(vec![
            test_span("frag", "root", "root"),
            test_span("frag", "child-1", "child"),
            test_span("frag", "child-2", "child"),
        ]);
        store.flush().unwrap();
        hot.add_spans(vec![span_with(
            "frag",
            "child-wide",
            wide_attributes(400, 64 * 1024),
        )]);
        store.flush().unwrap();
        let after_drop = archived_span_ids(directory.path(), "frag");
        hot.add_spans(vec![test_span("frag", "child-late", "child")]);
        store.flush().unwrap();
        let after_late = archived_span_ids(directory.path(), "frag");
        eprintln!(
            "R7_EVIDENCE after_drop={after_drop:?} after_late={after_late:?} dropped={}",
            store.status().known_dropped_spans
        );
        assert_eq!(after_drop, vec!["child-1", "child-2", "root"]);
        assert_eq!(after_late, vec!["child-1", "child-2", "child-late", "root"]);
        assert_eq!(store.status().known_dropped_spans, 1);
        store.shutdown();
    }

    /// Review R8: spans of the same trace ingested while its irrecoverable
    /// span is dropped are judged on their own; every loss counts once.
    #[test]
    fn concurrent_spans_of_a_trace_with_an_irrecoverable_span_are_counted_exactly_once() {
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), 64 * MIB)).unwrap();
        let hot = hot_cache(1_000_000, 4096 * MIB);
        store.attach_hot_storage(&hot);
        hot.add_spans(vec![span_with(
            "race",
            "wide",
            wide_attributes(400, 64 * 1024),
        )]);
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, producer) = (stop.clone(), hot.clone());
        let adder = thread::spawn(move || {
            let mut sent = 0u64;
            while !flag.load(Ordering::Relaxed) {
                producer.add_spans(vec![test_span("race", &format!("late-{sent:07}"), "late")]);
                sent += 1;
                thread::sleep(Duration::from_micros(100));
            }
            sent
        });
        let first = store.flush();
        thread::sleep(Duration::from_millis(300));
        stop.store(true, Ordering::Relaxed);
        let sent = adder.join().unwrap();
        for _ in 0..20 {
            if store.flush().is_ok() && hot.dirty_len() == 0 {
                break;
            }
        }
        let archived = archived_span_ids(directory.path(), "race").len() as u64;
        let lost = 1 + sent - archived;
        let status = store.status();
        eprintln!(
            "R8_EVIDENCE first_flush={first:?} late_sent={sent} archived={archived} lost={lost} known_dropped={} dirty={}",
            status.known_dropped_spans,
            hot.dirty_len()
        );
        assert_eq!(
            status.known_dropped_spans, lost,
            "every lost span counts exactly once"
        );
        assert_eq!(lost, 1, "only the irrecoverable span is lost");
        assert_eq!(hot.dirty_len(), 0);
        store.shutdown();
    }

    /// Review R10: a protected live trace holds more than a lowered cap.
    /// Nothing fits, so the writer backs off: flush and retain answer quickly
    /// with little I/O. Once the trace finishes, its newest span (which can
    /// never fit beside its durable rows) is dropped, the trace becomes
    /// evictable and the blocked traces are archived.
    #[test]
    fn blocked_archive_answers_controls_quickly_and_resumes_when_protection_ends() {
        if run_resource_test_in_child(
            "blocked_archive_answers_controls_quickly_and_resumes_when_protection_ends",
        ) {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), 1024 * MIB)).unwrap();
        let hot = hot_cache(100_000, 4096 * MIB);
        store.attach_hot_storage(&hot);
        let root = archive_live_trace(&store, &hot, 200);
        let protected_bytes = directory_size(directory.path()).unwrap();
        store.update_limits(&capped_config(directory.path(), 16 * MIB));
        hot.add_spans(
            (0..300)
                .map(|index| test_span(&format!("n-{index:05}"), "s", "normal"))
                .collect(),
        );

        let writes_before = process_write_chars();
        let started = Instant::now();
        let flush = store.flush();
        let flush_elapsed = started.elapsed();
        let started = Instant::now();
        let retain = store.retain();
        let retain_elapsed = started.elapsed();
        thread::sleep(Duration::from_secs(2));
        let writes = process_write_chars().saturating_sub(writes_before);
        eprintln!(
            "R10_EVIDENCE protected_bytes={protected_bytes} flush={flush:?} flush_ms={} retain={retain:?} retain_ms={} wchar_with_2s_ticks={writes} dirty={} warnings={}",
            flush_elapsed.as_millis(),
            retain_elapsed.as_millis(),
            hot.dirty_len(),
            store.warnings_emitted()
        );
        assert!(flush.is_err_and(|error| error.contains("trace storage limit reached")));
        assert!(
            flush_elapsed < Duration::from_secs(2),
            "flush took {flush_elapsed:?}"
        );
        assert!(retain.is_err());
        assert!(
            retain_elapsed < Duration::from_secs(2),
            "retain took {retain_elapsed:?}"
        );
        assert_eq!(hot.dirty_len(), 300, "blocked spans stay dirty");
        assert!(writes < 64 * MIB, "blocked writer wrote {writes} bytes");
        assert_eq!(store.status().archive, "degraded");
        assert_eq!(store.warnings_emitted(), 1, "capacity warnings are limited");

        let mut finished = root;
        finished.pending = false;
        finished.end_time_unix_nano = 2_000_000_000;
        hot.add_spans(vec![finished]);
        for _ in 0..5 {
            if store.flush().is_ok() && hot.dirty_len() == 0 {
                break;
            }
        }
        let status = store.status();
        eprintln!(
            "R10_RESUME_EVIDENCE dirty={} dropped={} live_rows={} physical={} archive={}",
            hot.dirty_len(),
            status.known_dropped_spans,
            archived_span_ids(directory.path(), "live").len(),
            status.physical_bytes,
            status.archive
        );
        assert_eq!(hot.dirty_len(), 0);
        assert_eq!(store.get_spans_by_trace_id("n-00299").unwrap().len(), 1);
        assert_eq!(
            status.known_dropped_spans, 1,
            "only the live root could not fit"
        );
        assert!(status.physical_bytes <= 16 * MIB + 8 * MIB);
        assert_eq!(status.archive, "healthy");
        store.shutdown();
    }

    /// Review R11: a blocked flush copies one bounded batch, not the whole
    /// dirty set, and answers promptly. The subprocess isolates VmHWM.
    #[test]
    fn blocked_flush_does_not_copy_the_dirty_set() {
        if run_resource_test_in_child("blocked_flush_does_not_copy_the_dirty_set") {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), 1024 * MIB)).unwrap();
        let hot = hot_cache(100_000, 4096 * MIB);
        store.attach_hot_storage(&hot);
        archive_live_trace(&store, &hot, 200);
        store.update_limits(&capped_config(directory.path(), 16 * MIB));
        hot.add_spans(
            (0..1200)
                .map(|index| span_with(&format!("n-{index:05}"), "s", payload_attribute(64 * 1024)))
                .collect(),
        );
        let peak_before = peak_resident_kib();
        let started = Instant::now();
        let flush = store.flush();
        let elapsed = started.elapsed();
        thread::sleep(Duration::from_secs(1));
        let peak_after = peak_resident_kib();
        eprintln!(
            "R11_EVIDENCE dirty={} flush={flush:?} flush_ms={} vm_hwm_before_kib={peak_before} vm_hwm_after_kib={peak_after}",
            hot.dirty_len(),
            elapsed.as_millis()
        );
        assert!(flush.is_err());
        assert!(elapsed < Duration::from_secs(2), "flush took {elapsed:?}");
        assert_eq!(hot.dirty_len(), 1200);
        assert!(
            peak_after.saturating_sub(peak_before) < 32 * 1024,
            "peak RSS grew {} KiB",
            peak_after.saturating_sub(peak_before)
        );
        store.shutdown();
    }

    /// Review R3/P2-7: emptying a 57 MB archive by retention returns the space
    /// in checkpointed steps; the directory never exceeds limit plus
    /// tolerance and the space is back when `retain` returns.
    #[test]
    fn retention_returns_space_without_exceeding_limit_plus_tolerance() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = capped_config(directory.path(), 64 * MIB);
        config.retention_seconds = 1;
        let store = TraceDiskStore::open(&config).unwrap();
        let hot = hot_cache(8192, 1024 * MIB);
        store.attach_hot_storage(&hot);
        for batch in 0..40 {
            hot.add_spans(
                (0..10)
                    .map(|index| {
                        span_with(
                            &format!("t-{batch:02}-{index}"),
                            "s",
                            payload_attribute(64 * 1024),
                        )
                    })
                    .collect(),
            );
            store.flush().unwrap();
        }
        let rows_before = store.span_count().unwrap();
        thread::sleep(Duration::from_millis(1500));
        let physical_before = directory_size(directory.path()).unwrap();
        let peak = DirectoryPeak::watch(directory.path());
        let started = Instant::now();
        store.retain().unwrap();
        let elapsed = started.elapsed();
        let after = directory_size(directory.path()).unwrap();
        let max_directory = peak.finish();
        let rows_after = store.span_count().unwrap();
        eprintln!(
            "R3_EVIDENCE rows_before={rows_before} physical_before={physical_before} retain_ms={} peak_during_retain={max_directory} physical_after_retain={after} rows_after={rows_after}",
            elapsed.as_millis()
        );
        assert_eq!(rows_after, 0);
        assert!(
            max_directory <= 64 * MIB + 8 * MIB,
            "directory peaked at {max_directory}"
        );
        assert!(
            after <= MIB,
            "space must be returned when retain returns: {after}"
        );
        store.shutdown();
    }

    /// Review P3-14: a trace that fits an empty archive but not the current
    /// one stays dirty (others progress), then is archived once the protected
    /// trace that held the space finishes.
    #[test]
    fn temporarily_blocked_trace_stays_dirty_and_is_archived_later() {
        let directory = tempfile::tempdir().unwrap();
        // max 1 B: limit plus tolerance is 8 MiB.
        let store = TraceDiskStore::open(&capped_config(directory.path(), 1)).unwrap();
        let hot = hot_cache(64, 64 * MIB);
        hot.set_max_attribute_bytes(0);
        store.attach_hot_storage(&hot);
        for part in 0..2 {
            hot.add_spans(vec![span_with(
                "holder",
                &format!("done-{part}"),
                payload_attribute(MIB as usize),
            )]);
            store.flush().unwrap();
        }
        let mut root = test_span("holder", "root", "holder");
        root.pending = true;
        root.end_time_unix_nano = 0;
        hot.add_pending_span(root.clone());

        hot.add_spans(vec![
            span_with("blocked", "s", payload_attribute(700 * 1024)),
            test_span("small", "s", "small"),
        ]);
        let first = store.flush();
        let small_archived = store.get_spans_by_trace_id("small").unwrap().len();
        let blocked_dirty = hot.dirty_len();

        root.pending = false;
        root.end_time_unix_nano = 2_000_000_000;
        hot.add_spans(vec![root]);
        for _ in 0..5 {
            if store.flush().is_ok() && hot.dirty_len() == 0 {
                break;
            }
        }
        let status = store.status();
        eprintln!(
            "BLOCKED_TRACE_EVIDENCE first={first:?} small_archived={small_archived} dirty_while_blocked={blocked_dirty} blocked_archived={} dirty={} dropped={} archive={}",
            store.get_spans_by_trace_id("blocked").unwrap().len(),
            hot.dirty_len(),
            status.known_dropped_spans,
            status.archive
        );
        assert!(first.is_err_and(|error| error.contains("trace storage limit reached")));
        assert_eq!(small_archived, 1, "independent traces progress");
        assert_eq!(
            blocked_dirty, 1,
            "the blocked trace stays dirty, not dropped"
        );
        assert_eq!(store.get_spans_by_trace_id("blocked").unwrap().len(), 1);
        assert_eq!(hot.dirty_len(), 0);
        assert_eq!(status.known_dropped_spans, 0, "nothing is lost");
        assert_eq!(status.archive, "healthy");
        store.shutdown();
    }

    /// Review P3-12: a backlog longer than one tick's budget is drained over
    /// several ticks without the archive ever reporting degraded, and a flush
    /// covering more than 32 batches succeeds.
    #[test]
    fn long_backlog_is_progress_not_degradation() {
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), 256 * MIB)).unwrap();
        let hot = hot_cache(100_000, 1024 * MIB);
        store.attach_hot_storage(&hot);
        hot.add_spans(
            (0..20_000)
                .map(|index| test_span(&format!("b-{index:05}"), "s", "backlog"))
                .collect(),
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut degraded_seen = false;
        while hot.dirty_len() > 0 && Instant::now() < deadline {
            degraded_seen |= store.status().archive == "degraded";
            thread::sleep(Duration::from_millis(5));
        }
        let drained = hot.dirty_len();

        hot.add_spans(
            (0..17_000)
                .map(|index| test_span(&format!("f-{index:05}"), "s", "flush"))
                .collect(),
        );
        let flush = flush_settled(&store, &hot);
        eprintln!(
            "BACKLOG_EVIDENCE degraded_seen={degraded_seen} dirty_after_ticks={drained} flush={flush:?} archived={}",
            store.span_count().unwrap()
        );
        assert!(
            !degraded_seen,
            "partial progress must not degrade the archive"
        );
        assert_eq!(drained, 0);
        flush.expect("a flush over more than 32 batches completes");
        assert_eq!(store.span_count().unwrap(), 37_000);
        store.shutdown();
    }

    /// Review P3-11: the full-VACUUM fallback runs at most once per interval
    /// on the real eviction path, and its failure is warned once.
    #[test]
    fn failing_full_vacuum_fallback_is_attempted_and_warned_at_most_once() {
        let directory = tempfile::tempdir().unwrap();
        // max 1 B: every write evicts older traces, and deleting a tiny trace
        // never shrinks the file, so each write reaches the fallback.
        let store = TraceDiskStore::open(&capped_config(directory.path(), 1)).unwrap();
        let hot = hot_cache(1024, 64 * MIB);
        store.attach_hot_storage(&hot);
        store.fail_full_vacuum.store(true, Ordering::Relaxed);
        for index in 0..20 {
            hot.add_spans(vec![test_span(&format!("t-{index:02}"), "s", "small")]);
            store.flush().unwrap();
        }
        let attempts = store.full_vacuum_attempts.load(Ordering::Relaxed);
        let warnings = store.warnings_emitted();
        eprintln!(
            "VACUUM_EVIDENCE flushes=20 full_vacuum_attempts={attempts} warnings_emitted={warnings}"
        );
        assert_eq!(attempts, 1, "the fallback is rate limited");
        assert_eq!(warnings, 1, "the failure is warned once");
        store.shutdown();
    }

    /// Review R12: drops and repeated capacity errors on the real writer path
    /// produce one warning line per minute.
    #[test]
    fn writer_warnings_are_limited_on_the_real_path() {
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), 64 * MIB)).unwrap();
        let hot = hot_cache(100_000, 4096 * MIB);
        store.attach_hot_storage(&hot);
        for index in 0..10 {
            hot.add_spans(vec![span_with(
                &format!("imp-{index:02}"),
                "w",
                wide_attributes(400, 64 * 1024),
            )]);
            store.flush().unwrap();
        }
        archive_live_trace(&store, &hot, 50);
        store.update_limits(&capped_config(directory.path(), 1));
        hot.add_spans(vec![test_span("after", "s", "after")]);
        thread::sleep(Duration::from_secs(3));
        let status = store.status();
        eprintln!(
            "WARN_EVIDENCE dropped={} archive={} warnings_emitted={}",
            status.known_dropped_spans,
            status.archive,
            store.warnings_emitted()
        );
        assert_eq!(status.known_dropped_spans, 10);
        assert_eq!(status.archive, "degraded");
        assert_eq!(store.warnings_emitted(), 1);
        store.shutdown();
    }

    /// The loss count is committed before `flush` answers, so it survives a
    /// crash right after; the restarted archive reports `partial`.
    #[cfg(unix)]
    #[test]
    fn dropped_span_count_survives_a_crash_after_flush() {
        const CHILD_MARKER: &str = "III_TRACE_STORE_DROP_CRASH_CHILD";
        const CHILD_DIRECTORY: &str = "III_TRACE_STORE_DROP_CRASH_DIRECTORY";
        if let Some(directory) = std::env::var_os(CHILD_DIRECTORY)
            && std::env::var_os(CHILD_MARKER).is_some()
        {
            let directory = PathBuf::from(directory);
            let store = TraceDiskStore::open(&test_config(&directory)).unwrap();
            let hot = hot_cache(64, 1024 * MIB);
            store.attach_hot_storage(&hot);
            hot.add_spans(vec![
                span_with("wide", "w", wide_attributes(400, 64 * 1024)),
                test_span("kept", "s", "kept"),
            ]);
            store.flush().unwrap();
            assert_eq!(store.status().known_dropped_spans, 1);
            std::process::abort();
        }

        let directory = tempfile::tempdir().unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "workers::observability::trace_store::tests::dropped_span_count_survives_a_crash_after_flush",
                "--nocapture",
            ])
            .env(CHILD_MARKER, "1")
            .env(CHILD_DIRECTORY, directory.path())
            .status()
            .unwrap();
        assert!(!status.success(), "the child must crash after its flush");
        let store = TraceDiskStore::open(&test_config(directory.path())).unwrap();
        let restarted = store.status();
        eprintln!(
            "CRASH_EVIDENCE child_status={status:?} dropped_after_restart={} completeness={} kept={}",
            restarted.known_dropped_spans,
            restarted.completeness,
            store.get_spans_by_trace_id("kept").unwrap().len()
        );
        assert_eq!(restarted.known_dropped_spans, 1);
        assert_eq!(restarted.completeness, "partial");
        assert_eq!(store.get_spans_by_trace_id("kept").unwrap().len(), 1);
        store.shutdown();
    }

    fn realistic_trace(tag: u64, index: u64) -> String {
        format!("{tag:08x}{index:024x}")
    }

    fn realistic_span(index: u64) -> String {
        format!("{index:016x}")
    }

    fn tiny_attributes(count: usize) -> Vec<(String, String)> {
        (0..count)
            .map(|index| (format!("k{index:03}"), "v".to_string()))
            .collect()
    }

    fn count_trace_prefix(directory: &Path, prefix: &str) -> i64 {
        Connection::open(directory.join("traces.sqlite3"))
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM spans WHERE trace_id LIKE ?1",
                params![format!("{prefix}%")],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// Review B1: the scratch measurement is an optimization. An unusable
    /// TMPDIR disables it (one warning) and the large trace is measured by a
    /// trial in the archive; independent traces are written in the same
    /// flush. Control: a usable scratch measures the large trace once.
    #[test]
    fn unusable_scratch_directory_does_not_block_independent_traces() {
        for scratch in [Some(PathBuf::from("/nonexistent-iii-trace-scratch")), None] {
            let directory = tempfile::tempdir().unwrap();
            let store = TraceDiskStore::open(&capped_config(directory.path(), 64 * MIB)).unwrap();
            *store.scratch_dir_override.lock().unwrap() = scratch.clone();
            let hot = hot_cache(8192, 1024 * MIB);
            store.attach_hot_storage(&hot);
            let big = span_with("aaa-big", "big", wide_attributes(150, 64 * 1024));
            assert!(lower_bound_of(&big) > 64 * MIB / LARGE_GROUP_DIVISOR);
            hot.add_spans(vec![big]);
            hot.add_spans(
                (0..12)
                    .map(|index| test_span(&format!("zzz-n-{index:02}"), "s", "normal"))
                    .collect(),
            );
            let started = Instant::now();
            let flush = store.flush();
            let elapsed = started.elapsed();
            let normals = count_trace_prefix(directory.path(), "zzz-n-");
            let big_rows = count_trace_prefix(directory.path(), "aaa-big");
            let status = store.status();
            eprintln!(
                "B1_EVIDENCE scratch={scratch:?} flush={flush:?} flush_ms={} normals={normals} big_rows={big_rows} dirty={} archive={} dropped={} scratch_measurements={} warnings={}",
                elapsed.as_millis(),
                hot.dirty_len(),
                status.archive,
                status.known_dropped_spans,
                store.scratch_measurements.load(Ordering::Relaxed),
                store.warnings_emitted()
            );
            flush.expect("a failed measurement is not an archive error");
            assert_eq!(
                normals, 12,
                "independent traces are written in the same flush"
            );
            assert_eq!(
                big_rows, 1,
                "the large trace is measured by a trial instead"
            );
            assert_eq!(hot.dirty_len(), 0);
            assert_eq!(status.archive, "healthy");
            assert_eq!(store.scratch_measurements.load(Ordering::Relaxed), 1);
            if scratch.is_some() {
                assert_eq!(store.warnings_emitted(), 1);
                // Disabled for a while: the next large trace does not retry it.
                hot.add_spans(vec![span_with(
                    "aaa-big2",
                    "big",
                    wide_attributes(150, 64 * 1024),
                )]);
                store.flush().unwrap();
                assert_eq!(count_trace_prefix(directory.path(), "aaa-big2"), 1);
                assert_eq!(store.scratch_measurements.load(Ordering::Relaxed), 1);
                assert_eq!(store.warnings_emitted(), 1);
            }
            store.shutdown();
        }
    }

    /// Review B1 with real write errors: `ulimit -f` makes the scratch file
    /// and the archive fail for the large trace (EFBIG). Only that trace is
    /// deferred; the others, and later ones, are written.
    #[cfg(unix)]
    #[test]
    fn write_limits_on_scratch_and_archive_defer_only_the_failing_trace() {
        const CHILD_MARKER: &str = "III_TRACE_STORE_SCRATCH_LIMIT_CHILD";
        const CHILD_DIRECTORY: &str = "III_TRACE_STORE_SCRATCH_LIMIT_DIRECTORY";
        if let Some(directory) = std::env::var_os(CHILD_DIRECTORY)
            && std::env::var_os(CHILD_MARKER).is_some()
        {
            let _lift = LiftFileSizeLimitOnDrop;
            let directory = PathBuf::from(directory);
            let store = TraceDiskStore::open(&capped_config(&directory, 64 * MIB)).unwrap();
            let hot = hot_cache(8192, 1024 * MIB);
            store.attach_hot_storage(&hot);
            hot.add_spans(vec![span_with(
                "aaa-big",
                "big",
                wide_attributes(150, 64 * 1024),
            )]);
            hot.add_spans(
                (0..12)
                    .map(|index| test_span(&format!("zzz-n-{index:02}"), "s", "normal"))
                    .collect(),
            );
            let flush = store.flush();
            let normals = count_trace_prefix(&directory, "zzz-n-");
            let big_rows = count_trace_prefix(&directory, "aaa-big");
            let status = store.status();
            hot.add_spans(vec![test_span("zzz-after", "s", "after")]);
            let second = store.flush();
            let after = count_trace_prefix(&directory, "zzz-after");
            eprintln!(
                "B1_ULIMIT_EVIDENCE tmpdir={:?} flush={flush:?} normals={normals} big_rows={big_rows} dirty={} archive={} dropped={} scratch_measurements={} second_flush_err={} later_trace={after}",
                std::env::var_os("TMPDIR"),
                hot.dirty_len(),
                status.archive,
                status.known_dropped_spans,
                store.scratch_measurements.load(Ordering::Relaxed),
                second.is_err()
            );
            assert!(flush.is_err(), "the failing trace is reported");
            assert_eq!(normals, 12, "independent traces are written");
            assert_eq!(big_rows, 0);
            assert_eq!(hot.dirty_len(), 1, "only the failing trace stays dirty");
            assert_eq!(status.archive, "degraded");
            assert_eq!(status.known_dropped_spans, 0, "an I/O error is not a drop");
            assert_eq!(after, 1, "the archive keeps working for traces that fit");
            store.shutdown();
            return;
        }

        let directory = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let output = Command::new("bash")
            .args([
                "-c",
                "ulimit -S -f 20480; trap '' XFSZ; exec \"$1\" --exact \"$2\" --nocapture",
                "trace-store-scratch-limit-child",
            ])
            .arg(std::env::current_exe().unwrap())
            .arg("workers::observability::trace_store::tests::write_limits_on_scratch_and_archive_defer_only_the_failing_trace")
            .env(CHILD_MARKER, "1")
            .env(CHILD_DIRECTORY, directory.path())
            .env("TMPDIR", scratch.path())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines().filter(|line| line.contains("EVIDENCE")) {
            eprintln!("{line}");
        }
        assert!(
            output.status.success(),
            "write-limit child failed:\nstdout:\n{}\nstderr:\n{stderr}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(
            fs::read_dir(scratch.path()).unwrap().count(),
            0,
            "scratch removed"
        );
    }

    /// Review N3: a live trace whose real size is far above its payload bytes
    /// (many tiny attributes) is accounted by its measured pages, so a group
    /// that cannot fit beside it evicts nothing, now or on its retries, and
    /// is measured once.
    #[test]
    fn protected_trace_size_is_measured_so_history_is_not_evicted_in_vain() {
        let directory = tempfile::tempdir().unwrap();
        let max = 64 * MIB;
        let threshold = max + 8 * MIB;
        let store = TraceDiskStore::open(&capped_config(directory.path(), max)).unwrap();
        let hot = hot_cache(200_000, 4096 * MIB);
        store.attach_hot_storage(&hot);
        let live = realistic_trace(0xaaaa_0001, 1);
        for chunk in 0..3_u64 {
            hot.add_spans(
                (0..200_u64)
                    .map(|index| {
                        span_with(
                            &live,
                            &realistic_span(chunk * 200 + index + 10),
                            tiny_attributes(200),
                        )
                    })
                    .collect(),
            );
            store.flush().unwrap();
        }
        let mut root = span_with(&live, &realistic_span(1), Vec::new());
        root.pending = true;
        root.end_time_unix_nano = 0;
        hot.add_pending_span(root);
        let h1 = format!("{:08x}", 0xbbbb_0001_u64);
        hot.add_spans(
            (0..40)
                .map(|index| {
                    span_with(
                        &realistic_trace(0xbbbb_0001, index),
                        &realistic_span(1),
                        payload_attribute(64 * 1024),
                    )
                })
                .collect(),
        );
        store.flush().unwrap();
        // Size G so the previous estimate (payload bytes times the archive's
        // real/approx ratio) would have let it fit beside the live trace.
        let (live_approx, total_approx, live_image): (i64, i64, i64) =
            Connection::open(directory.path().join("traces.sqlite3"))
                .unwrap()
                .query_row(
                    "SELECT (SELECT approx_bytes FROM trace_meta WHERE trace_id = ?1),
                            (SELECT SUM(approx_bytes) FROM trace_meta),
                            (SELECT image_bytes FROM trace_meta WHERE trace_id = ?1)",
                    params![live],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
        let current = directory_size(directory.path()).unwrap();
        let ratio_estimate = (live_approx as f64 * current as f64 / total_approx as f64) as u64;
        let g_attributes = (threshold - ratio_estimate - 2 * MIB) / (3 * 65_536 + 20);
        let g_trace = realistic_trace(0xcccc_0001, 1);
        hot.add_spans(vec![span_with(
            &g_trace,
            &realistic_span(1),
            wide_attributes(g_attributes as usize, 65_536),
        )]);
        let first = store.flush();
        let h1_after = count_trace_prefix(directory.path(), &h1);
        let h2 = format!("{:08x}", 0xdddd_0001_u64);
        hot.add_spans(
            (0..40)
                .map(|index| {
                    span_with(
                        &realistic_trace(0xdddd_0001, index),
                        &realistic_span(1),
                        payload_attribute(64 * 1024),
                    )
                })
                .collect(),
        );
        let waited = Instant::now();
        while count_trace_prefix(directory.path(), &h2) < 40
            && waited.elapsed() < Duration::from_secs(3)
        {
            thread::sleep(Duration::from_millis(20));
        }
        let h2_written = count_trace_prefix(directory.path(), &h2);
        let mut h2_min = h2_written;
        let watched = Instant::now();
        while watched.elapsed() < Duration::from_secs(4) {
            h2_min = h2_min.min(count_trace_prefix(directory.path(), &h2));
            thread::sleep(Duration::from_millis(50));
        }
        let status = store.status();
        eprintln!(
            "N3_EVIDENCE live_approx={live_approx} live_image={live_image} ratio_estimate={ratio_estimate} g_attributes={g_attributes} first={first:?} h1_after={h1_after} h2_written={h2_written} h2_min_4s={h2_min} g_rows={} dirty={} scratch_measurements={} archive={}",
            count_trace_prefix(directory.path(), &g_trace),
            hot.dirty_len(),
            store.scratch_measurements.load(Ordering::Relaxed),
            status.archive
        );
        assert!(
            live_image as u64 > ratio_estimate,
            "measured above the old estimate"
        );
        assert!(first.is_err_and(|error| error.contains("deferred")));
        assert_eq!(
            h1_after, 40,
            "history not evicted for a group that cannot fit"
        );
        assert_eq!(h2_written, 40);
        assert_eq!(h2_min, 40, "history not evicted on the backoff retries");
        assert_eq!(count_trace_prefix(directory.path(), &g_trace), 0);
        assert_eq!(hot.dirty_len(), 1, "G waits for the live trace");
        assert_eq!(
            store.scratch_measurements.load(Ordering::Relaxed),
            1,
            "measured once"
        );
        store.shutdown();
    }

    /// Review N4: spans whose trace's own durable rows leave no room for
    /// them are dropped and counted (only those), instead of staying dirty
    /// and the archive degraded; the durable rows are kept.
    #[test]
    fn spans_that_never_fit_beside_their_own_durable_rows_are_dropped_and_counted() {
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&capped_config(directory.path(), 16 * MIB)).unwrap();
        let hot = hot_cache(200_000, 4096 * MIB);
        store.attach_hot_storage(&hot);
        let trace = realistic_trace(0xeeee_0001, 1);
        hot.add_spans(
            (0..1500_u64)
                .map(|index| span_with(&trace, &realistic_span(index + 1), tiny_attributes(100)))
                .collect(),
        );
        let flush = flush_settled(&store, &hot);
        let after_flush = count_trace_prefix(directory.path(), &trace);
        thread::sleep(Duration::from_secs(3));
        let archived = count_trace_prefix(directory.path(), &trace) as u64;
        let status = store.status();
        eprintln!(
            "N4_EVIDENCE flush={flush:?} archived_after_flush={after_flush} archived={archived} dropped={} dirty={} archive={} physical={}",
            status.known_dropped_spans,
            hot.dirty_len(),
            status.archive,
            status.physical_bytes
        );
        assert_eq!(hot.dirty_len(), 0, "never left dirty indefinitely");
        assert_eq!(
            archived + status.known_dropped_spans,
            1500,
            "archived or counted"
        );
        assert!(archived >= 1024, "durable rows kept, archive filled");
        assert!(status.known_dropped_spans > 0);
        assert_eq!(status.archive, "healthy");
        assert!(status.physical_bytes <= 16 * MIB + 8 * MIB);
        store.shutdown();
    }

    /// `image_bytes` is the trace's share of the pages its commits used: a
    /// trace written alone gets about its isolated image, the sum stays within
    /// the file's used pages, and a smaller version gives pages back.
    #[test]
    fn image_bytes_accounts_for_the_pages_each_commit_uses() {
        let directory = tempfile::tempdir().unwrap();
        let store = TraceDiskStore::open(&test_config(directory.path())).unwrap();
        let hot = hot_cache(64, 64 * MIB);
        hot.set_max_attribute_bytes(0);
        store.attach_hot_storage(&hot);
        let spans: Vec<_> = [8 * 1024, 64 * 1024, 256 * 1024]
            .iter()
            .enumerate()
            .map(|(index, size)| span_with(&format!("acct-{index}"), "s", payload_attribute(*size)))
            .collect();
        for span in &spans {
            hot.add_spans(vec![span.clone()]);
            store.flush().unwrap();
        }
        let read = || -> (Vec<i64>, u64) {
            let connection = Connection::open(directory.path().join("traces.sqlite3")).unwrap();
            let images = (0..3)
                .map(|index| {
                    connection
                        .query_row(
                            "SELECT image_bytes FROM trace_meta WHERE trace_id = ?1",
                            params![format!("acct-{index}")],
                            |row| row.get(0),
                        )
                        .unwrap()
                })
                .collect();
            let stats = page_stats(&connection).unwrap();
            (images, (stats.pages - stats.free) * stats.page_size)
        };
        let (images, used) = read();
        let payload = serde_json::to_string(&spans[2]).unwrap();
        let isolated = isolated_image_bytes(&[(&spans[2], payload.as_str())]).unwrap();
        eprintln!("IMAGE_EVIDENCE images={images:?} used={used} isolated_largest={isolated}");
        // Page deltas: the local part of a row can land in a leaf page an
        // earlier commit already allocated, so allow four pages of slack.
        for (image, span) in images.iter().zip(&spans) {
            assert!(
                *image as u64 + 4 * 4096 >= lower_bound_of(span),
                "{image} far below the lower bound"
            );
        }
        assert!(images.iter().sum::<i64>() as u64 <= used);
        assert!(images[2].abs_diff(isolated as i64) <= 16 * 4096);
        hot.add_spans(vec![span_with("acct-2", "s", payload_attribute(1024))]);
        store.flush().unwrap();
        let (shrunk, _) = read();
        assert!(
            shrunk[2] < images[2] / 4,
            "replaced pages leave the account: {shrunk:?}"
        );
        store.shutdown();
    }

    /// An archive created before `image_bytes` gains the column on open; its
    /// traces count by their lower bound until written again.
    #[test]
    fn archive_without_image_bytes_is_migrated_on_open() {
        let directory = tempfile::tempdir().unwrap();
        {
            let store = TraceDiskStore::open(&test_config(directory.path())).unwrap();
            let hot = hot_cache(16, 16 * MIB);
            store.attach_hot_storage(&hot);
            hot.add_spans(vec![test_span("legacy", "s", "legacy")]);
            store.flush().unwrap();
            store.shutdown();
        }
        let has_column = |connection: &Connection| {
            connection
                .prepare("SELECT 1 FROM pragma_table_info('trace_meta') WHERE name = 'image_bytes'")
                .unwrap()
                .exists([])
                .unwrap()
        };
        {
            let connection = Connection::open(directory.path().join("traces.sqlite3")).unwrap();
            connection
                .execute_batch("ALTER TABLE trace_meta DROP COLUMN image_bytes;")
                .unwrap();
            assert!(!has_column(&connection), "fixture: pre-column archive");
        }
        let store = TraceDiskStore::open(&test_config(directory.path())).unwrap();
        let hot = hot_cache(16, 16 * MIB);
        store.attach_hot_storage(&hot);
        hot.add_spans(vec![test_span("fresh", "s", "fresh")]);
        store.flush().unwrap();
        let connection = Connection::open(directory.path().join("traces.sqlite3")).unwrap();
        assert!(has_column(&connection));
        let image = |trace: &str| -> i64 {
            connection
                .query_row(
                    "SELECT image_bytes FROM trace_meta WHERE trace_id = ?1",
                    params![trace],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(image("legacy"), 0, "not measured yet");
        assert!(image("fresh") >= 0);
        assert_eq!(store.get_spans_by_trace_id("legacy").unwrap().len(), 1);
        store.shutdown();
    }

    /// Review B1 with the real environment: `TMPDIR` points to a missing
    /// directory for the whole process. The scratch is skipped (one warning),
    /// the large trace is measured by a trial, and every trace is written.
    #[cfg(unix)]
    #[test]
    fn missing_tmpdir_does_not_block_independent_traces() {
        const CHILD_MARKER: &str = "III_TRACE_STORE_MISSING_TMPDIR_CHILD";
        const CHILD_DIRECTORY: &str = "III_TRACE_STORE_MISSING_TMPDIR_DIRECTORY";
        if let Some(directory) = std::env::var_os(CHILD_DIRECTORY)
            && std::env::var_os(CHILD_MARKER).is_some()
        {
            let directory = PathBuf::from(directory);
            let store = TraceDiskStore::open(&capped_config(&directory, 64 * MIB)).unwrap();
            let hot = hot_cache(8192, 1024 * MIB);
            store.attach_hot_storage(&hot);
            hot.add_spans(vec![span_with(
                "aaa-big",
                "big",
                wide_attributes(150, 64 * 1024),
            )]);
            hot.add_spans(
                (0..12)
                    .map(|index| test_span(&format!("zzz-n-{index:02}"), "s", "normal"))
                    .collect(),
            );
            let started = Instant::now();
            let flush = store.flush();
            let status = store.status();
            eprintln!(
                "B1_TMPDIR_EVIDENCE tmpdir={:?} flush={flush:?} flush_ms={} normals={} big_rows={} dirty={} archive={} warnings={} scratch_measurements={}",
                std::env::var_os("TMPDIR"),
                started.elapsed().as_millis(),
                count_trace_prefix(&directory, "zzz-n-"),
                count_trace_prefix(&directory, "aaa-big"),
                hot.dirty_len(),
                status.archive,
                store.warnings_emitted(),
                store.scratch_measurements.load(Ordering::Relaxed)
            );
            flush.expect("a missing TMPDIR is not an archive error");
            assert_eq!(count_trace_prefix(&directory, "zzz-n-"), 12);
            assert_eq!(count_trace_prefix(&directory, "aaa-big"), 1);
            assert_eq!(hot.dirty_len(), 0);
            assert_eq!(status.archive, "healthy");
            assert_eq!(store.warnings_emitted(), 1);
            store.shutdown();
            return;
        }

        let directory = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "workers::observability::trace_store::tests::missing_tmpdir_does_not_block_independent_traces",
                "--nocapture",
            ])
            .env(CHILD_MARKER, "1")
            .env(CHILD_DIRECTORY, directory.path())
            .env("TMPDIR", "/nonexistent-iii-trace-tmpdir")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines().filter(|line| line.contains("EVIDENCE")) {
            eprintln!("{line}");
        }
        assert!(
            output.status.success(),
            "missing-TMPDIR child failed:\nstdout:\n{}\nstderr:\n{stderr}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    fn hex_trace(tag: u64, index: u64) -> String {
        format!("{tag:08x}{index:024x}")
    }

    fn hex_span(index: u64) -> String {
        format!("{index:016x}")
    }

    fn wait_for_rows(directory: &Path, prefix: &str, rows: i64, limit: Duration) -> i64 {
        let deadline = Instant::now() + limit;
        loop {
            let found = count_trace_prefix(directory, prefix);
            if found >= rows || Instant::now() >= deadline {
                return found;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// `flush` is bounded by wall-clock (the writer's budget, the caller's
    /// timeout), so on a loaded machine one call can end before the drain
    /// does. Call it again only while a call used up that budget and the
    /// dirty set shrank; any other answer is returned as is.
    fn flush_settled(store: &TraceDiskStore, hot: &InMemorySpanStorage) -> Result<(), String> {
        loop {
            let dirty = hot.dirty_len();
            let started = Instant::now();
            let result = store.flush();
            if result.is_ok() || started.elapsed() < FLUSH_TIME_BUDGET || hot.dirty_len() >= dirty {
                return result;
            }
        }
    }

    /// A live trace (600 spans of 200 tiny attributes) committed in the same
    /// batches as 60 payload traces (prefix `bbbb0003`), so its size is
    /// attributed from shared commits. Returns the live trace id.
    fn shared_commit_live_trace(store: &TraceDiskStore, hot: &Arc<InMemorySpanStorage>) -> String {
        let live = hex_trace(0xaaaa_0003, 1);
        for round in 0..6_u64 {
            let mut spans = Vec::new();
            for batch in 0..10_u64 {
                for index in 0..10_u64 {
                    let span_id = hex_span(round * 100 + batch * 10 + index + 10);
                    spans.push(span_with(&live, &span_id, tiny_attributes(200)));
                }
                spans.push(span_with(
                    &hex_trace(0xbbbb_0003, round * 10 + batch),
                    &hex_span(1),
                    payload_attribute(64 * 1024),
                ));
            }
            hot.add_spans(spans);
            store.flush().unwrap();
        }
        let mut snapshot = span_with(&live, &hex_span(1), Vec::new());
        snapshot.pending = true;
        snapshot.end_time_unix_nano = 0;
        hot.add_pending_span(snapshot);
        live
    }

    /// Review N3c: the measured floor survives a retention pass that deletes
    /// none of its traces; deleting rows of one of them, new limits or a
    /// clear (`reset`) drop it.
    #[test]
    fn held_floor_survives_retention_unless_its_traces_lose_rows() {
        let floor = || {
            Some(HeldFloor {
                traces: HashSet::from(["live".to_string()]),
                bytes: 42,
                attributed: 0,
            })
        };
        let mut writer = WriterState {
            held_floor: floor(),
            ..WriterState::default()
        };
        writer.deferred.insert(
            "group".to_string(),
            Deferral {
                until: Instant::now() + Duration::from_secs(60),
                delay: Duration::from_secs(1),
                measured: None,
            },
        );
        writer.retry_deferred_now();
        assert!(writer.held_floor.is_some(), "retention keeps the floor");
        assert!(
            writer.deferred["group"].until <= Instant::now(),
            "retried now"
        );
        writer.forget_floor_of(["other"]);
        assert!(writer.held_floor.is_some(), "unrelated rows deleted");
        writer.forget_floor_of(["live"]);
        assert!(writer.held_floor.is_none(), "rows of a floor trace deleted");
        writer.held_floor = floor();
        writer.reset();
        assert!(writer.held_floor.is_none() && writer.deferred.is_empty());
    }

    /// Review N3c: the size of a live trace is unknown (`image_bytes = 0`, as
    /// written by an older version), so a group that cannot fit beside it
    /// evicts the unprotected history once and measures the floor. Later
    /// history is spared by the floor on the backoff retry AND after a
    /// retention pass; the group waits (it fits an empty archive).
    #[test]
    fn retention_keeps_the_floor_of_a_live_trace_of_unknown_size() {
        let directory = tempfile::tempdir().unwrap();
        let store =
            TraceDiskStore::open(&capped_config(directory.path(), 64 * 1024 * 1024)).unwrap();
        let hot = hot_cache(200_000, 4096 * 1024 * 1024);
        store.attach_hot_storage(&hot);
        let live = shared_commit_live_trace(&store, &hot);
        Connection::open(directory.path().join("traces.sqlite3"))
            .unwrap()
            .execute(
                "UPDATE trace_meta SET image_bytes = 0 WHERE trace_id = ?1",
                params![live],
            )
            .unwrap();
        let group = hex_trace(0xcccc_0004, 1);
        hot.add_spans(vec![span_with(
            &group,
            &hex_span(1),
            wide_attributes(280, 64 * 1024),
        )]);
        let first = store.flush();
        let first_history = count_trace_prefix(directory.path(), "bbbb0003");
        hot.add_spans(
            (0..40_u64)
                .map(|index| {
                    span_with(
                        &hex_trace(0xdddd_0004, index),
                        &hex_span(1),
                        payload_attribute(64 * 1024),
                    )
                })
                .collect(),
        );
        let written = wait_for_rows(directory.path(), "dddd0004", 40, Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(2_500));
        let after_retry = count_trace_prefix(directory.path(), "dddd0004");
        let retained = store.retain();
        std::thread::sleep(Duration::from_millis(1_500));
        let after_retain = count_trace_prefix(directory.path(), "dddd0004");
        let status = store.status();
        eprintln!(
            "EVIDENCE N3C first_flush={first:?} first_history_left={first_history} written={written} after_backoff_retry={after_retry} retain={retained:?} after_retain={after_retain} group_rows={} dropped={}",
            count_trace_prefix(directory.path(), "cccc0004"),
            status.known_dropped_spans
        );
        store.shutdown();
        assert_eq!(written, 40);
        assert_eq!(after_retry, 40, "the floor spares later history");
        assert!(retained.is_ok());
        assert_eq!(after_retain, 40, "retention keeps the measured floor");
        assert_eq!(status.known_dropped_spans, 0, "the group waits");
    }

    /// Review N11: a group that fits an empty archive but not beside a live
    /// trace sized from shared commits waits: no history is evicted for it
    /// and nothing is dropped.
    #[test]
    fn shared_commit_attribution_does_not_evict_history_in_vain() {
        let directory = tempfile::tempdir().unwrap();
        let store =
            TraceDiskStore::open(&capped_config(directory.path(), 64 * 1024 * 1024)).unwrap();
        let hot = hot_cache(200_000, 4096 * 1024 * 1024);
        store.attach_hot_storage(&hot);
        let live = shared_commit_live_trace(&store, &hot);
        let image_bytes: i64 = Connection::open(directory.path().join("traces.sqlite3"))
            .unwrap()
            .query_row(
                "SELECT image_bytes FROM trace_meta WHERE trace_id = ?1",
                params![live],
                |row| row.get(0),
            )
            .unwrap();
        let history = count_trace_prefix(directory.path(), "bbbb0003");
        hot.add_spans(vec![span_with(
            &hex_trace(0xcccc_0003, 1),
            &hex_span(1),
            wide_attributes(300, 64 * 1024),
        )]);
        let flushed = store.flush();
        let status = store.status();
        let history_after = count_trace_prefix(directory.path(), "bbbb0003");
        eprintln!(
            "EVIDENCE N11 live_image_bytes={image_bytes} flush={flushed:?} history={history}->{history_after} dropped={}",
            status.known_dropped_spans
        );
        store.shutdown();
        assert_eq!(history, 60);
        assert_eq!(history_after, history, "no history evicted in vain");
        assert_eq!(status.known_dropped_spans, 0, "the group waits");
    }

    /// A live trace whose rows are far larger than their lower bound (many
    /// tiny attributes) fills the archive, and its large spans can never fit
    /// beside those rows. They are dropped (counted) on the measured floor
    /// without evicting the history written between them.
    #[test]
    fn spans_that_never_fit_beside_their_live_trace_do_not_evict_history() {
        let directory = tempfile::tempdir().unwrap();
        let store =
            TraceDiskStore::open(&capped_config(directory.path(), 16 * 1024 * 1024)).unwrap();
        let hot = hot_cache(200_000, 4096 * 1024 * 1024);
        store.attach_hot_storage(&hot);
        let live = hex_trace(0xabcd_0001, 1);
        let mut root = span_with(&live, &hex_span(1), Vec::new());
        root.pending = true;
        root.end_time_unix_nano = 0;
        hot.add_pending_span(root);
        hot.add_spans(
            (0..900_u64)
                .map(|index| span_with(&live, &hex_span(index + 10), tiny_attributes(100)))
                .collect(),
        );
        flush_settled(&store, &hot).unwrap();
        let mut evidence = Vec::new();
        for (round, tag) in [0xbcde_0001_u64, 0xbcde_0002].into_iter().enumerate() {
            hot.add_spans(
                (0..10_u64)
                    .map(|index| {
                        span_with(
                            &hex_trace(tag, index),
                            &hex_span(1),
                            payload_attribute(64 * 1024),
                        )
                    })
                    .collect(),
            );
            flush_settled(&store, &hot).unwrap();
            let prefix = format!("{tag:08x}");
            let written = count_trace_prefix(directory.path(), &prefix);
            hot.add_spans(vec![span_with(
                &live,
                &hex_span(5_000 + round as u64),
                wide_attributes(33, 64 * 1024),
            )]);
            let flushed = flush_settled(&store, &hot);
            let left = count_trace_prefix(directory.path(), &prefix);
            evidence.push(format!(
                "round{round} history {written}->{left} flush={flushed:?}"
            ));
            assert_eq!(written, 10);
            assert_eq!(left, written, "history evicted for a span that never fits");
        }
        let status = store.status();
        eprintln!(
            "EVIDENCE NEVER_FIT {} live_rows={} dropped={} dirty={}",
            evidence.join(" | "),
            count_trace_prefix(directory.path(), "abcd0001"),
            status.known_dropped_spans,
            hot.dirty_len()
        );
        store.shutdown();
        assert_eq!(status.known_dropped_spans, 2, "both spans dropped, counted");
        assert_eq!(hot.dirty_len(), 0);
    }

    /// Review N13: in an archive written by an older version (`image_bytes
    /// = 0`) the sizes of other traces are unknown, so a group that fits once
    /// unprotected history is evicted is written, not dropped. Control: the
    /// same with measured sizes.
    #[test]
    fn unknown_sizes_of_other_traces_never_drop_a_group_that_fits() {
        for unknown in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let store =
                TraceDiskStore::open(&capped_config(directory.path(), 64 * 1024 * 1024)).unwrap();
            let hot = hot_cache(200_000, 4096 * 1024 * 1024);
            store.attach_hot_storage(&hot);
            for chunk in 0..5_u64 {
                hot.add_spans(
                    (0..50_u64)
                        .map(|index| {
                            span_with(
                                &hex_trace(0xeeee_0003, chunk * 50 + index),
                                &hex_span(1),
                                payload_attribute(64 * 1024),
                            )
                        })
                        .collect(),
                );
                store.flush().unwrap();
            }
            if unknown {
                Connection::open(directory.path().join("traces.sqlite3"))
                    .unwrap()
                    .execute("UPDATE trace_meta SET image_bytes = 0", [])
                    .unwrap();
            }
            hot.add_spans(vec![span_with(
                &hex_trace(0xffff_0003, 1),
                &hex_span(1),
                wide_attributes(230, 64 * 1024),
            )]);
            let flushed = store.flush();
            let group_rows =
                wait_for_rows(directory.path(), "ffff0003", 1, Duration::from_secs(10));
            let status = store.status();
            let legacy = count_trace_prefix(directory.path(), "eeee0003");
            eprintln!(
                "EVIDENCE N13 unknown_sizes={unknown} flush={flushed:?} legacy=250->{legacy} group_rows={group_rows} dropped={} dirty={}",
                status.known_dropped_spans,
                hot.dirty_len()
            );
            store.shutdown();
            assert_eq!(status.known_dropped_spans, 0, "unknown_sizes={unknown}");
            assert_eq!(group_rows, 1, "unknown_sizes={unknown}");
            assert!(legacy < 250, "unprotected history made room");
        }
    }
}
