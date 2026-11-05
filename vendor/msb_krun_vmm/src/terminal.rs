use nix::sys::termios::{cfmakeraw, tcgetattr, tcsetattr, LocalFlags, SetArg, Termios};
use std::os::fd::BorrowedFd;

#[must_use]
pub struct TerminalMode(Termios);

// Enable raw mode for the terminal and return the old state to be restored
pub fn term_set_raw_mode(
    term: BorrowedFd,
    handle_signals_by_terminal: bool,
) -> Result<TerminalMode, nix::Error> {
    let mut termios = tcgetattr(term)?;
    let old_state = termios.clone();

    cfmakeraw(&mut termios);

    if handle_signals_by_terminal {
        termios.local_flags |= LocalFlags::ISIG;
    }

    tcsetattr(term, SetArg::TCSANOW, &termios)?;
    Ok(TerminalMode(old_state))
}

pub fn term_restore_mode(term: BorrowedFd, restore: &TerminalMode) -> Result<(), nix::Error> {
    tcsetattr(term, SetArg::TCSANOW, &restore.0)
}
