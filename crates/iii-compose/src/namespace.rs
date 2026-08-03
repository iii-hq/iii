// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Where a project registers.
//!
//! One namespace holds everything a project puts on the engine: the daemon's
//! own `compose::*` and every container's functions. Resolution has three
//! steps and no derivation — `--ns` on the command line, then `name:` in the
//! compose file, then `default`.
//!
//! Nothing is hashed into it. A namespace an operator cannot predict is one
//! they cannot address, and addressing it is the whole point: `stop`, `logs`
//! and every `iii trigger` take it by hand. The cost is that two copies of a
//! project sharing a `name:` now collide instead of quietly running side by
//! side, which is the intended reading of a duplicate.

/// Namespace the daemon and its containers register in.
///
/// `explicit` is `--ns`; `declared` is `name:` in the compose file. Blank
/// values count as absent: a file with `name: ""` has not named anything.
pub fn project_namespace(explicit: Option<&str>, declared: Option<&str>) -> String {
    first_present([explicit, declared])
        .map(sanitize)
        .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string())
}

/// The daemon's own worker name, resolved the same way with a different last
/// resort.
///
/// It cannot fall back to `default`: that is a namespace, not a name, and the
/// lease is on the pair. Two unnamed daemons in `default` therefore both claim
/// `compose` and the second is rejected — which is what a duplicate should do.
pub fn daemon_worker_name(explicit: Option<&str>, declared: Option<&str>) -> String {
    first_present([explicit, declared])
        .map(sanitize)
        .unwrap_or_else(|| UNNAMED_DAEMON.to_string())
}

/// Namespace for a project that names itself nowhere.
pub const DEFAULT_NAMESPACE: &str = "default";

/// Worker name for a daemon that names itself nowhere.
pub const UNNAMED_DAEMON: &str = "compose";

fn first_present(candidates: [Option<&str>; 2]) -> Option<&str> {
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

/// Namespaces travel into queue names, metric labels and log lines, so the
/// derived form stays inside `[a-z0-9_-]`.
fn sanitize(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_was_dash = false;
    for ch in value.chars() {
        let mapped = match ch {
            'a'..='z' | '0'..='9' | '_' | '-' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => '-',
        };
        if mapped == '-' {
            if last_was_dash {
                continue;
            }
            last_was_dash = true;
        } else {
            last_was_dash = false;
        }
        out.push(mapped);
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_wins() {
        assert_eq!(project_namespace(Some("orders"), Some("ignored")), "orders");
    }

    #[test]
    fn the_file_is_used_when_the_command_line_says_nothing() {
        assert_eq!(project_namespace(None, Some("orders")), "orders");
    }

    #[test]
    fn naming_it_nowhere_lands_in_default() {
        // The same rule the rest of the engine follows: no namespace means
        // `default`, not a namespace invented on the project's behalf.
        assert_eq!(project_namespace(None, None), DEFAULT_NAMESPACE);
    }

    #[test]
    fn blank_counts_as_absent() {
        // `name: ""` has not named anything, and `--ns ""` has not either.
        assert_eq!(project_namespace(Some("  "), Some("orders")), "orders");
        assert_eq!(project_namespace(Some("  "), None), DEFAULT_NAMESPACE);
        assert_eq!(project_namespace(None, Some("")), DEFAULT_NAMESPACE);
    }

    #[test]
    fn the_same_name_yields_the_same_namespace_anywhere() {
        // Nothing about the path enters it. Two copies of a project therefore
        // collide rather than quietly running side by side, which is what a
        // duplicate should do.
        assert_eq!(
            project_namespace(None, Some("shop")),
            project_namespace(None, Some("shop"))
        );
    }

    #[test]
    fn a_name_is_reduced_to_what_a_namespace_may_hold() {
        // Namespaces travel into queue names, metric labels and log lines.
        assert_eq!(project_namespace(None, Some("My Shop!")), "my-shop");
        assert_eq!(project_namespace(Some("A B"), None), "a-b");
    }

    #[test]
    fn the_daemon_name_follows_the_same_order() {
        assert_eq!(daemon_worker_name(Some("shop"), Some("loja")), "shop");
        assert_eq!(daemon_worker_name(None, Some("loja")), "loja");
    }

    #[test]
    fn an_unnamed_daemon_is_still_named_something() {
        // `default` is a namespace, not a name, and the lease is on the pair.
        // Two unnamed daemons therefore both claim `compose`, and the second
        // is rejected.
        assert_eq!(daemon_worker_name(None, None), UNNAMED_DAEMON);
        assert_ne!(daemon_worker_name(None, None), DEFAULT_NAMESPACE);
    }
}
