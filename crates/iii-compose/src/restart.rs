// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Retry limits shared by initial starts and supervised replacements.

use std::time::Duration;

/// Wait before the second restart attempt. Each further attempt doubles it.
pub(crate) const BACKOFF_BASE: Duration = Duration::from_millis(500);

/// Longest wait between two restart attempts.
pub(crate) const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Replacement attempts available after a failed start or unexpected exit.
pub(crate) const MAX_ATTEMPTS: u32 = 5;

/// Time a container must hold `Ready` before the run-time budget refills.
pub(crate) const BUDGET_RESET_AFTER: Duration = Duration::from_secs(60);

/// Backoff before the attempt after `spent` attempts have failed.
pub(crate) fn backoff(spent: u32) -> Duration {
    BACKOFF_BASE
        .saturating_mul(2u32.saturating_pow(spent.saturating_sub(1)))
        .min(BACKOFF_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_then_holds_at_the_ceiling() {
        assert_eq!(backoff(1), BACKOFF_BASE);
        assert_eq!(backoff(2), BACKOFF_BASE * 2);
        assert_eq!(backoff(3), BACKOFF_BASE * 4);
        assert_eq!(
            backoff(30),
            BACKOFF_MAX,
            "a long-running loop must not overflow into an unbounded wait"
        );
    }

    #[test]
    fn the_budget_is_spent_in_bounded_time() {
        let total: Duration = (1..=MAX_ATTEMPTS).map(backoff).sum();
        assert!(
            total <= BACKOFF_MAX * MAX_ATTEMPTS,
            "every attempt waits at most the ceiling"
        );
        assert!(
            total >= BACKOFF_BASE,
            "attempts after the first always wait"
        );
    }
}
