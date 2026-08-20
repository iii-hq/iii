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
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    thread,
    time::Duration,
};

use super::otel::StoredSpan;

const BATCH_MAX_SPANS: usize = 512;
const BATCH_MAX_BYTES: u64 = 4 * 1024 * 1024;
const BATCH_INTERVAL: Duration = Duration::from_millis(250);
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(500),
    Duration::from_secs(2),
];
const CAP_HIGH_WATERMARK: f64 = 0.90;
const CAP_LOW_WATERMARK: f64 = 0.80;
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
    RecordDropped {
        count: u64,
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
    known_dropped_spans: u64,
    unclean_shutdown: bool,
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
                unclean_shutdown,
            }),
            stop: AtomicBool::new(false),
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

    /// Synchronously drain the current hot batch. This is used by tests and
    /// lifecycle code that needs an explicit durability boundary without
    /// waiting for the periodic writer tick.
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
        self.notify();
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

    pub(crate) fn record_dropped_spans(&self, count: u64) {
        if count == 0 {
            return;
        }
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.known_dropped_spans = state.known_dropped_spans.saturating_add(count);
        }
        let _ = self
            .control_tx
            .send(ControlMessage::RecordDropped { count });
        self.notify();
    }

    pub(crate) fn is_degraded(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .degraded
    }
}

