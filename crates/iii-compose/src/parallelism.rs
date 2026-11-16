// Copyright 2025 Motia LLC. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

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
    parse(value).unwrap_or(DEFAULT_MAX_PARALLEL_WORKERS)
}

/// A registry `/resolve` is one small request: a whole project's go out at
/// once, not eight at a time, unless the operator set the limit.
const DEFAULT_MAX_PARALLEL_RESOLVES: usize = 32;

pub(crate) fn max_parallel_resolves() -> usize {
    let value = std::env::var(MAX_PARALLEL_WORKERS_ENV).ok();
    parse_max_parallel_resolves(value.as_deref())
}

fn parse_max_parallel_resolves(value: Option<&str>) -> usize {
    parse(value).unwrap_or(DEFAULT_MAX_PARALLEL_RESOLVES)
}

fn parse(value: Option<&str>) -> Option<usize> {
    value
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_worker_limit_defaults_to_eight() {
        assert_eq!(parse_max_parallel_workers(None), 8);
    }

    #[test]
    fn resolves_default_to_thirty_two_and_follow_the_operator_limit() {
        assert_eq!(parse_max_parallel_resolves(None), 32);
        assert_eq!(parse_max_parallel_resolves(Some("2")), 2);
        assert_eq!(parse_max_parallel_resolves(Some("0")), 32);
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
