// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Bounded process-output storage for Compose workers.
//!
//! Every project owns one writer. Worker pipes only read and enqueue records;
//! the writer serializes stdout and stderr into one rotating file per worker.
//! This keeps rotation race-free across streams and worker restarts.

use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::managed_engine::TerminalSanitizer;

pub const WORKER_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const WORKER_LOG_ARCHIVES: usize = 3;
pub const DEFAULT_TAIL_LINES: usize = 100;
pub const MAX_TAIL_LINES: usize = 1_000;
pub const MAX_WAIT_MS: u64 = 5_000;

const LOG_HEADER_PREFIX: &str = "# iii-compose-worker-log-v1 ";
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_BATCH_BYTES: usize = 512 * 1024;
const MAX_TAIL_SCAN_BYTES: u64 = WORKER_LOG_MAX_BYTES + MAX_LINE_BYTES as u64;
const MAX_WRITE_BATCH_RECORDS: usize = 128;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl LogStream {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LogCursor {
    /// Opaque identity of the active or archived log segment.
    pub generation: String,
    /// Byte offset after the last record returned from that segment.
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LogEntry {
    pub stream: LogStream,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContainerLogs {
    pub container: String,
    pub entries: Vec<LogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<LogCursor>,
    /// True when the requested cursor was older than the retained archives.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LogsOutcome {
    pub containers: Vec<ContainerLogs>,
}

impl LogsOutcome {
    pub fn is_empty(&self) -> bool {
        self.containers
            .iter()
            .all(|container| container.entries.is_empty())
    }

    fn has_cursor_progress(&self, cursors: &BTreeMap<String, LogCursor>) -> bool {
        self.containers.iter().any(|container| {
            container.cursor.as_ref() != cursors.get(&container.container)
                && container.cursor.is_some()
        })
    }
}

struct LogMessage {
    container: String,
    stream: LogStream,
    message: Vec<u8>,
}

/// One project-wide log writer and query surface.
pub struct LogStore {
    dir: PathBuf,
    sender: tokio::sync::mpsc::Sender<LogMessage>,
    changed: tokio::sync::watch::Sender<u64>,
    io_lock: Arc<Mutex<()>>,
    archives: usize,
}

impl LogStore {
    pub fn open(dir: PathBuf) -> std::io::Result<Self> {
        Self::open_with_limits(dir, WORKER_LOG_MAX_BYTES, WORKER_LOG_ARCHIVES)
    }

    fn open_with_limits(dir: PathBuf, max_bytes: u64, archives: usize) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        owner_only(&dir)?;

        let (sender, mut receiver) = tokio::sync::mpsc::channel::<LogMessage>(1_024);
        let (changed, _) = tokio::sync::watch::channel(0_u64);
        let writer_changed = changed.clone();
        let writer_dir = dir.clone();
        let io_lock = Arc::new(Mutex::new(()));
        let writer_io_lock = Arc::clone(&io_lock);
        tokio::spawn(async move {
            let mut files: HashMap<String, WorkerLogFile> = HashMap::new();
            while let Some(first) = receiver.recv().await {
                let mut records = Vec::with_capacity(MAX_WRITE_BATCH_RECORDS);
                records.push(first);
                while records.len() < MAX_WRITE_BATCH_RECORDS {
                    let Ok(record) = receiver.try_recv() else {
                        break;
                    };
                    records.push(record);
                }

                let io_lock = Arc::clone(&writer_io_lock);
                let dir = writer_dir.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let Ok(_guard) = io_lock.lock() else {
                        return None;
                    };
                    let mut wrote = false;
                    for record in records {
                        let file = match files.entry(record.container.clone()) {
                            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                            std::collections::hash_map::Entry::Vacant(entry) => {
                                let path = log_path(&dir, &record.container);
                                let Ok(file) = WorkerLogFile::open(path, max_bytes, archives)
                                else {
                                    continue;
                                };
                                entry.insert(file)
                            }
                        };

                        if file.write_entry(record.stream, &record.message).is_ok() {
                            wrote = true;
                        } else {
                            files.remove(&record.container);
                        }
                    }
                    Some((files, wrote))
                })
                .await;
                let Ok(Some((next_files, wrote))) = result else {
                    break;
                };
                files = next_files;
                if wrote {
                    writer_changed.send_modify(|generation| {
                        *generation = generation.wrapping_add(1);
                    });
                }
            }
        });

        Ok(Self {
            dir,
            sender,
            changed,
            io_lock,
            archives,
        })
    }

    pub fn path(&self, container: &str) -> PathBuf {
        log_path(&self.dir, container)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Drains both child streams until EOF and keeps every record after the
    /// worker becomes ready. The bounded channel applies backpressure instead
    /// of growing memory without limit when disk is slower than the worker.
    pub fn capture(
        &self,
        container: &str,
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
    ) {
        for (stream, reader) in [
            stdout.map(|reader| {
                (
                    LogStream::Stdout,
                    Box::new(reader) as Box<dyn AsyncRead + Unpin + Send>,
                )
            }),
            stderr.map(|reader| {
                (
                    LogStream::Stderr,
                    Box::new(reader) as Box<dyn AsyncRead + Unpin + Send>,
                )
            }),
        ]
        .into_iter()
        .flatten()
        {
            let sender = self.sender.clone();
            let container = container.to_string();
            tokio::spawn(async move {
                pump_stream(reader, container, stream, sender).await;
            });
        }
    }

    pub async fn query(
        &self,
        containers: Vec<String>,
        cursors: BTreeMap<String, LogCursor>,
        tail: usize,
        stream: Option<LogStream>,
        wait: Duration,
    ) -> std::io::Result<LogsOutcome> {
        let mut changed = self.changed.subscribe();
        changed.borrow_and_update();
        let first = self
            .query_once(containers.clone(), cursors.clone(), tail, stream)
            .await?;
        if !first.is_empty() || first.has_cursor_progress(&cursors) || wait.is_zero() {
            return Ok(first);
        }

        let _ = tokio::time::timeout(wait, changed.changed()).await;
        self.query_once(containers, cursors, tail, stream).await
    }

    async fn query_once(
        &self,
        containers: Vec<String>,
        cursors: BTreeMap<String, LogCursor>,
        tail: usize,
        stream: Option<LogStream>,
    ) -> std::io::Result<LogsOutcome> {
        let dir = self.dir.clone();
        let io_lock = Arc::clone(&self.io_lock);
        let archives = self.archives;
        let tail = tail.min(MAX_TAIL_LINES);
        tokio::task::spawn_blocking(move || {
            let _guard = io_lock
                .lock()
                .map_err(|_| std::io::Error::other("worker log lock is poisoned"))?;
            let mut batches = Vec::with_capacity(containers.len());
            for container in containers {
                batches.push(read_container_logs(
                    &dir,
                    &container,
                    cursors.get(&container),
                    tail,
                    stream,
                    archives,
                )?);
            }
            Ok(LogsOutcome {
                containers: batches,
            })
        })
        .await
        .map_err(std::io::Error::other)?
    }
}

async fn pump_stream(
    mut reader: Box<dyn AsyncRead + Unpin + Send>,
    container: String,
    stream: LogStream,
    sender: tokio::sync::mpsc::Sender<LogMessage>,
) {
    let mut sanitizer = TerminalSanitizer::default();
    let mut read_buffer = vec![0_u8; 8 * 1024];
    let mut line = Vec::new();

    while let Ok(count) = reader.read(&mut read_buffer).await {
        if count == 0 {
            break;
        }

        let clean = sanitizer.sanitize(&read_buffer[..count]);
        let mut remaining = clean.as_slice();
        while !remaining.is_empty() {
            let newline = remaining.iter().position(|byte| *byte == b'\n');
            let end = newline.map_or(remaining.len(), |index| index + 1);
            line.extend_from_slice(&remaining[..end]);
            remaining = &remaining[end..];

            while line.len() >= MAX_LINE_BYTES {
                let rest = line.split_off(MAX_LINE_BYTES);
                let chunk = std::mem::replace(&mut line, rest);
                if send_line(&sender, &container, stream, chunk).await.is_err() {
                    return;
                }
            }

            if newline.is_some() {
                let complete = std::mem::take(&mut line);
                if send_line(&sender, &container, stream, complete)
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }

    if !line.is_empty() {
        let _ = send_line(&sender, &container, stream, line).await;
    }
}

async fn send_line(
    sender: &tokio::sync::mpsc::Sender<LogMessage>,
    container: &str,
    stream: LogStream,
    mut message: Vec<u8>,
) -> Result<(), tokio::sync::mpsc::error::SendError<LogMessage>> {
    while matches!(message.last(), Some(b'\n' | b'\r')) {
        message.pop();
    }
    sender
        .send(LogMessage {
            container: container.to_string(),
            stream,
            message,
        })
        .await
}

struct WorkerLogFile {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max_bytes: u64,
    archives: usize,
}

impl WorkerLogFile {
    fn open(path: PathBuf, max_bytes: u64, archives: usize) -> std::io::Result<Self> {
        let max_bytes = max_bytes.max(1);
        if path.exists() && read_generation(&path)?.is_none() {
            rotate_path(&path, archives)?;
        }

        let mut log = Self {
            path,
            file: None,
            size: 0,
            max_bytes,
            archives,
        };
        log.open_active()?;
        if log.size >= log.max_bytes {
            log.rotate()?;
        }
        Ok(log)
    }

    fn open_active(&mut self) -> std::io::Result<()> {
        let new_file = !self.path.exists() || self.path.metadata()?.len() == 0;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true).read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path)?;
        owner_only(&self.path)?;
        if new_file {
            writeln!(file, "{LOG_HEADER_PREFIX}{}", uuid::Uuid::new_v4())?;
            file.flush()?;
        }
        self.size = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    fn write_entry(&mut self, stream: LogStream, message: &[u8]) -> std::io::Result<()> {
        let mut record = Vec::with_capacity(stream.as_str().len() + message.len() + 2);
        record.extend_from_slice(stream.as_str().as_bytes());
        record.push(b'\t');
        record.extend_from_slice(message);
        record.push(b'\n');

        if self.size + record.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("worker log file is not open"))?;
        file.write_all(&record)?;
        file.flush()?;
        self.size += record.len() as u64;
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        rotate_path(&self.path, self.archives)?;
        self.open_active()
    }
}

fn rotate_path(path: &Path, archives: usize) -> std::io::Result<()> {
    if archives == 0 {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }

    let oldest = archive_path(path, archives);
    if oldest.exists() {
        std::fs::remove_file(oldest)?;
    }
    for index in (1..archives).rev() {
        let source = archive_path(path, index);
        if source.exists() {
            std::fs::rename(source, archive_path(path, index + 1))?;
        }
    }
    if path.exists() {
        std::fs::rename(path, archive_path(path, 1))?;
    }
    Ok(())
}

fn read_container_logs(
    dir: &Path,
    container: &str,
    cursor: Option<&LogCursor>,
    tail: usize,
    stream: Option<LogStream>,
    archives: usize,
) -> std::io::Result<ContainerLogs> {
    let active = log_path(dir, container);
    let segments = segment_paths(&active, archives);
    let available: Vec<(PathBuf, String)> = segments
        .into_iter()
        .filter_map(|path| match read_generation(&path) {
            Ok(Some(generation)) => Some(Ok((path, generation))),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<std::io::Result<_>>()?;

    if available.is_empty() {
        return Ok(ContainerLogs {
            container: container.to_string(),
            entries: Vec::new(),
            cursor: None,
            truncated: false,
        });
    }

    match cursor {
        Some(cursor) => read_after_cursor(container, &available, cursor, tail, stream),
        None => read_tail(container, &available, tail, stream, false),
    }
}

fn read_after_cursor(
    container: &str,
    segments: &[(PathBuf, String)],
    cursor: &LogCursor,
    limit: usize,
    stream: Option<LogStream>,
) -> std::io::Result<ContainerLogs> {
    let Some(start) = segments
        .iter()
        .position(|(_, generation)| generation == &cursor.generation)
    else {
        return read_tail(container, segments, limit, stream, true);
    };

    let mut entries = Vec::new();
    let mut bytes = 0;
    let mut next = cursor.clone();
    for (index, (path, generation)) in segments.iter().enumerate().skip(start) {
        let offset = if index == start {
            cursor.offset
        } else {
            header_end(path)?.unwrap_or_default()
        };
        let (more, position) =
            read_entries(path, offset, limit - entries.len(), stream, &mut bytes)?;
        entries.extend(more);
        next = LogCursor {
            generation: generation.clone(),
            offset: position,
        };
        if entries.len() >= limit || bytes >= MAX_BATCH_BYTES {
            break;
        }
    }

    Ok(ContainerLogs {
        container: container.to_string(),
        entries,
        cursor: Some(next),
        truncated: false,
    })
}

fn read_tail(
    container: &str,
    segments: &[(PathBuf, String)],
    limit: usize,
    stream: Option<LogStream>,
    truncated: bool,
) -> std::io::Result<ContainerLogs> {
    let mut entries = Vec::new();
    for (path, _) in segments.iter().rev() {
        let mut from_segment = tail_entries(path, limit.saturating_sub(entries.len()), stream)?;
        from_segment.append(&mut entries);
        entries = from_segment;
        if entries.len() >= limit {
            break;
        }
    }
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }

    let (active, generation) = segments
        .last()
        .ok_or_else(|| std::io::Error::other("worker log has no readable segment"))?;
    let offset = active.metadata()?.len();
    Ok(ContainerLogs {
        container: container.to_string(),
        entries,
        cursor: Some(LogCursor {
            generation: generation.clone(),
            offset,
        }),
        truncated,
    })
}

fn read_entries(
    path: &Path,
    offset: u64,
    limit: usize,
    stream: Option<LogStream>,
    bytes: &mut usize,
) -> std::io::Result<(Vec<LogEntry>, u64)> {
    let mut file = BufReader::new(File::open(path)?);
    let length = file.get_ref().metadata()?.len();
    file.seek(SeekFrom::Start(offset.min(length)))?;
    let mut entries = Vec::new();
    let mut record = Vec::new();
    while entries.len() < limit && *bytes < MAX_BATCH_BYTES {
        record.clear();
        let count = file.read_until(b'\n', &mut record)?;
        if count == 0 {
            break;
        }
        *bytes += count;
        if let Some(entry) = parse_entry(&record, stream) {
            entries.push(entry);
        }
    }
    Ok((entries, file.stream_position()?))
}

fn tail_entries(
    path: &Path,
    limit: usize,
    stream: Option<LogStream>,
) -> std::io::Result<Vec<LogEntry>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(MAX_TAIL_SCAN_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((length - start) as usize);
    file.take(MAX_TAIL_SCAN_BYTES).read_to_end(&mut bytes)?;
    let text = if start > 0 {
        bytes
            .splitn(2, |byte| *byte == b'\n')
            .nth(1)
            .unwrap_or_default()
    } else {
        bytes.as_slice()
    };
    let mut entries: Vec<LogEntry> = text
        .split_inclusive(|byte| *byte == b'\n')
        .filter_map(|record| parse_entry(record, stream))
        .collect();
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }
    Ok(entries)
}

fn parse_entry(record: &[u8], filter: Option<LogStream>) -> Option<LogEntry> {
    let record = record.strip_suffix(b"\n").unwrap_or(record);
    let separator = record.iter().position(|byte| *byte == b'\t')?;
    let stream = &record[..separator];
    let message = &record[separator + 1..];
    let stream = match stream {
        b"stdout" => LogStream::Stdout,
        b"stderr" => LogStream::Stderr,
        _ => return None,
    };
    if filter.is_some_and(|filter| filter != stream) {
        return None;
    }
    Some(LogEntry {
        stream,
        message: String::from_utf8_lossy(message).into_owned(),
    })
}

fn read_generation(path: &Path) -> std::io::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut header = String::new();
    BufReader::new(File::open(path)?).read_line(&mut header)?;
    Ok(header
        .trim_end()
        .strip_prefix(LOG_HEADER_PREFIX)
        .map(str::to_string))
}

