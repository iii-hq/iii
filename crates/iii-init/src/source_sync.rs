// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Guest-side receiver for host source changes.
//!
//! Entries are applied with ordinary guest filesystem operations. This is the
//! important part of the design: the guest kernel then emits native fsnotify /
//! inotify events, so the worker's own `watchfiles`, `node --watch`, `tsx`, or
//! `cargo watch` process decides how and when to restart.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use iii_supervisor::source_sync::{self, EntryHeader, EntryKind};

const MAX_SYMLINK_BYTES: u64 = 64 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn run(port_path: &Path, workspace: &Path, ready_path: &Path) -> io::Result<()> {
    while !ready_path.exists() {
        std::thread::sleep(Duration::from_millis(25));
    }

    fs::create_dir_all(workspace)?;
    let mut port = OpenOptions::new().read(true).write(true).open(port_path)?;
    loop {
        let header = read_header_when_ready(&mut port)?;

        match apply_entry(&mut port, workspace, &header) {
            Ok(()) => source_sync::write_ack(&mut port, Ok(()))?,
            Err(error) => {
                source_sync::write_ack(&mut port, Err(&error.to_string()))?;
                return Err(io::Error::new(
                    error.kind(),
                    format!("source-sync frame rejected: {error}"),
                ));
            }
        }
    }
}

fn read_header_when_ready(reader: &mut impl Read) -> io::Result<EntryHeader> {
    loop {
        match source_sync::read_header(&mut *reader) {
            Ok(header) => return Ok(header),
            // A virtio-console port can report a transient EOF before its host
            // backend is ready. Keep the guest receiver alive until the first
            // source event arrives instead of permanently disabling sync.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::WouldBlock
                ) =>
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error),
        }
    }
}

fn apply_entry(reader: &mut impl Read, workspace: &Path, header: &EntryHeader) -> io::Result<()> {
    let relative = source_sync::validate_relative_path(&header.path)?;
    let target = workspace.join(&relative);

    match header.kind {
        EntryKind::Remove => {
            reject_payload(header)?;
            ensure_safe_parent(workspace, &relative)?;
            remove_existing(&target)
        }
        EntryKind::Directory => {
            reject_payload(header)?;
            ensure_safe_parent(workspace, &relative)?;
            replace_with_directory(&target, header.mode)
        }
        EntryKind::File => apply_file(reader, workspace, &relative, header),
        EntryKind::Symlink => apply_symlink(reader, workspace, &relative, header),
    }
}

fn reject_payload(header: &EntryHeader) -> io::Result<()> {
    if header.payload_len == 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{:?} entry cannot carry a payload", header.kind),
        ))
    }
}

fn apply_file(
    reader: &mut impl Read,
    workspace: &Path,
    relative: &Path,
    header: &EntryHeader,
) -> io::Result<()> {
    if header.payload_len > source_sync::MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "source-sync file payload is too large: {} bytes (maximum {})",
                header.payload_len,
                source_sync::MAX_FILE_BYTES
            ),
        ));
    }
    ensure_safe_parent(workspace, relative)?;
    let target = workspace.join(relative);
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no parent"))?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("entry");
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".{file_name}.iii-sync-{}-{sequence}",
        std::process::id()
    ));

    let result = (|| {
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        let mut payload = reader.take(header.payload_len);
        let copied = io::copy(&mut payload, &mut output)?;
        if copied != header.payload_len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "source-sync file {} ended after {copied} of {} bytes",
                    header.path, header.payload_len
                ),
            ));
        }
        output.flush()?;
        output.set_permissions(fs::Permissions::from_mode(header.mode & 0o7777))?;
        drop(output);

        if target.is_dir() {
            fs::remove_dir_all(&target)?;
        } else if fs::symlink_metadata(&target).is_ok_and(|meta| meta.file_type().is_symlink()) {
            fs::remove_file(&target)?;
        }
        fs::rename(&temp, &target)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn apply_symlink(
    reader: &mut impl Read,
    workspace: &Path,
    relative: &Path,
    header: &EntryHeader,
) -> io::Result<()> {
    if header.payload_len > MAX_SYMLINK_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "source-sync symlink target is too long",
        ));
    }
    ensure_safe_parent(workspace, relative)?;
    let mut bytes = vec![0u8; header.payload_len as usize];
    reader.read_exact(&mut bytes)?;
    let target = workspace.join(relative);
    remove_existing(&target)?;
    symlink(OsString::from_vec(bytes), target)
}