fn writer_loop(
    weak: Weak<TraceDiskStore>,
    mut connection: Connection,
    wake_rx: mpsc::Receiver<()>,
    control_rx: mpsc::Receiver<ControlMessage>,
) {
    loop {
        let Some(store) = weak.upgrade() else { break };
        while let Ok(control) = control_rx.try_recv() {
            if handle_control(&store, &mut connection, control) {
                return;
            }
        }

        match wake_rx.recv_timeout(BATCH_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {
                persist_dirty_batch(&store, &mut connection);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn handle_control(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    control: ControlMessage,
) -> bool {
    match control {
        ControlMessage::Clear { response } => {
            let result = clear_database(store, connection);
            let _ = response.send(result);
            false
        }
        ControlMessage::Flush { response } => {
            let result = drain_dirty(store, connection);
            let _ = response.send(result);
            false
        }
        ControlMessage::RecordDropped { count } => {
            let result = connection.execute(
                "INSERT INTO storage_meta(key, value) VALUES ('known_dropped_spans', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = CAST(value AS INTEGER) + ?1",
                params![count as i64],
            );
            if let Err(error) = result {
                store.record_error(format!("cannot persist dropped trace count: {error}"));
            }
            false
        }
        ControlMessage::Retain { response } => {
            let result = retain_database(store, connection);
            let _ = response.send(result);
            false
        }
        ControlMessage::Shutdown { response } => {
            let result = drain_dirty(store, connection).and_then(|()| {
                set_clean_shutdown(connection, true)
                    .map(|_| ())
                    .map_err(|err| err.to_string())
            });
            let _ = response.send(result);
            true
        }
    }
}

fn persist_dirty_batch(store: &Arc<TraceDiskStore>, connection: &mut Connection) {
    let Some(hot) = store
        .hot
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .and_then(Weak::upgrade)
    else {
        return;
    };

    let batch = hot.dirty_spans(BATCH_MAX_SPANS, BATCH_MAX_BYTES);
    if batch.is_empty() {
        return;
    }

    let keys: Vec<(String, String)> = batch
        .iter()
        .map(|span| (span.trace_id.clone(), span.span_id.clone()))
        .collect();
    for attempt in 0..=RETRY_DELAYS.len() {
        match write_batch(store, connection, &batch) {
            Ok(()) => {
                hot.mark_durable(&keys);
                store.record_success();
                return;
            }
            Err(error) if attempt < RETRY_DELAYS.len() => {
                thread::sleep(RETRY_DELAYS[attempt]);
                store.record_error(error);
            }
            Err(error) => {
                store.record_error(error);
                return;
            }
        }
    }
}

fn drain_dirty(store: &Arc<TraceDiskStore>, connection: &mut Connection) -> Result<(), String> {
    for _ in 0..32 {
        let Some(hot) = store
            .hot
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .and_then(Weak::upgrade)
        else {
            return Ok(());
        };
        let batch = hot.dirty_spans(BATCH_MAX_SPANS, BATCH_MAX_BYTES);
        if batch.is_empty() {
            return Ok(());
        }
        let keys: Vec<(String, String)> = batch
            .iter()
            .map(|span| (span.trace_id.clone(), span.span_id.clone()))
            .collect();
        write_batch(store, connection, &batch)?;
        hot.mark_durable(&keys);
        store.record_success();
    }
    Err("shutdown drain reached the bounded batch limit".to_string())
}

fn write_batch(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    batch: &[StoredSpan],
) -> Result<(), String> {
    let incoming_bytes: u64 = batch.iter().map(approx_span_bytes).sum();
    ensure_capacity(store, connection, incoming_bytes, batch)?;
    let epoch = store.epoch.load(Ordering::Acquire) as i64;
    let tx = connection
        .transaction()
        .map_err(|err| database_error(&store.database_path, "write transaction start", err))?;

    for span in batch {
        if span.pending {
            continue;
        }
        let payload = serde_json::to_string(span)
            .map_err(|err| format!("cannot serialize trace {}: {err}", span.trace_id))?;
        let approx_bytes = approx_span_bytes(span) as i64;
        tx.execute(
            "INSERT INTO spans (
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
                approx_bytes = excluded.approx_bytes",
            params![
                epoch,
                span.trace_id,
                span.span_id,
                span.parent_span_id,
                span.name,
                span.start_time_unix_nano as i64,
                span.end_time_unix_nano as i64,
                span.status,
                span.status_description,
                span.service_name,
                payload,
                super::otel::now_unix_nanos() as i64,
                approx_bytes,
            ],
        )
        .map_err(|err| {
            database_error(
                &store.database_path,
                &format!("write trace '{}'", span.trace_id),
                err,
            )
        })?;

        tx.execute(
            "DELETE FROM span_attributes WHERE epoch = ?1 AND trace_id = ?2 AND span_id = ?3",
            params![epoch, span.trace_id, span.span_id],
        )
        .map_err(|err| {
            database_error(
                &store.database_path,
                &format!("replace attributes for trace '{}'", span.trace_id),
                err,
            )
        })?;
        for (ordinal, (key, value)) in span.attributes.iter().enumerate() {
            tx.execute(
                "INSERT INTO span_attributes (epoch, trace_id, span_id, ordinal, key, value)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    epoch,
                    span.trace_id,
                    span.span_id,
                    ordinal as i64,
                    key,
                    value
                ],
            )
            .map_err(|err| {
                database_error(
                    &store.database_path,
                    &format!("write attribute for trace '{}'", span.trace_id),
                    err,
                )
            })?;
        }

        tx.execute(
            "INSERT INTO trace_meta (epoch, trace_id, first_start_ns, last_end_ns, last_ingest_ns, span_count, approx_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
             ON CONFLICT(epoch, trace_id) DO UPDATE SET
                first_start_ns = MIN(trace_meta.first_start_ns, excluded.first_start_ns),
                last_end_ns = MAX(trace_meta.last_end_ns, excluded.last_end_ns),
                last_ingest_ns = excluded.last_ingest_ns,
                span_count = (SELECT COUNT(*) FROM spans WHERE epoch = excluded.epoch AND trace_id = excluded.trace_id),
                approx_bytes = (SELECT COALESCE(SUM(approx_bytes), 0) FROM spans WHERE epoch = excluded.epoch AND trace_id = excluded.trace_id)",
            params![
                epoch,
                span.trace_id,
                span.start_time_unix_nano as i64,
                span.end_time_unix_nano as i64,
                super::otel::now_unix_nanos() as i64,
                approx_bytes,
            ],
        )
        .map_err(|err| {
            database_error(
                &store.database_path,
                &format!("update metadata for trace '{}'", span.trace_id),
                err,
            )
        })?;
    }

    tx.commit()
        .map_err(|err| database_error(&store.database_path, "write transaction commit", err))?;
    let _ = connection.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
    Ok(())
}

fn ensure_capacity(
    store: &Arc<TraceDiskStore>,
    connection: &mut Connection,
    incoming_bytes: u64,
    batch: &[StoredSpan],
) -> Result<(), String> {
    let max_bytes = store.max_disk_bytes.load(Ordering::Acquire);
    let tolerance = (max_bytes / 100).max(8 * 1024 * 1024);
    let current = directory_size(&store.directory).map_err(|err| {
        format!(
            "cannot measure trace storage directory '{}': {err}",
            store.directory.display()
        )
    })?;
    if current.saturating_add(incoming_bytes) <= ((max_bytes as f64) * CAP_HIGH_WATERMARK) as u64 {
        return Ok(());
    }

    let mut protected: HashSet<String> = batch.iter().map(|span| span.trace_id.clone()).collect();
    if let Some(hot) = store
        .hot
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .and_then(Weak::upgrade)
    {
        protected.extend(hot.pending_trace_ids());
    }
    let target = ((max_bytes as f64) * CAP_LOW_WATERMARK) as u64;
    let epoch = store.epoch.load(Ordering::Acquire) as i64;
    let mut candidate_statement = connection
        .prepare(
            "SELECT trace_id, approx_bytes FROM trace_meta
             WHERE epoch = ?1 ORDER BY last_ingest_ns ASC, trace_id ASC",
        )
        .map_err(|err| database_error(&store.database_path, "eviction preparation", err))?;
    let mut candidates = candidate_statement
        .query_map(params![epoch], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| database_error(&store.database_path, "eviction candidate query", err))?;
    let mut deleted = Vec::new();
    let mut remaining = current;
    while let Some((candidate, candidate_bytes)) = candidates
        .next()
        .transpose()
        .map_err(|err| database_error(&store.database_path, "eviction candidate read", err))?
    {
        if protected.contains(&candidate) {
            continue;
        }
        remaining = remaining.saturating_sub(candidate_bytes.max(0) as u64);
        deleted.push(candidate);
        if remaining <= target {
            break;
        }
    }
    drop(candidates);
    drop(candidate_statement);

    if !deleted.is_empty() {
        let tx = connection.transaction().map_err(|err| {
            database_error(&store.database_path, "eviction transaction start", err)
        })?;
        for trace_id in &deleted {
            tx.execute(
                "DELETE FROM span_attributes WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| {
                database_error(&store.database_path, "eviction attribute delete", err)
            })?;
            tx.execute(
                "DELETE FROM spans WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| database_error(&store.database_path, "eviction span delete", err))?;
            tx.execute(
                "DELETE FROM trace_meta WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| database_error(&store.database_path, "eviction metadata delete", err))?;
        }
        tx.commit().map_err(|err| {
            database_error(&store.database_path, "eviction transaction commit", err)
        })?;
        let _ =
            connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA incremental_vacuum;");
    }

    let measured = directory_size(&store.directory).unwrap_or(current);
    if measured.saturating_add(incoming_bytes) > max_bytes.saturating_add(tolerance) {
        return Err(format!(
            "trace storage limit reached: {} bytes used, limit {} bytes, tolerance {} bytes",
            measured, max_bytes, tolerance
        ));
    }
    Ok(())
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
    tx.commit()
        .map_err(|err| database_error(&store.database_path, "clear transaction commit", err))?;
    {
        let mut state = store
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.known_dropped_spans = 0;
    }
    connection
        .execute(
            "INSERT INTO storage_meta(key, value) VALUES ('known_dropped_spans', '0')
             ON CONFLICT(key) DO UPDATE SET value = '0'",
            [],
        )
        .map_err(|err| database_error(&store.database_path, "clear dropped-count update", err))?;
    let _ = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA incremental_vacuum;");
    store.record_success();
    Ok(())
}

fn retain_database(store: &Arc<TraceDiskStore>, connection: &mut Connection) -> Result<(), String> {
    let retention_seconds = store.retention_seconds.load(Ordering::Acquire);
    if retention_seconds == 0 {
        return ensure_capacity(store, connection, 0, &[]);
    }
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
    for trace_id in traces {
        connection
            .execute(
                "DELETE FROM span_attributes WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| {
                database_error(&store.database_path, "retention attribute delete", err)
            })?;
        connection
            .execute(
                "DELETE FROM spans WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| database_error(&store.database_path, "retention span delete", err))?;
        connection
            .execute(
                "DELETE FROM trace_meta WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| {
                database_error(&store.database_path, "retention metadata delete", err)
            })?;
    }
    let _ = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA incremental_vacuum;");
    let result = ensure_capacity(store, connection, 0, &[]);
    if result.is_ok() {
        store.record_success();
    }
    result
}

fn configure_connection(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;
             PRAGMA auto_vacuum=INCREMENTAL;
             PRAGMA busy_timeout=1000;
             CREATE TABLE IF NOT EXISTS storage_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
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
                PRIMARY KEY (epoch, trace_id)
             );
             CREATE INDEX IF NOT EXISTS trace_meta_retention_idx
                ON trace_meta(epoch, last_ingest_ns, trace_id);
             INSERT INTO storage_meta(key, value) VALUES ('epoch', '0')
                ON CONFLICT(key) DO NOTHING;
             INSERT INTO storage_meta(key, value) VALUES ('clean_shutdown', '1')
                ON CONFLICT(key) DO NOTHING;
             INSERT INTO storage_meta(key, value) VALUES ('known_dropped_spans', '0')
                ON CONFLICT(key) DO NOTHING;",
        )
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

fn approx_span_bytes(span: &StoredSpan) -> u64 {
    serde_json::to_vec(span)
        .map(|payload| payload.len() as u64)
        .unwrap_or(0)
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
        }
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
    fn disk_cap_rejects_a_protected_trace_without_partial_writes() {
        let directory = tempfile::tempdir().expect("temp trace directory");
        let mut config = test_config(directory.path());
        // The capacity guard has an 8 MiB SQLite allocation tolerance. Keep
        // the configured limit below it, then write two 3 MiB spans from one
        // trace. SQLite stores each attribute in both the span payload and
        // its lookup table, so the first write consumes most of that
        // allowance. The second cannot evict the earlier row because the
        // current trace is protected by the incoming batch.
        config.max_disk_bytes = 1;
        let store = TraceDiskStore::open(&config).expect("open trace store");
        let hot = Arc::new(InMemorySpanStorage::new_with_limits(16, 20 * 1024 * 1024));
        store.attach_hot_storage(&hot);

        let mut committed = test_span("trace-protected", "span-1", "large");
        committed.attributes = vec![("payload".to_string(), "x".repeat(3 * 1024 * 1024))];
        hot.add_spans(vec![committed]);
        store
            .flush()
            .expect("persist the first span before the cap is reached");

        let mut rejected = test_span("trace-protected", "span-2", "large");
        rejected.attributes = vec![("payload".to_string(), "x".repeat(3 * 1024 * 1024))];
        hot.add_spans(vec![rejected]);
        let error = store
            .flush()
            .expect_err("the protected trace must be rejected once it exhausts the disk cap");

        assert!(
            error.contains("trace storage limit reached"),
            "unexpected disk-cap error: {error}"
        );
        assert_eq!(
            store
                .get_spans_by_trace_id("trace-protected")
                .expect("read committed archive rows")
                .len(),
            1,
            "the failed batch must not partially persist its span"
        );
        assert_eq!(
            hot.dirty_spans(16, 20 * 1024 * 1024).len(),
            1,
            "the rejected span must remain eligible for a later retry"
        );

        store.shutdown();
        let status = store.status();
        assert_eq!(status.archive, "degraded");
        assert!(
            status
                .last_error
                .as_deref()
                .is_some_and(|message| message.contains("trace storage limit reached"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_write_limit_degrades_archive_without_partial_commit() {
        const CHILD_MARKER: &str = "III_TRACE_STORE_FILE_LIMIT_CHILD";
        const CHILD_DIRECTORY: &str = "III_TRACE_STORE_FILE_LIMIT_DIRECTORY";

        if std::env::var_os(CHILD_MARKER).is_some() {
            let directory = PathBuf::from(
                std::env::var_os(CHILD_DIRECTORY).expect("child trace directory must be provided"),
            );
            let store = TraceDiskStore::open(&test_config(&directory)).expect("open trace store");
            let hot = Arc::new(InMemorySpanStorage::new_with_limits(16, 20 * 1024 * 1024));
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
                // `ulimit -f` applies only to the isolated child process.
                // Ignore SIGXFSZ so SQLite receives EFBIG and the archive can
                // report degradation instead of terminating the process.
                "ulimit -f 1024; trap '' XFSZ; exec \"$1\" --exact \"$2\" --nocapture",
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
}
