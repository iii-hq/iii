// Copyright 2018 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicI32, Ordering};

use libc::{_exit, c_int, c_void, siginfo_t, SIGBUS, SIGINT, SIGSEGV, SIGSYS, SIGWINCH};
use utils::signal::register_signal_handler;

// The offset of `si_syscall` (offending syscall identifier) within the siginfo structure
// expressed as an `(u)int*`.
// Offset `6` for an `i32` field means that the needed information is located at `6 * sizeof(i32)`.
// See /usr/include/linux/signal.h for the C struct definition.
// See https://github.com/rust-lang/libc/issues/716 for why the offset is different in Rust.
const SI_OFF_SYSCALL: isize = 6;

const SYS_SECCOMP_CODE: i32 = 1;

static CONSOLE_SIGWINCH_FD: AtomicI32 = AtomicI32::new(-1);
static CONSOLE_SIGINT_FD: AtomicI32 = AtomicI32::new(-1);

/// Signal handler for `SIGSYS`.
///
/// Increments the `seccomp.num_faults` metric, logs an error message and terminates the process
/// with a specific exit code.
extern "C" fn sigsys_handler(num: c_int, info: *mut siginfo_t, _unused: *mut c_void) {
    // Safe because we're just reading some fields from a supposedly valid argument.
    let si_signo = unsafe { (*info).si_signo };
    let si_code = unsafe { (*info).si_code };

    // Sanity check. The condition should never be true.
    if num != si_signo || num != SIGSYS || si_code != SYS_SECCOMP_CODE {
        // Safe because we're terminating the process anyway.
        unsafe { _exit(i32::from(super::FC_EXIT_CODE_UNEXPECTED_ERROR)) };
    }

    // Other signals which might do async unsafe things incompatible with the rest of this
    // function are blocked due to the sa_mask used when registering the signal handler.
    let syscall = unsafe { *(info as *const i32).offset(SI_OFF_SYSCALL) as usize };
    error!("Shutting down VM after intercepting a bad syscall ({syscall}).");
    // Safe because we're terminating the process anyway. We don't actually do anything when
    // running unit tests.
    #[cfg(not(test))]
    unsafe {
        _exit(i32::from(super::FC_EXIT_CODE_BAD_SYSCALL))
    };
}

/// Signal handler for `SIGBUS` and `SIGSEGV`.
///
/// Logs an error message and terminates the process with a specific exit code.
extern "C" fn sigbus_sigsegv_handler(num: c_int, info: *mut siginfo_t, _unused: *mut c_void) {
    // Safe because we're just reading some fields from a supposedly valid argument.
    let si_signo = unsafe { (*info).si_signo };
    let si_code = unsafe { (*info).si_code };

    // Sanity check. The condition should never be true.
    if num != si_signo || (num != SIGBUS && num != SIGSEGV) {
        // Safe because we're terminating the process anyway.
        unsafe { _exit(i32::from(super::FC_EXIT_CODE_UNEXPECTED_ERROR)) };
    }

    error!("Shutting down VM after intercepting signal {si_signo}, code {si_code}.");

    // Safe because we're terminating the process anyway. We don't actually do anything when
    // running unit tests.
    #[cfg(not(test))]
    unsafe {
        _exit(i32::from(match si_signo {
            SIGBUS => super::FC_EXIT_CODE_SIGBUS,
            SIGSEGV => super::FC_EXIT_CODE_SIGSEGV,
            _ => super::FC_EXIT_CODE_UNEXPECTED_ERROR,
        }))
    };
}

extern "C" fn sigwinch_handler(num: c_int, info: *mut siginfo_t, _unused: *mut c_void) {
    // Safe because we're just reading some fields from a supposedly valid argument.
    let si_signo = unsafe { (*info).si_signo };

    // Sanity check. The condition should never be true.
    if num != si_signo || num != SIGWINCH {
        // Safe because we're terminating the process anyway.
        unsafe { _exit(i32::from(super::FC_EXIT_CODE_UNEXPECTED_ERROR)) };
    }

    let val: u64 = 1;
    let console_fd = CONSOLE_SIGWINCH_FD.load(Ordering::Relaxed);
    let _ = unsafe { libc::write(console_fd, &val as *const _ as *const c_void, 8) };
}

extern "C" fn sigint_handler(num: c_int, info: *mut siginfo_t, _unused: *mut c_void) {
    // Safe because we're just reading some fields from a supposedly valid argument.
    let si_signo = unsafe { (*info).si_signo };

    // Sanity check. The condition should never be true.
    if num != si_signo || num != SIGINT {
        // Safe because we're terminating the process anyway.
        unsafe { _exit(i32::from(super::FC_EXIT_CODE_UNEXPECTED_ERROR)) };
    }

    let val: u64 = 1;
    let console_fd = CONSOLE_SIGINT_FD.load(Ordering::Relaxed);
    let _ = unsafe { libc::write(console_fd, &val as *const _ as *const c_void, 8) };
}

pub fn register_sigwinch_handler(console_fd: RawFd) -> utils::errno::Result<()> {
    CONSOLE_SIGWINCH_FD.store(console_fd, Ordering::Relaxed);

    register_signal_handler(SIGWINCH, sigwinch_handler)?;

    Ok(())
}

pub fn register_sigint_handler(sigint_fd: RawFd) -> utils::errno::Result<()> {
    CONSOLE_SIGINT_FD.store(sigint_fd, Ordering::Relaxed);

    register_signal_handler(SIGINT, sigint_handler)?;

    Ok(())
}

/// Registers all the required signal handlers.
///
/// Custom handlers are installed for: `SIGBUS`, `SIGSEGV`, `SIGSYS`.
pub fn register_signal_handlers() -> utils::errno::Result<()> {
    // Call to unsafe register_signal_handler which is considered unsafe because it will
    // register a signal handler which will be called in the current thread and will interrupt
    // whatever work is done on the current thread, so we have to keep in mind that the registered
    // signal handler must only do async-signal-safe operations.
    register_signal_handler(SIGSYS, sigsys_handler)?;
    register_signal_handler(SIGBUS, sigbus_sigsegv_handler)?;
    register_signal_handler(SIGSEGV, sigbus_sigsegv_handler)?;

    Ok(())
}
