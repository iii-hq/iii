//! Cross-platform signal handlers for graceful VMM shutdown.
//!
//! Registers `SIGTERM` and `SIGUSR1` handlers that write to the VMM's exit
//! event fd, causing the event loop to trigger a graceful shutdown with exit
//! observer invocation.
//!
//! Uses `libc::sigaction` directly (POSIX) instead of `vmm_sys_util::signal`
//! so it works on both Linux and macOS.

use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicI32, Ordering};
use std::{io, mem, ptr};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

static EXIT_EVT_FD: AtomicI32 = AtomicI32::new(-1);

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Signal handler for `SIGTERM` and `SIGUSR1`.
///
/// Writes to the VMM exit event fd so that the event loop triggers a graceful
/// shutdown (exit observers run before `_exit`). All operations are
/// async-signal-safe.
extern "C" fn exit_signal_handler(
    num: libc::c_int,
    info: *mut libc::siginfo_t,
    _unused: *mut libc::c_void,
) {
    let si_signo = unsafe { (*info).si_signo };

    if num != si_signo || (num != libc::SIGTERM && num != libc::SIGUSR1) {
        unsafe { libc::_exit(i32::from(super::FC_EXIT_CODE_UNEXPECTED_ERROR)) };
    }

    let val: u64 = 1;
    let exit_fd = EXIT_EVT_FD.load(Ordering::Relaxed);
    let _ = unsafe { libc::write(exit_fd, &val as *const _ as *const libc::c_void, 8) };
}

/// Registers `SIGTERM` and `SIGUSR1` handlers that write to the given fd,
/// causing a graceful VMM shutdown with exit observer invocation.
///
/// # Arguments
///
/// * `exit_write_fd` - The file descriptor to write to when a signal arrives.
///   On Linux this is the eventfd itself (`as_raw_fd()`). On macOS, where
///   `EventFd` is backed by a pipe, this must be the **write** end
///   (`get_write_fd()`).
pub fn register_exit_signal_handlers(exit_write_fd: RawFd) -> io::Result<()> {
    EXIT_EVT_FD.store(exit_write_fd, Ordering::Relaxed);

    unsafe {
        let mut sa: libc::sigaction = mem::zeroed();
        sa.sa_sigaction = exit_signal_handler as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO;

        if libc::sigaction(libc::SIGTERM, &sa, ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::sigaction(libc::SIGUSR1, &sa, ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}
