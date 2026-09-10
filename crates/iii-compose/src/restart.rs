// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Exponential backoff shared by initial starts and supervised replacements.

use std::time::Duration;

use crate::config::RestartConfig;

/// Backoff before the attempt after `spent` attempts have failed.
pub(crate) fn backoff(config: &RestartConfig, spent: u32) -> Duration {
    config
        .delay
        .saturating_mul(2u32.saturating_pow(spent.saturating_sub(1)))
        .min(config.max_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_then_holds_at_the_ceiling() {
        let config = RestartConfig::default();

        assert_eq!(backoff(&config, 1), config.delay);
        assert_eq!(backoff(&config, 2), config.delay * 2);
        assert_eq!(backoff(&config, 3), config.delay * 4);
        assert_eq!(
            backoff(&config, 30),
            config.max_delay,
            "a long-running loop must not overflow into an unbounded wait"
        );
    }

    #[test]
    fn the_budget_is_spent_in_bounded_time() {
        let config = RestartConfig::default();
        let total: Duration = (1..=config.max_attempts)
            .map(|spent| backoff(&config, spent))
            .sum();
        assert!(
            total <= config.max_delay * config.max_attempts,
            "every attempt waits at most the ceiling"
        );
        assert!(
            total >= config.delay,
            "attempts after the first always wait"
        );
    }

    #[test]
    fn backoff_uses_the_container_limits() {
        let config = RestartConfig {
            delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(3),
            ..RestartConfig::default()
        };

        assert_eq!(backoff(&config, 2), Duration::from_secs(3));
    }
}
