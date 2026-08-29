// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Wire protocol for forwarding host source changes into a worker VM.
//!
//! Virtiofs exposes current file contents to the guest, but host-side changes
//! do not produce guest `inotify` events. The host therefore sends the changed
//! entries over a dedicated virtio-console port. `iii-init` applies them to
//! `/workspace`; because those writes originate in the guest, normal file
//! watchers receive native kernel events.

use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

pub const SOURCE_SYNC_PORT_NAME: &str = "iii.source-sync";
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

const FRAME_MAGIC: [u8; 4] = *b"IIIS";
const ACK_MAGIC: [u8; 4] = *b"IIIA";
const PROTOCOL_VERSION: u8 = 1;
const MAX_PATH_BYTES: usize = 64 * 1024;
const MAX_ACK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EntryKind {
    Remove = 1,
    Directory = 2,
    File = 3,
    Symlink = 4,
}

impl TryFrom<u8> for EntryKind {
    type Error = io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Remove),
            2 => Ok(Self::Directory),
            3 => Ok(Self::File),
            4 => Ok(Self::Symlink),
            _ => Err(invalid_data(format!(
                "unknown source-sync entry kind {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryHeader {
    pub kind: EntryKind,
    pub path: String,
    pub mode: u32,
    pub payload_len: u64,
}

pub fn write_header(mut writer: impl Write, header: &EntryHeader) -> io::Result<()> {
    validate_relative_path(&header.path)?;
    if header.kind == EntryKind::File && header.payload_len > MAX_FILE_BYTES {
        return Err(invalid_input(format!(
            "source-sync file payload is too large: {} bytes (maximum {MAX_FILE_BYTES})",
            header.payload_len
        )));
    }
    let path = header.path.as_bytes();
    if path.len() > MAX_PATH_BYTES {
        return Err(invalid_input(format!(
            "source-sync path is too long: {} bytes",
            path.len()
        )));
    }

    writer.write_all(&FRAME_MAGIC)?;
    writer.write_all(&[PROTOCOL_VERSION, header.kind as u8, 0, 0])?;
    writer.write_all(&(path.len() as u32).to_be_bytes())?;
    writer.write_all(&header.mode.to_be_bytes())?;
    writer.write_all(&header.payload_len.to_be_bytes())?;
    writer.write_all(path)
}

pub fn read_header(mut reader: impl Read) -> io::Result<EntryHeader> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if magic != FRAME_MAGIC {
        return Err(invalid_data("invalid source-sync frame magic"));
    }

    let mut fixed = [0u8; 20];
    reader.read_exact(&mut fixed)?;
    if fixed[0] != PROTOCOL_VERSION {
        return Err(invalid_data(format!(
            "unsupported source-sync protocol version {}",
            fixed[0]
        )));
    }
    let kind = EntryKind::try_from(fixed[1])?;
    let path_len = u32::from_be_bytes(fixed[4..8].try_into().expect("fixed header slice")) as usize;
    if path_len == 0 || path_len > MAX_PATH_BYTES {
        return Err(invalid_data(format!(
            "source-sync path length {path_len} is out of range"
        )));
    }
    let mode = u32::from_be_bytes(fixed[8..12].try_into().expect("fixed header slice"));
    let payload_len = u64::from_be_bytes(fixed[12..20].try_into().expect("fixed header slice"));
    if kind == EntryKind::File && payload_len > MAX_FILE_BYTES {
        return Err(invalid_data(format!(
            "source-sync file payload is too large: {payload_len} bytes (maximum {MAX_FILE_BYTES})"
        )));
    }

    let mut path = vec![0u8; path_len];
    reader.read_exact(&mut path)?;
    let path =
        String::from_utf8(path).map_err(|_| invalid_data("source-sync path is not UTF-8"))?;
    validate_relative_path(&path)?;

    Ok(EntryHeader {
        kind,
        path,
        mode,
        payload_len,
    })
}

pub fn write_ack(mut writer: impl Write, result: Result<(), &str>) -> io::Result<()> {
    let (status, message) = match result {
        Ok(()) => (0u8, ""),
        Err(message) => (1u8, message),
    };
    let bytes = message.as_bytes();
    if bytes.len() > MAX_ACK_BYTES {
        return Err(invalid_input("source-sync acknowledgement is too long"));
    }
    writer.write_all(&ACK_MAGIC)?;
    writer.write_all(&[status])?;
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()
}

pub fn read_ack(mut reader: impl Read) -> io::Result<()> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if magic != ACK_MAGIC {
        return Err(invalid_data("invalid source-sync acknowledgement magic"));
    }
    let mut fixed = [0u8; 5];
    reader.read_exact(&mut fixed)?;
    let status = fixed[0];
    let len =
        u32::from_be_bytes(fixed[1..5].try_into().expect("fixed acknowledgement slice")) as usize;
    if len > MAX_ACK_BYTES {
        return Err(invalid_data(format!(
            "source-sync acknowledgement length {len} is out of range"
        )));
    }
    let mut message = vec![0u8; len];
    reader.read_exact(&mut message)?;
    if status == 0 {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&message);
    Err(io::Error::other(format!(
        "guest rejected source change: {message}"
    )))
}

pub fn validate_relative_path(value: &str) -> io::Result<PathBuf> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() {
        return Err(invalid_input("source-sync path must be relative"));
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid_input(format!(
            "source-sync path contains an unsafe component: {value}"
        )));
    }
    Ok(path.to_path_buf())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrips() {
        let expected = EntryHeader {
            kind: EntryKind::File,
            path: "src/main.rs".to_string(),
            mode: 0o100644,
            payload_len: 123,
        };
        let mut bytes = Vec::new();
        write_header(&mut bytes, &expected).unwrap();

        assert_eq!(read_header(bytes.as_slice()).unwrap(), expected);
    }

    #[test]
    fn parent_component_is_rejected() {
        let error = validate_relative_path("src/../../etc/passwd").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn error_ack_is_reported_to_host() {
        let mut bytes = Vec::new();
        write_ack(&mut bytes, Err("cannot replace target")).unwrap();

        let error = read_ack(bytes.as_slice()).unwrap_err();
        assert!(error.to_string().contains("cannot replace target"));
    }

    #[test]
    fn oversized_file_payload_is_rejected_when_writing() {
        let header = EntryHeader {
            kind: EntryKind::File,
            path: "large.bin".to_string(),
            mode: 0o100644,
            payload_len: MAX_FILE_BYTES + 1,
        };

        let error = write_header(Vec::new(), &header).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn oversized_file_payload_is_rejected_when_reading() {
        let header = EntryHeader {
            kind: EntryKind::File,
            path: "large.bin".to_string(),
            mode: 0o100644,
            payload_len: MAX_FILE_BYTES,
        };
        let mut bytes = Vec::new();
        write_header(&mut bytes, &header).unwrap();
        bytes[16..24].copy_from_slice(&(MAX_FILE_BYTES + 1).to_be_bytes());

        let error = read_header(bytes.as_slice()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
