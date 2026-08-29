// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Host-side source transport for local worker VMs.
//!
//! This module observes the host project and sends changed entries to
//! `iii-init`. It never restarts a process or VM. Restart policy stays inside
//! the worker, where normal tools consume the guest kernel's file events.

use std::collections::{BTreeSet, HashSet};
use std::fs::{File, Metadata};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use iii_supervisor::source_sync::{self, EntryHeader, EntryKind};
use notify::{Event, EventKind, Watcher};

use super::source_watcher::{DEBOUNCE_MS, register_new_dirs, should_ignore_path, watch_pruned};

const FILE_READ_ATTEMPTS: usize = 3;

#[derive(Debug)]
struct FileSnapshot {
    contents: Vec<u8>,
    mode: u32,
}

pub fn spawn(source_root: PathBuf, stream: UnixStream) -> io::Result<()> {
    let source_root = std::fs::canonicalize(&source_root)?;
    let (tx, rx) = mpsc::channel::<Event>();
    let filter_root = source_root.clone();
    let mut watcher = notify::RecommendedWatcher::new(
        move |result: Result<Event, notify::Error>| match result {
            Ok(event)
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                ) && event.paths.iter().any(|path| {
                    path != &filter_root && !should_ignore_path(path, &filter_root)
                }) =>
            {
                let _ = tx.send(event);
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "source sync: notify backend error"),
        },
        notify::Config::default(),
    )
    .map_err(io::Error::other)?;

    let mut registered = HashSet::new();
    watch_pruned(&mut watcher, &source_root, &mut registered).map_err(io::Error::other)?;

    std::thread::Builder::new()
        .name("iii-source-sync".to_string())
        .spawn(move || {
            if let Err(error) = run_loop(source_root, stream, watcher, registered, rx) {
                tracing::warn!(%error, "source sync stopped");
            }
        })?;
    Ok(())
}

fn run_loop(
    source_root: PathBuf,
    mut stream: UnixStream,
    mut watcher: notify::RecommendedWatcher,
    mut registered: HashSet<PathBuf>,
    rx: mpsc::Receiver<Event>,
) -> io::Result<()> {
    tracing::info!(path = %source_root.display(), "source sync: online");

    loop {
        let first = match rx.recv() {
            Ok(event) => event,
            Err(_) => return Ok(()),
        };
        let mut paths = first.paths;
        loop {
            match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                Ok(event) => paths.extend(event.paths),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }

        register_new_dirs(&mut watcher, &paths, &source_root, &mut registered);
        for path in coalesce_paths(paths, &source_root) {
            send_current_entry(&mut stream, &source_root, &path)?;
        }
    }
}

fn coalesce_paths(paths: Vec<PathBuf>, root: &Path) -> Vec<PathBuf> {
    let mut unique: Vec<PathBuf> = paths
        .into_iter()
        .filter(|path| path != root && path.starts_with(root))
        .filter(|path| !should_ignore_path(path, root))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    unique.sort_by_key(|path| path.components().count());

    let mut selected: Vec<PathBuf> = Vec::new();
    for path in unique {
        if selected.iter().any(|parent| path.starts_with(parent)) {
            continue;
        }
        selected.push(path);
    }
    selected
}

fn send_current_entry(stream: &mut UnixStream, root: &Path, path: &Path) -> io::Result<()> {
    if should_ignore_path(path, root) {
        return Ok(());
    }
    let relative = relative_utf8(root, path)?;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let link = std::fs::read_link(path)?;
            let bytes = link.as_os_str().as_bytes();
            send_header(
                stream,
                EntryKind::Symlink,
                relative,
                metadata.permissions().mode(),
                bytes.len() as u64,
            )?;
            stream.write_all(bytes)?;
            finish_entry(stream)
        }
        Ok(metadata) if metadata.is_dir() => {
            send_header(
                stream,
                EntryKind::Directory,
                relative,
                metadata.permissions().mode(),
                0,
            )?;
            finish_entry(stream)?;

            let mut children = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
            children.sort_by_key(|entry| entry.file_name());
            for child in children {
                send_current_entry(stream, root, &child.path())?;
            }
            Ok(())
        }
        Ok(metadata) if metadata.is_file() => {
            let snapshot = match read_stable_file(path) {
                Ok(snapshot) => snapshot,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::InvalidData | io::ErrorKind::WouldBlock
                    ) =>
                {
                    tracing::warn!(path = %path.display(), %error, "source sync: file skipped");
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    send_header(stream, EntryKind::Remove, relative, 0, 0)?;
                    return finish_entry(stream);
                }
                Err(error) => return Err(error),
            };
            send_header(
                stream,
                EntryKind::File,
                relative,
                snapshot.mode,
                snapshot.contents.len() as u64,
            )?;
            stream.write_all(&snapshot.contents)?;
            finish_entry(stream)
        }
        Ok(_) => {
            tracing::debug!(path = %path.display(), "source sync: unsupported file type skipped");
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            send_header(stream, EntryKind::Remove, relative, 0, 0)?;
            finish_entry(stream)
        }
        Err(error) => Err(error),
    }
}

