// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The `iii_engine_version` range a worker manifest declares, checked against
//! the engine hosting Compose.
//!
//! The grammar is the node-semver subset the registry and the workers CI also
//! accept: `OP? MAJOR.MINOR.PATCH` comparators (`>=`, `>`, `<=`, `<`, `=`, `^`,
//! `~`), space-joined into sets (all must hold) and `||`-joined into a range
//! (any set may hold). A bare version means exactly that version.

use semver::{Op, Version, VersionReq};

const OPERATORS: [&str; 7] = [">=", "<=", ">", "<", "=", "^", "~"];

/// The refusal for a value outside the supported range grammar.
pub fn malformed_iii_engine_version(got: impl std::fmt::Debug) -> String {
    format!(
        "iii_engine_version must be a semver range like \">=0.26.0\" (comparators >=, >, <=, <, =, ^, ~ on MAJOR.MINOR.PATCH, joined by spaces or ||), got {got:?}"
    )
}

pub fn valid_iii_engine_version(range: &str) -> bool {
    parse_range(range).is_some()
}

/// Compatible when the engine's `MAJOR.MINOR.PATCH` satisfies any `||` set, so
/// `0.26.0-rc.2` counts as `0.26.0`.
pub fn check_iii_engine_requirement(
    worker: &str,
    range: &str,
    running: &Version,
) -> Result<(), String> {
    let worker = sanitize(worker);
    let Some(sets) = parse_range(range) else {
        return Err(format!("{worker}: {}", malformed_iii_engine_version(range)));
    };
    let base = Version::new(running.major, running.minor, running.patch);
    if sets.iter().any(|set| set.matches(&base)) {
        return Ok(());
    }
    let tail = if update_can_help(&sets, &base) {
        "run: iii update".to_string()
    } else {
        format!(
            "this iii is newer than {worker} supports \u{2014} install a version of {worker} that supports iii {running}"
        )
    };
    Err(format!(
        "{worker} requires iii {range}, running {running}; {tail}"
    ))
}

/// The engine version Compose checks manifests against: the binary hosting
/// it, which is the engine that runs the workers. `None` outside an engine.
pub fn running_engine_version() -> Option<Version> {
    crate::host_version().and_then(|raw| Version::parse(raw.trim().trim_start_matches('v')).ok())
}

/// One `VersionReq` per `||` set, or `None` when `range` is outside the grammar.
/// `VersionReq` reads a bare `1.2.3` as `^1.2.3` and joins comparators with
/// commas, so a bare version gets `=` and each set is comma-joined.
fn parse_range(range: &str) -> Option<Vec<VersionReq>> {
    if range.is_empty() || range.starts_with(is_space) || range.ends_with(is_space) {
        return None;
    }
    range
        .split("||")
        .map(|set| {
            let set = set.trim_matches(is_space);
            if set.is_empty() || set.chars().any(|c| c.is_whitespace() && !is_space(c)) {
                return None;
            }
            let comparators = set
                .split(is_space)
                .filter(|c| !c.is_empty())
                .map(comparator)
                .collect::<Option<Vec<_>>>()?;
            VersionReq::parse(&comparators.join(", ")).ok()
        })
        .collect()
}

/// `OP? MAJOR.MINOR.PATCH` with no leading zeros, normalised for `VersionReq`.
fn comparator(text: &str) -> Option<String> {
    let op = OPERATORS
        .iter()
        .find(|op| text.starts_with(*op))
        .copied()
        .unwrap_or("");
    let version = &text[op.len()..];
    let parts: Vec<&str> = version.split('.').collect();
    let numeric = |part: &&str| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && (*part == "0" || !part.starts_with('0'))
    };
    if parts.len() != 3 || !parts.iter().all(numeric) {
        return None;
    }
    Some(format!("{}{version}", if op.is_empty() { "=" } else { op }))
}

fn is_space(c: char) -> bool {
    c.is_ascii_whitespace()
}

/// True when some `||` set fails only because the engine is too old for it.
fn update_can_help(sets: &[VersionReq], base: &Version) -> bool {
    sets.iter().any(|set| {
        set.comparators.iter().all(|c| {
            let single = VersionReq {
                comparators: vec![c.clone()],
            };
            single.matches(base) || {
                let floor = Version::new(c.major, c.minor.unwrap_or(0), c.patch.unwrap_or(0));
                match c.op {
                    Op::Greater | Op::GreaterEq => true,
                    Op::Less | Op::LessEq => false,
                    _ => *base < floor,
                }
            }
        })
    })
}

