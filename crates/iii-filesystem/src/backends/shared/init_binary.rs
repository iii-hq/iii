//! Virtual init.krun file serving the embedded init binary.
//!
//! The init binary appears at the root of every filesystem backend as
//! `/init.krun` (inode `ROOT_ID + 1`, handle `0`). It is read-only,
//! cannot be deleted or modified, and is immune to whiteouts.
//!
//! ## Storage
//!
//! The binary is stored in a memfd (Linux) or tmpfile (macOS) created at init time.
//! Reads use `ZeroCopyWriter::write_from` for zero-copy transfer from the backing file
//! to the FUSE response buffer, avoiding intermediate copies of the binary data.

use std::{fs::File, io, time::Duration};

use crate::{Entry, ZeroCopyWriter, init::INIT_BYTES, stat64};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// The filename of the virtual init binary as it appears in the guest.
pub(crate) const INIT_FILENAME: &[u8] = b"init.krun";

/// Reserved FUSE inode for the init binary (ROOT_ID + 1 = 2).
pub(crate) const INIT_INODE: u64 = 2;

/// Reserved FUSE handle for init binary reads.
pub(crate) const INIT_HANDLE: u64 = 0;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Build a synthetic `stat64` for the init binary.
pub(crate) fn init_stat() -> stat64 {
    let mut st: stat64 = unsafe { std::mem::zeroed() };

    #[cfg(target_os = "linux")]
    {
        st.st_ino = INIT_INODE;
        st.st_nlink = 1;
        st.st_mode = super::platform::MODE_REG | 0o755;
        st.st_uid = 0;
        st.st_gid = 0;
        st.st_size = INIT_BYTES.len() as i64;
        st.st_blocks = ((INIT_BYTES.len() as i64) + 511) / 512;
        st.st_blksize = 4096;
    }

    #[cfg(target_os = "macos")]
    {
        st.st_ino = INIT_INODE;
        st.st_nlink = 1;
        st.st_mode = libc::S_IFREG | 0o755;
        st.st_uid = 0;
        st.st_gid = 0;
        st.st_size = INIT_BYTES.len() as i64;
        st.st_blocks = ((INIT_BYTES.len() as i64) + 511) / 512;
        st.st_blksize = 4096;
    }

    st
}

/// Build a FUSE `Entry` for the init binary.
pub(crate) fn init_entry(entry_timeout: Duration, attr_timeout: Duration) -> Entry {
    Entry {
        inode: INIT_INODE,
        generation: 0,
        attr: init_stat(),
        attr_flags: 0,
        attr_timeout,
        entry_timeout,
    }
}

/// Create a `File` backed by a memfd (Linux) or tmpfile (macOS) containing INIT_BYTES.
///
/// This file is stored in `PassthroughFs` and used by `read_init` via `write_from`.
/// If `INIT_BYTES` is empty (feature disabled or binary unavailable), an empty
/// memfd/tmpfile is created for graceful degradation.
pub(crate) fn create_init_file() -> io::Result<File> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::FromRawFd;

        let name = std::ffi::CString::new("init.krun").unwrap();
        let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let data = INIT_BYTES;
        if !data.is_empty() {
            let written =
                unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
            if written < 0 {
                let err = io::Error::last_os_error();
                unsafe { libc::close(fd) };
                return Err(err);
            }
            if (written as usize) != data.len() {
                unsafe { libc::close(fd) };
                return Err(super::platform::eio());
            }
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        let mut file = tempfile::tempfile()?;
        if !INIT_BYTES.is_empty() {
            file.write_all(INIT_BYTES)?;
        }
        Ok(file)
    }
}

/// Handle a read request for the virtual init binary.
///
/// Uses `write_from` with the pre-created init file to transfer bytes
/// via the zero-copy FUSE buffer path.
pub(crate) fn read_init(
    w: &mut dyn ZeroCopyWriter,
    init_file: &File,
    size: u32,
    offset: u64,
) -> io::Result<usize> {
    let data_len = INIT_BYTES.len() as u64;

    if offset >= data_len {
        return Ok(0);
    }

    let count = std::cmp::min(size as u64, data_len - offset) as usize;
    w.write_from(init_file, count, offset)
}

/// Check if a name matches the init binary filename.
pub(crate) fn is_init_name(name: &[u8]) -> bool {
    name == INIT_FILENAME
}
