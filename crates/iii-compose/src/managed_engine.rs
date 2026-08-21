// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Lifecycle of the engine process owned by `iii compose up`.

use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};

use tokio::io::AsyncReadExt;

use crate::{
    error::{ComposeError, Result},
    process::{ChildOutput, DEFAULT_STOP_GRACE, Supervised, spawn_supervised_piped},
    state::StateStore,
};

/// The config selected by the root `iii` CLI when `--config` is absent.
pub const DEFAULT_ENGINE_CONFIG: &str = "config.yaml";

/// Maximum size of the active engine log before it rolls into an archive.
const ENGINE_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
/// Number of old engine log segments retained beside `engine.log`.
const ENGINE_LOG_ARCHIVES: usize = 3;

/// The engine process owned by one foreground compose invocation.
pub struct ManagedEngine {
    process: Supervised,
    logs: LogCapture,
    config_path: PathBuf,
    log_path: PathBuf,
}

impl ManagedEngine {
    /// Starts the current `iii` executable with output detached from compose's
    /// terminal and captured below this daemon namespace's state directory.
    pub async fn start(config_path: impl Into<PathBuf>, daemon_namespace: &str) -> Result<Self> {
        let executable =
            std::env::current_exe().map_err(|err| ComposeError::EngineSpawnFailed {
                message: format!("could not locate the current iii executable: {err}"),
            })?;
        let log_path = engine_log_path(&StateStore::root()?, daemon_namespace);
        Self::spawn_with_paths(&executable, &config_path.into(), &log_path).await
    }

    async fn spawn_with_paths(
        executable: &Path,
        config_path: &Path,
        log_path: &Path,
    ) -> Result<Self> {
        let parent = log_path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|source| ComposeError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        owner_only(parent).map_err(|source| ComposeError::Io {
            path: parent.to_path_buf(),
            source,
        })?;

        let log = RotatingLog::open(log_path, ENGINE_LOG_MAX_BYTES, ENGINE_LOG_ARCHIVES).map_err(
            |source| ComposeError::Io {
                path: log_path.to_path_buf(),
                source,
            },
        )?;

        let mut command = tokio::process::Command::new(executable);
        command
            .arg("--config")
            .arg(config_path)
            .stdin(Stdio::null());

        let (process, output) =
            spawn_supervised_piped(command).map_err(|err| ComposeError::EngineSpawnFailed {
                message: format!("could not start {}: {err}", executable.display()),
            })?;
        let logs = capture_output(output, log);

        Ok(Self {
            process,
            logs,
            config_path: config_path.to_path_buf(),
            log_path: log_path.to_path_buf(),
        })
    }

