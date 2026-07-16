//! Cross-platform wake notification built on `pipe()`.
//!
//! Works on both Linux and macOS (unlike `eventfd` which is Linux-only).
//! The write end signals, the read end is pollable via `epoll`/`kqueue`/`poll`.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide count of `wake()` calls whose first write found the pipe
/// full. A rising value means some consumer is not draining its pipe —
/// the MOT-3966 stall signature.
static FAILED_WAKES: AtomicU64 = AtomicU64::new(0);

/// Number of `wake()` first-writes that failed because the pipe was full.
pub fn failed_wakes() -> u64 {
    FAILED_WAKES.load(Ordering::Relaxed)
}

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
    /// Writes a single byte. A dropped write is NOT safe to ignore: msb_krun's
    /// NetWorker registers the read end EDGE_TRIGGERED (kqueue `EV_CLEAR`) and
    /// never reads it, so a lost write means no event is ever delivered
    /// again — the MOT-3966 permanent host→guest stall. On failure we
    /// self-heal and retry, bounded so a persistent error can't spin:
    /// - full pipe (EAGAIN): drain our own read end (both ends live in this
    ///   process; concurrent drainers are harmless — only edges matter, not
    ///   bytes) so the next write lands and raises a fresh edge;
    /// - interrupted (EINTR): just write again.
    pub fn wake(&self) {
        for attempt in 0..3 {
            // SAFETY: write_fd is a valid, non-blocking file descriptor.
            // Writing 1 byte to a pipe is atomic on all POSIX systems.
            let n = unsafe { libc::write(self.write_fd.as_raw_fd(), [1u8].as_ptr().cast(), 1) };
            if n == 1 {
                return;
            }

            if attempt == 0 {
                let failed = FAILED_WAKES.fetch_add(1, Ordering::Relaxed) + 1;
                if failed == 1 || failed.is_multiple_of(1000) {
                    tracing::warn!(
                        failed,
                        "wake pipe write failed; self-healing (consumer not draining its pipe?)"
                    );
                }
            }

            let interrupted =
                n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted;
            if !interrupted {
                self.drain();
            }
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
            if n > 0 {
                continue;
            }
            // EINTR must retry: treating it as drained can strand bytes and,
            // for an edge-triggered peer, a permanently-readable stale state.
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
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

    /// MOT-3966: a full pipe must not swallow wakes — the consumer is
    /// edge-triggered, so a dropped write means no event is ever raised
    /// again. wake() must self-heal by draining and retrying.
    #[test]
    fn wake_self_heals_on_full_pipe() {
        let pipe = WakePipe::new();
        // Fill the pipe raw (bypassing wake's self-heal) until EAGAIN.
        let one = [1u8];
        loop {
            // SAFETY: valid nonblocking fd, 1-byte write.
            let n = unsafe { libc::write(pipe.write_fd.as_raw_fd(), one.as_ptr().cast(), 1) };
            if n != 1 {
                break;
            }
        }

        let before = failed_wakes();
        pipe.wake();
        assert!(
            failed_wakes() > before,
            "first write should have failed on a full pipe"
        );

        // Exactly the retried wake byte remains — the pipe was drained and
        // the wake landed, so a fresh edge was raised.
        let mut buf = [0u8; 4096];
        let mut total = 0isize;
        loop {
            // SAFETY: valid nonblocking fd reading into a local buffer.
            let n =
                unsafe { libc::read(pipe.read_fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            total += n;
        }
        assert_eq!(total, 1, "only the retried wake byte should remain");

        // And the pipe has room again.
        // SAFETY: as above.
        let n = unsafe { libc::write(pipe.write_fd.as_raw_fd(), one.as_ptr().cast(), 1) };
        assert_eq!(n, 1);
    }
}