/// Drops terminal control and bidi-override characters so a worker name can't
/// rewrite the error line it appears in.
fn sanitize(name: &str) -> String {
    name.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn ok(range: &str, running: &str) -> bool {
        check_iii_engine_requirement("http", range, &v(running)).is_ok()
    }

    #[test]
    fn rc_engine_satisfies_release_floor() {
        assert!(ok(">=0.26.0", "0.26.0-rc.2"));
    }

    #[test]
    fn older_engine_is_refused_with_update_hint() {
        assert_eq!(
            check_iii_engine_requirement("http", ">=0.26.0", &v("0.25.9")).unwrap_err(),
            "http requires iii >=0.26.0, running 0.25.9; run: iii update"
        );
    }

    #[test]
    fn too_new_engine_gets_a_different_hint() {
        let msg =
            |r: &str, run: &str| check_iii_engine_requirement("http", r, &v(run)).unwrap_err();
        for (r, run) in [("<0.28.0", "0.28.1"), ("^0.26.0", "0.27.0")] {
            let e = msg(r, run);
            assert!(
                e.contains("newer than http supports") && !e.contains("iii update"),
                "{e}"
            );
        }
        for (r, run) in [("^0.26.0", "0.25.0"), ("^0.25.0 || ^0.27.0", "0.26.0")] {
            assert!(msg(r, run).ends_with("run: iii update"), "{r}");
        }
    }

    #[test]
    fn caret_range_refuses_the_next_minor() {
        assert!(ok("^0.26.0", "0.26.4"));
        assert!(!ok("^0.26.0", "0.27.0"));
    }

    #[test]
    fn space_joined_comparators_must_all_hold() {
        assert!(ok(">=0.26.0 <0.28.0", "0.27.3"));
        assert!(!ok(">=0.26.0 <0.28.0", "0.28.0"));
    }

    #[test]
    fn any_or_set_may_hold() {
        assert!(ok("^0.25.0 || ^0.26.0", "0.25.4"));
        assert!(ok("^0.25.0 || ^0.26.0", "0.26.1"));
        assert!(!ok("^0.25.0 || ^0.26.0", "0.27.0"));
    }

    #[test]
    fn bare_version_means_exactly_that_version() {
        assert!(ok("0.26.0", "0.26.0"));
        assert!(ok("0.26.0", "0.26.0-rc.1"));
        assert!(!ok("0.26.0", "0.26.1"));
    }

    #[test]
    fn valid_iii_engine_version_follows_the_range_grammar() {
        for good in [
            ">=0.26.0",
            "^0.26.0",
            "~0.26.0",
            "=0.26.0",
            "0.26.0",
            ">=0.26.0 <0.28.0",
            "^0.25.0 || ^0.26.0",
            "^0.25.0||^0.26.0",
        ] {
            assert!(valid_iii_engine_version(good), "{good}");
        }
        for bad in [
            "",
            "0.26",
            "v0.26.0",
            ">=0.26.0-rc.1",
            ">=01.2.3",
            "0.26.x",
            "1.0.0 - 2.0.0",
            " >=0.26.0",
            ">=0.26.0 ",
            ">=0.26.0 ||",
            ">=0.26.0<0.28.0",
            "=>0.26.0",
            ">=0.26.0\u{a0}<0.28.0",
            ">=0.2\u{666}.0",
        ] {
            assert!(!valid_iii_engine_version(bad), "{bad:?}");
        }
    }

    #[test]
    fn malformed_range_is_refused() {
        let err = check_iii_engine_requirement("http", "0.26", &v("0.26.0")).unwrap_err();
        assert!(
            err.starts_with("http: iii_engine_version must be a semver range"),
            "{err}"
        );
    }

    #[test]
    fn worker_name_is_sanitized_in_errors() {
        let err = check_iii_engine_requirement("ht\x1b[31mtp\u{202E}", ">=0.26.0", &v("0.25.9"))
            .unwrap_err();
        assert!(
            !err.contains('\x1b') && !err.contains('\u{202E}'),
            "{err:?}"
        );
        assert!(err.starts_with("ht[31mtp requires iii >=0.26.0"), "{err:?}");
    }
}