fn header_end(path: &Path) -> std::io::Result<Option<u64>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut reader = BufReader::new(File::open(path)?);
    let mut header = Vec::new();
    reader.read_until(b'\n', &mut header)?;
    Ok(Some(reader.stream_position()?))
}

fn segment_paths(path: &Path, archives: usize) -> Vec<PathBuf> {
    let mut paths = (1..=archives)
        .rev()
        .map(|index| archive_path(path, index))
        .collect::<Vec<_>>();
    paths.push(path.to_path_buf());
    paths
}

fn archive_path(path: &Path, index: usize) -> PathBuf {
    let mut archive = path.as_os_str().to_os_string();
    archive.push(format!(".{index}"));
    PathBuf::from(archive)
}

fn log_path(dir: &Path, container: &str) -> PathBuf {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut name = String::with_capacity(container.len() + 4);
    for byte in container.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            name.push(char::from(byte));
        } else {
            name.push('%');
            name.push(char::from(HEX[(byte >> 4) as usize]));
            name.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    name.push_str(".log");
    dir.join(name)
}

#[cfg(unix)]
fn owner_only(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if path.is_dir() { 0o700 } else { 0o600 }),
    )
}

#[cfg(not(unix))]
fn owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    fn write(file: &mut WorkerLogFile, stream: LogStream, message: &str) {
        file.write_entry(stream, message.as_bytes()).unwrap();
    }

    #[test]
    fn worker_log_rotates_without_losing_a_cursor_in_retained_archives() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.log");
        let mut file = WorkerLogFile::open(path, 130, 2).unwrap();
        write(&mut file, LogStream::Stdout, "before rotation");
        let first = read_container_logs(dir.path(), "queue", None, 10, None, 2).unwrap();
        let cursor = first.cursor.unwrap();

        for index in 0..3 {
            write(&mut file, LogStream::Stdout, &format!("line-{index:02}"));
        }
        let after = read_container_logs(dir.path(), "queue", Some(&cursor), 20, None, 2).unwrap();

        assert!(
            after.entries.iter().any(|entry| entry.message == "line-00"),
            "cursor did not continue into the retained archive: {after:?}"
        );
    }

    #[test]
    fn worker_log_reports_a_cursor_older_than_retention_as_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.log");
        let mut file = WorkerLogFile::open(path, 80, 1).unwrap();
        write(&mut file, LogStream::Stdout, "old");
        let old = read_container_logs(dir.path(), "queue", None, 10, None, 1)
            .unwrap()
            .cursor
            .unwrap();

        for index in 0..30 {
            write(&mut file, LogStream::Stdout, &format!("new-{index:02}"));
        }
        let result = read_container_logs(dir.path(), "queue", Some(&old), 10, None, 1).unwrap();

        assert!(result.truncated);
    }

    #[test]
    fn worker_log_filters_stderr_without_restyling_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.log");
        let mut file = WorkerLogFile::open(path, 1_024, 1).unwrap();
        write(&mut file, LogStream::Stdout, "ordinary output");
        write(&mut file, LogStream::Stderr, "failure output");

        let result =
            read_container_logs(dir.path(), "queue", None, 10, Some(LogStream::Stderr), 1).unwrap();

        assert_eq!(
            result.entries,
            vec![LogEntry {
                stream: LogStream::Stderr,
                message: "failure output".to_string(),
            }]
        );
    }

    #[test]
    fn worker_name_cannot_escape_the_log_directory() {
        let dir = Path::new("logs");
        let path = log_path(dir, "../../outside");

        assert_eq!(path, dir.join("..%2F..%2Foutside.log"));
    }

    #[tokio::test]
    async fn capture_keeps_output_written_after_the_initial_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let store = LogStore::open_with_limits(dir.path().to_path_buf(), 1_024, 1).unwrap();
        let (mut writer, reader) = tokio::io::duplex(1_024);
        let sender = store.sender.clone();
        tokio::spawn(async move {
            pump_stream(
                Box::new(reader),
                "queue".to_string(),
                LogStream::Stdout,
                sender,
            )
            .await;
        });

        writer.write_all(b"boot output\n").await.unwrap();
        let first = store
            .query(
                vec!["queue".to_string()],
                BTreeMap::new(),
                10,
                None,
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        let cursor = first.containers[0].cursor.clone().unwrap();

        writer.write_all(b"output after ready\n").await.unwrap();
        let after = store
            .query(
                vec!["queue".to_string()],
                BTreeMap::from([("queue".to_string(), cursor)]),
                10,
                None,
                Duration::from_secs(1),
            )
            .await
            .unwrap();

        assert_eq!(after.containers[0].entries[0].message, "output after ready");
    }
}