fn read_stable_file(path: &Path) -> io::Result<FileSnapshot> {
    for attempt in 0..FILE_READ_ATTEMPTS {
        let mut file = File::open(path)?;
        let before = file.metadata()?;
        if before.len() > source_sync::MAX_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "source file is too large: {} bytes (maximum {})",
                    before.len(),
                    source_sync::MAX_FILE_BYTES
                ),
            ));
        }

        let capacity = usize::try_from(before.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "source file size does not fit in memory on this host",
            )
        })?;
        let mut contents = Vec::new();
        contents.try_reserve_exact(capacity).map_err(|error| {
            io::Error::other(format!(
                "could not reserve memory for source file {}: {error}",
                path.display()
            ))
        })?;
        let copied = (&mut file).take(before.len()).read_to_end(&mut contents)? as u64;
        let after = std::fs::symlink_metadata(path)?;

        if copied == before.len() && same_file_version(&before, &after) {
            return Ok(FileSnapshot {
                contents,
                mode: before.permissions().mode(),
            });
        }

        if attempt + 1 < FILE_READ_ATTEMPTS {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        format!(
            "source file {} kept changing while it was being synchronized",
            path.display()
        ),
    ))
}

fn same_file_version(before: &Metadata, after: &Metadata) -> bool {
    after.is_file()
        && before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
        && before.permissions().mode() == after.permissions().mode()
}

fn send_header(
    stream: &mut UnixStream,
    kind: EntryKind,
    path: &str,
    mode: u32,
    payload_len: u64,
) -> io::Result<()> {
    source_sync::write_header(
        stream,
        &EntryHeader {
            kind,
            path: path.to_string(),
            mode,
            payload_len,
        },
    )
}

fn finish_entry(stream: &mut UnixStream) -> io::Result<()> {
    stream.flush()?;
    source_sync::read_ack(stream)
}

fn relative_utf8<'a>(root: &Path, path: &'a Path) -> io::Result<&'a str> {
    path.strip_prefix(root)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path is outside source root"))?
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "source path is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_paths_are_collapsed_under_changed_directory() {
        let root = Path::new("/project");
        let paths = vec![
            root.join("src/lib.rs"),
            root.join("src"),
            root.join("src/nested/mod.rs"),
        ];

        assert_eq!(coalesce_paths(paths, root), vec![root.join("src")]);
    }

    #[test]
    fn ignored_dependency_paths_are_removed_from_burst() {
        let root = Path::new("/project");
        let paths = vec![
            root.join("node_modules/pkg/index.js"),
            root.join("src/index.js"),
        ];

        assert_eq!(coalesce_paths(paths, root), vec![root.join("src/index.js")]);
    }

    #[test]
    fn changed_file_is_framed_with_its_content() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let changed = source.join("main.rs");
        std::fs::write(&changed, b"fn main() {}\n").unwrap();
        let (mut host, mut guest) = UnixStream::pair().unwrap();

        let receiver = std::thread::spawn(move || {
            let header = source_sync::read_header(&mut guest).unwrap();
            let mut payload = vec![0u8; header.payload_len as usize];
            guest.read_exact(&mut payload).unwrap();
            source_sync::write_ack(&mut guest, Ok(())).unwrap();
            (header, payload)
        });

        send_current_entry(&mut host, &source, &changed).unwrap();
        let (header, payload) = receiver.join().unwrap();
        assert_eq!(header.path, "main.rs");
        assert_eq!(payload, b"fn main() {}\n");
    }

    #[test]
    fn oversized_file_is_rejected_before_a_frame_is_sent() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.bin");
        let file = File::create(&path).unwrap();
        file.set_len(source_sync::MAX_FILE_BYTES + 1).unwrap();

        let error = read_stable_file(&path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