fn ensure_safe_parent(workspace: &Path, relative: &Path) -> io::Result<()> {
    let Some(parent) = relative.parent() else {
        return Ok(());
    };
    let mut current = workspace.to_path_buf();
    for component in parent.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("source-sync parent is a symlink: {}", current.display()),
                ));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                remove_existing(&current)?;
                fs::create_dir(&current)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn replace_with_directory(path: &Path, mode: u32) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            remove_existing(path)?;
            fs::create_dir(path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(path)?,
        Err(error) => return Err(error),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o7777))
}

fn remove_existing(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)
        }
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TransientEof {
        returned_eof: bool,
        bytes: io::Cursor<Vec<u8>>,
    }

    impl Read for TransientEof {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.returned_eof {
                self.returned_eof = true;
                return Ok(0);
            }
            self.bytes.read(buffer)
        }
    }

    fn header(kind: EntryKind, path: &str, payload_len: u64) -> EntryHeader {
        EntryHeader {
            kind,
            path: path.to_string(),
            mode: 0o100644,
            payload_len,
        }
    }

    #[test]
    fn transient_console_eof_does_not_stop_receiver() {
        let expected = header(EntryKind::File, "src/main.py", 5);
        let mut bytes = Vec::new();
        source_sync::write_header(&mut bytes, &expected).unwrap();
        let mut reader = TransientEof {
            returned_eof: false,
            bytes: io::Cursor::new(bytes),
        };

        assert_eq!(read_header_when_ready(&mut reader).unwrap(), expected);
    }

    #[test]
    fn file_entry_replaces_content() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path();
        fs::create_dir(workspace.join("src")).unwrap();
        fs::write(workspace.join("src/main.py"), "before").unwrap();
        let mut payload = b"after".as_slice();

        apply_entry(
            &mut payload,
            workspace,
            &header(EntryKind::File, "src/main.py", 5),
        )
        .unwrap();

        assert_eq!(fs::read(workspace.join("src/main.py")).unwrap(), b"after");
    }

    #[test]
    fn remove_entry_deletes_directory_tree() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("removed/child");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("file"), "data").unwrap();
        let mut payload = io::empty();

        apply_entry(
            &mut payload,
            temp.path(),
            &header(EntryKind::Remove, "removed", 0),
        )
        .unwrap();

        assert!(!temp.path().join("removed").exists());
    }

    #[test]
    fn symlink_parent_cannot_escape_workspace() {
        let temp = tempfile::tempdir().unwrap();
        symlink("/tmp", temp.path().join("escape")).unwrap();
        let mut payload = b"blocked".as_slice();

        let error = apply_entry(
            &mut payload,
            temp.path(),
            &header(EntryKind::File, "escape/file", 7),
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn file_entry_emits_inotify_event() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let temp = tempfile::tempdir().unwrap();
        let watched = temp.path().join("src");
        fs::create_dir(&watched).unwrap();
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        assert!(
            fd >= 0,
            "inotify_init1 failed: {}",
            io::Error::last_os_error()
        );
        let watched_c = CString::new(watched.as_os_str().as_bytes()).unwrap();
        let wd = unsafe {
            libc::inotify_add_watch(
                fd,
                watched_c.as_ptr(),
                libc::IN_CREATE | libc::IN_MODIFY | libc::IN_MOVED_TO | libc::IN_CLOSE_WRITE,
            )
        };
        assert!(
            wd >= 0,
            "inotify_add_watch failed: {}",
            io::Error::last_os_error()
        );

        let mut payload = b"print('changed')\n".as_slice();
        let payload_len = payload.len() as u64;
        apply_entry(
            &mut payload,
            temp.path(),
            &header(EntryKind::File, "src/math_worker.py", payload_len),
        )
        .unwrap();

        let mut events = [0u8; 4096];
        let read = unsafe { libc::read(fd, events.as_mut_ptr().cast(), events.len()) };
        unsafe { libc::close(fd) };
        assert!(
            read > 0,
            "guest-side file write did not emit an inotify event"
        );
    }

    #[test]
    fn apply_file_rejects_payload_over_shared_limit() {
        let temp = tempfile::tempdir().unwrap();
        let mut payload = io::empty();

        let error = apply_entry(
            &mut payload,
            temp.path(),
            &header(
                EntryKind::File,
                "large.bin",
                source_sync::MAX_FILE_BYTES + 1,
            ),
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!temp.path().join("large.bin").exists());
    }
}
