// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.

//! Steady-tick spinner for silent CLI phases.
//!
//! Several `iii worker` flows do tens of seconds of synchronous work
//! between status prints (image pull, archive download, extract, VM
//! boot). Without a tick the user sees a frozen terminal and assumes
//! the command is hung. This wraps `indicatif::ProgressBar` with the
//! one-line spinner aesthetic the rest of the CLI uses (`  ⠋ ...`),
//! collapses to a no-op when stderr is not a tty (so daemon/test
//! output stays log-friendly), and prints a single final-status line
//! with elapsed time so the user can see each phase succeeded.

use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use std::time::{Duration, Instant};

/// Steady-tick spinner. Construct with [`Spinner::start`], optionally
/// update the visible label with [`Spinner::set_message`], then call
/// [`Spinner::finish_ok`] / [`Spinner::finish_err`] when the wrapped
/// work resolves. `Drop` is a safety net only — finish explicitly so
/// the elapsed-time footer is printed.
pub struct Spinner {
    pb: Option<ProgressBar>,
    started: Instant,
}

impl Spinner {
    /// Begin a spinner labeled `message`. Hidden (`Drop`-only) when
    /// stderr is not a tty so non-interactive callers (daemons, CI
    /// logs, captured stderr) don't see ANSI noise.
    pub fn start(message: impl Into<String>) -> Self {
        let message = message.into();
        let active = std::io::stderr().is_terminal();
        let pb = if active {
            let pb = ProgressBar::new_spinner();
            // Match the rest of the CLI: two-space gutter + cyan glyph.
            pb.set_style(
                ProgressStyle::with_template("  {spinner:.cyan} {msg}")
                    .expect("static template")
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
            );
            pb.set_message(message);
            pb.enable_steady_tick(Duration::from_millis(100));
            Some(pb)
        } else {
            None
        };
        Self {
            pb,
            started: Instant::now(),
        }
    }

    /// Update the visible label without resetting the timer.
    pub fn set_message(&self, message: impl Into<String>) {
        if let Some(pb) = &self.pb {
            pb.set_message(message.into());
        }
    }

    /// Stop the spinner and print a single line with a green check,
    /// the final message, and elapsed seconds.
    pub fn finish_ok(mut self, final_message: impl AsRef<str>) {
        if let Some(pb) = self.pb.take() {
            pb.finish_and_clear();
        }
        let elapsed = self.started.elapsed();
        eprintln!(
            "  {} {} {}",
            "✓".green(),
            final_message.as_ref(),
            format!("({:.1}s)", elapsed.as_secs_f64()).dimmed(),
        );
    }

    /// Stop the spinner and print a single red-cross line. Use when
    /// the wrapped operation returned an error so the spinner doesn't
    /// stay on screen above the error message.
    pub fn finish_err(mut self, final_message: impl AsRef<str>) {
        if let Some(pb) = self.pb.take() {
            pb.finish_and_clear();
        }
        eprintln!("  {} {}", "✗".red(), final_message.as_ref());
    }

    /// Stop the spinner without printing anything. Use when the caller
    /// has its own follow-up output (e.g., a detailed multi-line
    /// success block) and the spinner would just add noise.
    pub fn finish_silent(mut self) {
        if let Some(pb) = self.pb.take() {
            pb.finish_and_clear();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        // Safety net: if the caller forgot to finish explicitly, at
        // least clear the spinner so it doesn't keep ticking after the
        // owning future is dropped.
        if let Some(pb) = self.pb.take() {
            pb.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In test mode the test runner captures stderr (or pipes it), so
    /// `is_terminal()` returns false and `Spinner::start` takes the
    /// hidden path with `pb: None`. The finish_* methods must not
    /// panic on the None branch — they all unconditionally call
    /// `take()`, which is safe (returns `None`), but the eprintln
    /// fallback still has to be writeable.
    #[test]
    fn finish_ok_on_hidden_spinner_does_not_panic() {
        let s = Spinner::start("test op");
        s.finish_ok("done");
    }

    #[test]
    fn finish_err_on_hidden_spinner_does_not_panic() {
        let s = Spinner::start("test op");
        s.finish_err("failed");
    }

    #[test]
    fn finish_silent_on_hidden_spinner_does_not_panic() {
        let s = Spinner::start("test op");
        s.finish_silent();
    }

    /// Regression: dropping the Spinner without calling any finish_*
    /// method must run the safety net without panic. Catches a future
    /// refactor that accidentally double-takes the ProgressBar.
    #[test]
    fn drop_without_finish_does_not_panic() {
        let _s = Spinner::start("orphan op");
        // _s goes out of scope here, triggering Drop.
    }

    /// `set_message` on the hidden (non-tty) path must be a no-op
    /// rather than a panic — callers don't condition on tty state.
    #[test]
    fn set_message_on_hidden_spinner_does_not_panic() {
        let s = Spinner::start("test op");
        s.set_message("updated");
        s.finish_silent();
    }
}
