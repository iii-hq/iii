// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

//! Durable trace storage.
//!
//! The hot store remains the realtime ingestion path. This module owns the
//! write-behind SQLite connection and reads historical finalized spans without
//! making the application workload wait for disk I/O.

use super::{config::TraceStorageConfig, otel::InMemorySpanStorage};
use rusqlite::{Connection, params};
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

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TraceStorageStatus {
    pub archive: String,
    pub completeness: String,
    pub known_dropped_spans: u64,
    pub physical_bytes: u64,
    pub max_disk_bytes: u64,
    pub last_error: Option<String>,
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
        configure_connection(&mut connection)?;
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
            .map_err(|err| format!("cannot mark trace database as active: {err}"))?;

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

    pub fn get_root_spans(&self) -> Result<Vec<StoredSpan>, String> {
        let mut connection = Connection::open(&self.database_path)
            .map_err(|err| format!("cannot open trace archive for reading: {err}"))?;
        configure_read_connection(&mut connection)?;
        let epoch = self.epoch.load(Ordering::Acquire) as i64;
        let mut statement = connection
            .prepare(
                "SELECT s.payload FROM spans s
                 WHERE s.epoch = ?1
                   AND (s.parent_span_id IS NULL OR NOT EXISTS (
                       SELECT 1 FROM spans p
                       WHERE p.epoch = s.epoch
                         AND p.trace_id = s.trace_id
                         AND p.span_id = s.parent_span_id
                   ))
                 ORDER BY s.start_time_ns ASC, s.trace_id ASC, s.span_id ASC",
            )
            .map_err(|err| format!("cannot prepare root trace read: {err}"))?;
        let rows = statement
            .query_map(params![epoch], |row| row.get::<_, String>(0))
            .map_err(|err| format!("cannot query root trace archive: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("cannot read root trace archive row: {err}"))?;

        Ok(self.decode_payloads(rows))
    }

    pub fn get_spans_by_trace_id(&self, trace_id: &str) -> Result<Vec<StoredSpan>, String> {
        self.read_spans(Some(trace_id))
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
                .map_err(|err| format!("cannot prepare trace read: {err}"))?
        } else {
            connection
                .prepare(
                    "SELECT payload FROM spans WHERE epoch = ?1\n                     ORDER BY start_time_ns ASC, trace_id ASC, span_id ASC",
                )
                .map_err(|err| format!("cannot prepare trace read: {err}"))?
        };

        let rows = if let Some(trace_id) = trace_id {
            statement
                .query_map(params![epoch, trace_id], |row| row.get::<_, String>(0))
                .map_err(|err| format!("cannot query trace archive: {err}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| format!("cannot read trace archive row: {err}"))?
        } else {
            statement
                .query_map(params![epoch], |row| row.get::<_, String>(0))
                .map_err(|err| format!("cannot query trace archive: {err}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| format!("cannot read trace archive row: {err}"))?
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
        .map_err(|err| format!("cannot start trace transaction: {err}"))?;

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
        .map_err(|err| format!("cannot persist trace {}: {err}", span.trace_id))?;

        tx.execute(
            "DELETE FROM span_attributes WHERE epoch = ?1 AND trace_id = ?2 AND span_id = ?3",
            params![epoch, span.trace_id, span.span_id],
        )
        .map_err(|err| format!("cannot replace trace attributes: {err}"))?;
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
            .map_err(|err| format!("cannot persist trace attribute: {err}"))?;
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
        .map_err(|err| format!("cannot update trace metadata: {err}"))?;
    }

    tx.commit()
        .map_err(|err| format!("cannot commit trace batch: {err}"))?;
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
    let current = directory_size(&store.directory)
        .map_err(|err| format!("cannot measure trace storage directory: {err}"))?;
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
        .map_err(|err| format!("cannot prepare trace eviction: {err}"))?;
    let mut candidates = candidate_statement
        .query_map(params![epoch], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| format!("cannot list trace eviction candidates: {err}"))?;
    let mut deleted = Vec::new();
    let mut remaining = current;
    while let Some((candidate, candidate_bytes)) = candidates
        .next()
        .transpose()
        .map_err(|err| err.to_string())?
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
        let tx = connection
            .transaction()
            .map_err(|err| format!("cannot start trace eviction: {err}"))?;
        for trace_id in &deleted {
            tx.execute(
                "DELETE FROM span_attributes WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| format!("cannot delete trace attributes: {err}"))?;
            tx.execute(
                "DELETE FROM spans WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| format!("cannot delete trace spans: {err}"))?;
            tx.execute(
                "DELETE FROM trace_meta WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| format!("cannot delete trace metadata: {err}"))?;
        }
        tx.commit()
            .map_err(|err| format!("cannot commit trace eviction: {err}"))?;
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
        .map_err(|err| format!("cannot start trace clear: {err}"))?;
    tx.execute("DELETE FROM span_attributes", [])
        .map_err(|err| format!("cannot clear trace attributes: {err}"))?;
    tx.execute("DELETE FROM spans", [])
        .map_err(|err| format!("cannot clear trace spans: {err}"))?;
    tx.execute("DELETE FROM trace_meta", [])
        .map_err(|err| format!("cannot clear trace metadata: {err}"))?;
    let next_epoch = store.epoch.fetch_add(1, Ordering::AcqRel) + 1;
    tx.execute(
        "INSERT INTO storage_meta (key, value) VALUES ('epoch', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![next_epoch.to_string()],
    )
    .map_err(|err| format!("cannot persist trace epoch: {err}"))?;
    tx.commit()
        .map_err(|err| format!("cannot commit trace clear: {err}"))?;
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
        .map_err(|err| format!("cannot reset dropped trace count: {err}"))?;
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
        .map_err(|err| format!("cannot prepare trace retention: {err}"))?;
    let traces = statement
        .query_map(params![epoch, cutoff as i64], |row| row.get::<_, String>(0))
        .map_err(|err| format!("cannot query trace retention: {err}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("cannot read trace retention: {err}"))?;
    drop(statement);
    for trace_id in traces {
        connection
            .execute(
                "DELETE FROM span_attributes WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| format!("cannot retain trace attributes: {err}"))?;
        connection
            .execute(
                "DELETE FROM spans WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| format!("cannot retain trace spans: {err}"))?;
        connection
            .execute(
                "DELETE FROM trace_meta WHERE epoch = ?1 AND trace_id = ?2",
                params![epoch, trace_id],
            )
            .map_err(|err| format!("cannot retain trace metadata: {err}"))?;
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
}
