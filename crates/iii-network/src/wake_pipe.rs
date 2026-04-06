//! Cross-platform wake notification built on `pipe()`.
//!
//! Works on both Linux and macOS (unlike `eventfd` which is Linux-only).
//! The write end signals, the read end is pollable via `epoll`/`kqueue`/`poll`.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

/// Cross-platform wake notification built on `pipe()`.
///
/// The write end signals, the read end is pollable via `epoll`/`kqueue`/`poll`.
pub struct WakePipe {
    read_fd: OwnedFd,
    write_fd: OwnedFd,
}

impl WakePipe {
    /// Create a new wake pipe.
    ///
    /// Both ends are set to non-blocking and close-on-exec.
    pub fn new() -> Self {
        let mut fds = [0i32; 2];

        // SAFETY: pipe() is a standard POSIX call. We check the return value
        // and immediately wrap the raw fds in OwnedFd for RAII cleanup.
        let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert!(
            ret == 0,
            "pipe() failed: {}",
            std::io::Error::last_os_error()
        );

        // SAFETY: fds are valid open file descriptors from the pipe() call above.
        unsafe {
            set_nonblock_cloexec(fds[0]);
            set_nonblock_cloexec(fds[1]);
        }

        Self {
            // SAFETY: fds are valid and not owned by anything else yet.
            read_fd: unsafe { OwnedFd::from_raw_fd(fds[0]) },
            write_fd: unsafe { OwnedFd::from_raw_fd(fds[1]) },
        }
    }

    /// Signal the reader. Safe to call from any thread, multiple times.
    ///
    /// Writes a single byte. If the pipe buffer is full the write is silently
    /// dropped — the reader will still wake because there are unread bytes.
    pub fn wake(&self) {
        // SAFETY: write_fd is a valid, non-blocking file descriptor.
        // Writing 1 byte to a pipe is atomic on all POSIX systems.
        unsafe {
            libc::write(self.write_fd.as_raw_fd(), [1u8].as_ptr().cast(), 1);
        }
    }

    /// Drain all pending wake signals. Call after processing to reset the
    /// pipe for the next edge-triggered notification.
    pub fn drain(&self) {
        let mut buf = [0u8; 512];
        loop {
            // SAFETY: read_fd is a valid, non-blocking file descriptor.
            let n =
                unsafe { libc::read(self.read_fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
        }
    }

    /// File descriptor for `epoll`/`kqueue`/`poll(2)` registration.
    ///
    /// Becomes readable when [`wake()`](Self::wake) has been called.
    pub fn as_raw_fd(&self) -> RawFd {
        self.read_fd.as_raw_fd()
    }
}

impl Default for WakePipe {
    fn default() -> Self {
        Self::new()
    }
}

/// Set `O_NONBLOCK` and `FD_CLOEXEC` on a file descriptor.
///
/// # Safety
///
/// `fd` must be a valid, open file descriptor.
unsafe fn set_nonblock_cloexec(fd: RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        assert!(
            flags >= 0,
            "fcntl(F_GETFL) failed: {}",
            std::io::Error::last_os_error()
        );
        let ret = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        assert!(
            ret >= 0,
            "fcntl(F_SETFL) failed: {}",
            std::io::Error::last_os_error()
        );

        let flags = libc::fcntl(fd, libc::F_GETFD);
        assert!(
            flags >= 0,
            "fcntl(F_GETFD) failed: {}",
            std::io::Error::last_os_error()
        );
        let ret = libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        assert!(
            ret >= 0,
            "fcntl(F_SETFD) failed: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_and_drain() {
        let pipe = WakePipe::new();
        pipe.drain();

        pipe.wake();
        pipe.wake();
        pipe.drain();

        pipe.wake();
        pipe.drain();
    }

    #[test]
    fn fd_is_valid() {
        let pipe = WakePipe::new();
        let fd = pipe.as_raw_fd();
        assert!(fd >= 0);
    }

    #[test]
    fn nonblocking_read() {
        let pipe = WakePipe::new();
        pipe.drain();
    }
}
