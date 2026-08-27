// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

/// Environment variable that limits concurrent package work and container starts.
const MAX_PARALLEL_WORKERS_ENV: &str = "III_COMPOSE_MAX_PARALLEL_WORKERS";

/// Keep large compose files from opening an unbounded number of downloads or
/// processes at once.
const DEFAULT_MAX_PARALLEL_WORKERS: usize = 8;

pub(crate) fn max_parallel_workers() -> usize {
    let value = std::env::var(MAX_PARALLEL_WORKERS_ENV).ok();
    parse_max_parallel_workers(value.as_deref())
}

fn parse_max_parallel_workers(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_PARALLEL_WORKERS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_worker_limit_defaults_to_eight() {
        assert_eq!(parse_max_parallel_workers(None), 8);
    }

    #[test]
    fn parallel_worker_limit_accepts_a_positive_environment_value() {
        assert_eq!(parse_max_parallel_workers(Some("16")), 16);
        assert_eq!(parse_max_parallel_workers(Some(" 4 ")), 4);
    }

    #[test]
    fn parallel_worker_limit_uses_the_default_for_invalid_values() {
        for value in ["", "0", "many", "-1"] {
            assert_eq!(
                parse_max_parallel_workers(Some(value)),
                DEFAULT_MAX_PARALLEL_WORKERS,
                "{value:?} must not disable concurrent work"
            );
        }
    }
}