    pub fn pid(&self) -> u32 {
        self.process.pid
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    pub fn follow_command(&self) -> String {
        follow_command(&self.log_path)
    }

    pub fn log_tail(&self) -> Option<String> {
        log_tail(&self.log_path)
    }

    #[cfg(test)]
    async fn wait(&self) -> ExitStatus {
        let status = self.process.wait().await;
        self.finish_logging().await;
        status
    }

    pub fn poll(&self) -> crate::process::Outcome {
        self.process.poll()
    }

    pub async fn stop(&self, grace: Duration) -> ExitStatus {
        let status = self.process.stop(grace).await;
        self.finish_logging().await;
        status
    }

    pub async fn stop_with_default_grace(&self) -> ExitStatus {
        self.stop(DEFAULT_STOP_GRACE).await
    }

    /// Lets the output readers flush the final bytes after the child exits.
    pub async fn finish_logging(&self) {
        let _ = tokio::time::timeout(Duration::from_secs(2), self.logs.wait()).await;
    }
}

struct LogCapture(tokio::sync::watch::Receiver<bool>);

impl LogCapture {
    async fn wait(&self) {
        let mut done = self.0.clone();
        while !*done.borrow_and_update() {
            if done.changed().await.is_err() {
                break;
            }
        }
    }
}

/// One writer owns both child streams, so rotation cannot race stderr against
/// stdout. Readers keep draining even if disk writes fail, avoiding a full pipe
/// that would otherwise stall the engine itself.
fn capture_output(output: ChildOutput, log: RotatingLog) -> LogCapture {
    let (chunks_tx, mut chunks_rx) = tokio::sync::mpsc::channel::<(usize, Vec<u8>)>(64);
    for (stream_id, stream) in [
        output
            .stdout
            .map(|stream| Box::new(stream) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        output
            .stderr
            .map(|stream| Box::new(stream) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    .enumerate()
    {
        let chunks_tx = chunks_tx.clone();
        tokio::spawn(async move {
            let mut stream = stream;
            let mut buffer = vec![0_u8; 8 * 1024];
            while let Ok(read) = stream.read(&mut buffer).await {
                if read == 0
                    || chunks_tx
                        .send((stream_id, buffer[..read].to_vec()))
                        .await
                        .is_err()
                {
                    break;
                }
            }
        });
    }
    drop(chunks_tx);

    let (done_tx, done_rx) = tokio::sync::watch::channel(false);
    tokio::task::spawn_blocking(move || {
        let mut log = Some(log);
        let mut sanitizers = [TerminalSanitizer::default(), TerminalSanitizer::default()];
        while let Some((stream_id, chunk)) = chunks_rx.blocking_recv() {
            let chunk = sanitizers[stream_id].sanitize(&chunk);
            if let Some(sink) = log.as_mut()
                && !chunk.is_empty()
                && sink.write_bounded(&chunk).is_err()
            {
                log = None;
            }
        }
        if let Some(mut sink) = log {
            let _ = sink.file.flush();
        }
        let _ = done_tx.send(true);
    });

    LogCapture(done_rx)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum TerminalState {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
    ControlString,
    ControlStringEscape,
}

/// Removes terminal control sequences while preserving ordinary UTF-8 output.
///
/// State is retained across pipe reads because an ANSI/OSC sequence or a UTF-8
/// scalar can be split at any byte boundary. Unterminated control strings stay
/// suppressed, which fails closed instead of letting their payload reach a
/// terminal through `tail -f` or an error summary.
#[derive(Debug, Default)]
struct TerminalSanitizer {
    state: TerminalState,
    utf8: Vec<u8>,
    utf8_expected: usize,
}

impl TerminalSanitizer {
    fn sanitize(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut clean = Vec::with_capacity(bytes.len());

        for &byte in bytes {
            if self.state == TerminalState::Ground && !self.utf8.is_empty() {
                self.utf8.push(byte);
                if self.utf8.len() == self.utf8_expected {
                    if let Ok(text) = std::str::from_utf8(&self.utf8)
                        && text.chars().all(|character| !character.is_control())
                    {
                        clean.extend_from_slice(&self.utf8);
                    }
                    self.utf8.clear();
                    self.utf8_expected = 0;
                }
                continue;
            }

            match self.state {
                TerminalState::Ground => match byte {
                    0x1b => self.state = TerminalState::Escape,
                    b'\n' | b'\t' | 0x20..=0x7e => clean.push(byte),
                    0xc2..=0xdf => self.start_utf8(byte, 2),
                    0xe0..=0xef => self.start_utf8(byte, 3),
                    0xf0..=0xf4 => self.start_utf8(byte, 4),
                    _ => {}
                },
                TerminalState::Escape => {
                    self.state = match byte {
                        0x1b => TerminalState::Escape,
                        b'[' => TerminalState::Csi,
                        b']' => TerminalState::Osc,
                        b'P' | b'X' | b'^' | b'_' => TerminalState::ControlString,
                        _ => TerminalState::Ground,
                    };
                }
                TerminalState::Csi => {
                    if byte == 0x1b {
                        self.state = TerminalState::Escape;
                    } else if (0x40..=0x7e).contains(&byte) {
                        self.state = TerminalState::Ground;
                    }
                }
                TerminalState::Osc => match byte {
                    0x07 => self.state = TerminalState::Ground,
                    0x1b => self.state = TerminalState::OscEscape,
                    _ => {}
                },
                TerminalState::OscEscape => {
                    self.state = match byte {
                        b'\\' => TerminalState::Ground,
                        0x1b => TerminalState::OscEscape,
                        _ => TerminalState::Osc,
                    };
                }
                TerminalState::ControlString => {
                    if byte == 0x1b {
                        self.state = TerminalState::ControlStringEscape;
                    }
                }
                TerminalState::ControlStringEscape => {
                    self.state = match byte {
                        b'\\' => TerminalState::Ground,
                        0x1b => TerminalState::ControlStringEscape,
                        _ => TerminalState::ControlString,
                    };
                }
            }
        }

        clean
    }

    fn start_utf8(&mut self, byte: u8, expected: usize) {
        self.utf8.push(byte);
        self.utf8_expected = expected;
    }
}

fn sanitize_terminal_output(bytes: &[u8]) -> Vec<u8> {
    TerminalSanitizer::default().sanitize(bytes)
}

struct RotatingLog {
    path: PathBuf,
    file: std::fs::File,
    size: u64,
    max_bytes: u64,
    archives: usize,
}

impl RotatingLog {
    fn open(path: &Path, max_bytes: u64, archives: usize) -> std::io::Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).read(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        owner_only(path)?;
        let mut size = file.metadata()?.len();
        if size > 0 && size < max_bytes {
            file.seek(SeekFrom::Start(0))?;
            let mut existing = Vec::with_capacity(size as usize);
            file.read_to_end(&mut existing)?;
            let clean = sanitize_terminal_output(&existing);
            if clean != existing {
                file.set_len(0)?;
                file.seek(SeekFrom::Start(0))?;
                file.write_all(&clean)?;
                file.flush()?;
                size = clean.len() as u64;
            }
        }
        let mut log = Self {
            path: path.to_path_buf(),
            file,
            size,
            max_bytes,
            archives,
        };
        if log.size >= log.max_bytes {
            log.rotate()?;
        }
        Ok(log)
    }

    fn write_bounded(&mut self, mut bytes: &[u8]) -> std::io::Result<()> {
        while !bytes.is_empty() {
            if self.size >= self.max_bytes {
                self.rotate()?;
            }
            let available = (self.max_bytes - self.size) as usize;
            let count = available.min(bytes.len());
            self.file.write_all(&bytes[..count])?;
            self.size += count as u64;
            bytes = &bytes[count..];
        }
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file.flush()?;

        if self.archives > 0 {
            let oldest = archive_path(&self.path, self.archives);
            if oldest.exists() {
                std::fs::remove_file(oldest)?;
            }
            for index in (1..self.archives).rev() {
                let source = archive_path(&self.path, index);
                if source.exists() {
                    std::fs::rename(source, archive_path(&self.path, index + 1))?;
                }
            }

            let archive = archive_path(&self.path, 1);
            let mut options = std::fs::OpenOptions::new();
            options.create(true).write(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut archive_file = options.open(&archive)?;
            owner_only(&archive)?;

            let start = self.size.saturating_sub(self.max_bytes);
            self.file.seek(SeekFrom::Start(start))?;
            let mut remaining = self.max_bytes.min(self.size);
            let mut buffer = [0_u8; 8 * 1024];
            let mut sanitizer = TerminalSanitizer::default();
            while remaining > 0 {
                let limit = remaining.min(buffer.len() as u64) as usize;
                let count = self.file.read(&mut buffer[..limit])?;
                if count == 0 {
                    break;
                }
                archive_file.write_all(&sanitizer.sanitize(&buffer[..count]))?;
                remaining -= count as u64;
            }
            archive_file.flush()?;
        }

        // Keep the active path and inode stable so `tail -f engine.log` keeps
        // following across rotations on Unix and PowerShell keeps its handle.
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.size = 0;
        Ok(())
    }
}

fn archive_path(path: &Path, index: usize) -> PathBuf {
    let mut archive = path.as_os_str().to_os_string();
    archive.push(format!(".{index}"));
    PathBuf::from(archive)
}

fn engine_log_path(root: &Path, daemon_namespace: &str) -> PathBuf {
    root.join(daemon_namespace).join("engine.log")
}

#[cfg(unix)]
fn follow_command(path: &Path) -> String {
    let quoted = path.to_string_lossy().replace('\'', "'\"'\"'");
    format!("tail -f '{quoted}'")
}

#[cfg(windows)]
fn follow_command(path: &Path) -> String {
    let quoted = path.to_string_lossy().replace('\'', "''");
    format!("Get-Content -LiteralPath '{quoted}' -Wait")
}

fn log_tail(path: &Path) -> Option<String> {
    const LINES: usize = 5;
    const WIDTH: usize = 240;

    let bytes = std::fs::read(path).ok()?;
    let clean = sanitize_terminal_output(&bytes);
    let text = String::from_utf8_lossy(&clean);
    let tail: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(LINES)
        .collect();
    if tail.is_empty() {
        return None;
    }

    Some(
        tail.into_iter()
            .rev()
            .map(|line| format!("  {}", line.chars().take(WIDTH).collect::<String>()))
            .collect::<Vec<_>>()
            .join("\n"),
    )
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
    use std::{path::Path, time::Duration};

    use super::*;

    #[test]
    fn engine_log_lives_under_the_daemon_namespace() {
        assert_eq!(
            engine_log_path(Path::new("/state"), "blue-whale"),
            Path::new("/state/blue-whale/engine.log")
        );
    }

    #[cfg(unix)]
    #[test]
    fn follow_command_quotes_paths_for_a_shell() {
        assert_eq!(
            follow_command(Path::new("/tmp/my project's/engine.log")),
            "tail -f '/tmp/my project'\"'\"'s/engine.log'"
        );
    }

    #[cfg(windows)]
    #[test]
    fn follow_command_quotes_paths_for_powershell() {
        assert_eq!(
            follow_command(Path::new("C:\\my project's\\engine.log")),
            "Get-Content -LiteralPath 'C:\\my project''s\\engine.log' -Wait"
        );
    }

    #[test]
    fn log_tail_is_bounded_to_the_last_five_non_empty_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("engine.log");
        std::fs::write(&log, "one\n\ntwo\nthree\nfour\nfive\nsix\n").unwrap();

        assert_eq!(
            log_tail(&log).as_deref(),
            Some("  two\n  three\n  four\n  five\n  six")
        );
    }

    #[test]
    fn log_tail_strips_terminal_escape_sequences_from_existing_logs() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("engine.log");
        std::fs::write(
            &log,
            "\u{1b}[31mred\u{1b}[0m\n\u{1b}]2;forged title\u{7}visible\n",
        )
        .unwrap();

        assert_eq!(log_tail(&log).as_deref(), Some("  red\n  visible"));
    }

    #[test]
    fn terminal_sanitizer_tracks_escape_and_utf8_state_across_chunks() {
        let mut sanitizer = TerminalSanitizer::default();
        let mut clean = Vec::new();
        for chunk in [
            b"\x1b[3".as_slice(),
            b"1mred\x1b".as_slice(),
            b"[0m \x1b]2;forged".as_slice(),
            b" title\x1b".as_slice(),
            b"\\visible caf\xc3".as_slice(),
            b"\xa9\n".as_slice(),
        ] {
            clean.extend(sanitizer.sanitize(chunk));
        }

        assert_eq!(String::from_utf8(clean).unwrap(), "red visible café\n");
    }

    #[cfg(unix)]
    #[test]
    fn opening_an_existing_log_hardens_its_permissions() {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("engine.log");
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o644)
            .open(&log)
            .unwrap();
        std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o644)).unwrap();

        let _log = RotatingLog::open(&log, ENGINE_LOG_MAX_BYTES, ENGINE_LOG_ARCHIVES).unwrap();

        assert_eq!(
            std::fs::metadata(log).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_receives_config_and_captures_both_output_streams() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake iii");
        let config = dir.path().join("project config.yaml");
        let log = dir.path().join("engine.log");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'args:%s\\n' \"$*\"\nprintf '\\033[31mengine stdout\\033[0m\\n'\nprintf '\\033]2;forged title\\007engine stderr\\n' >&2\nexit 7\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log)
            .await
            .unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), engine.wait())
            .await
            .expect("fake engine should exit");

