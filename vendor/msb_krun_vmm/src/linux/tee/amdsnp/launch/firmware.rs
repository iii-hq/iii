// SPDX-License-Identifier: Apache-2.0

//! Operations for managing the SEV platform.

use std::{
    fs::{File, OpenOptions},
    os::fd::{AsRawFd, RawFd},
};

/// A handle to the SEV platform.
#[cfg(target_os = "linux")]
pub struct Firmware(File);

#[cfg(target_os = "linux")]
impl Firmware {
    /// Create a handle to the SEV platform.
    pub fn open() -> std::io::Result<Firmware> {
        Ok(Firmware(
            OpenOptions::new().read(true).write(true).open("/dev/sev")?,
        ))
    }
}

#[cfg(target_os = "linux")]
impl AsRawFd for Firmware {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}