        assert_eq!(status.code(), Some(7));
        let output = std::fs::read_to_string(&log).unwrap();
        assert!(
            output.contains(&format!("args:--config {}", config.display())),
            "config argument missing from {output:?}"
        );
        assert!(
            output.contains("engine stderr"),
            "stderr missing from {output:?}"
        );
        assert!(
            output.contains("engine stdout"),
            "stdout missing from {output:?}"
        );
        assert!(
            !output.contains('\u{1b}') && !output.contains("forged title"),
            "terminal escape sequence was persisted in {output:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_log_rotates_before_it_can_grow_without_bound() {
        use std::os::unix::fs::PermissionsExt;

        const EXPECTED_LIMIT: u64 = 10 * 1024 * 1024;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("noisy-iii");
        let config = dir.path().join("config.yaml");
        let log = dir.path().join("engine.log");
        std::fs::write(
            &script,
            "#!/bin/sh\ndd if=/dev/zero bs=1048576 count=11 2>/dev/null | tr '\\000' x\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log)
            .await
            .unwrap();
        let status = tokio::time::timeout(Duration::from_secs(10), engine.wait())
            .await
            .expect("noisy engine should exit");

        assert!(status.success());
        assert!(
            std::fs::metadata(&log).unwrap().len() <= EXPECTED_LIMIT,
            "current log exceeded its size limit"
        );
        let archive = log.with_file_name("engine.log.1");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !archive.exists() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            archive.exists(),
            "the previous log segment was not archived"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stopping_the_engine_stops_its_process_group() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-iii");
        let config = dir.path().join("config.yaml");
        let log = dir.path().join("engine.log");
        std::fs::write(
            &script,
            "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile :; do sleep 1; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log)
            .await
            .unwrap();
        let pid = engine.pid();
        assert!(crate::process::is_running(pid));

        let _status = engine.stop(Duration::from_secs(2)).await;

        assert!(!crate::process::is_running(pid));
    }
}
