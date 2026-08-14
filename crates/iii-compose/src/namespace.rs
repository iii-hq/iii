// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Where a project registers.
//!
//! One namespace holds everything a project puts on the engine: the daemon's
//! own `compose::*` and every container's functions. Resolution has three
//! steps and no derivation — `--namespace` on the command line, then `name:` in the
//! compose file, then `default`.
//!
//! Nothing is hashed into it. A namespace an operator cannot predict is one
//! they cannot address, and addressing it is the whole point: `stop`, `logs`
//! and every `iii trigger` take it by hand. The cost is that two copies of a
//! project sharing a `name:` now collide instead of quietly running side by
//! side, which is the intended reading of a duplicate.

/// Namespace the daemon and its containers register in.
///
/// `explicit` is `--namespace`; `declared` is `name:` in the compose file. Blank
/// values count as absent: a file with `name: ""` has not named anything.
///
/// Both have already been checked by [`check`] — `name:` when the compose file
/// parsed, `--namespace` when the command line did — so this only picks one.
pub fn project_namespace(explicit: Option<&str>, declared: Option<&str>) -> String {
    first_present([explicit, declared])
        .map(str::to_string)
        .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string())
}

/// Namespace for a project that names itself nowhere.
pub const DEFAULT_NAMESPACE: &str = "default";

/// The character set a namespace may hold.
pub const NAMESPACE_CHARSET: &str = "a-z, 0-9, '-' and '_'";

fn first_present(candidates: [Option<&str>; 2]) -> Option<&str> {
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

/// Whether `value` may be used as a namespace, and why not when it may not.
///
/// Rejects rather than rewrites. Namespaces travel into queue names, metric
/// labels and log lines, so the set is narrow — but the operator has to be
/// able to *type* the value back: it is what goes into `iii trigger
/// --namespace` and into every `worker.trigger` call. A value rewritten on the
/// way in is one they cannot type, and the rewrite was lossy besides, so
/// `My Shop!`, `my/shop`, `my shop` and `MY-SHOP` all became the same project
/// without any of the four declarations saying so.
///
/// Blank is not this function's business: an absent name is not an invalid
/// one, and the callers treat it as "not named".
pub fn check(value: &str) -> Result<(), &'static str> {
    if value.trim() != value {
        return Err("it has leading or trailing whitespace");
    }
    match value
        .chars()
        .find(|ch| !matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-'))
    {
        // Named separately from the general case: lowercasing is the rewrite an
        // operator is most likely to expect, so the error has to say that it is
        // not happening rather than leave them guessing at the rule.
        Some(ch) if ch.is_ascii_uppercase() => {
            Err("namespaces are lowercase, and are not lowercased for you")
        }
        Some(_) => Err("it may hold only a-z, 0-9, '-' and '_'"),
        None => Ok(()),
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
        // `name: ""` has not named anything, and `--namespace ""` has not either.
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
    fn a_name_is_taken_exactly_as_written() {
        // It used to be rewritten to fit. What the operator declares is what
        // they have to type into `--namespace`, so it arrives untouched or it
        // does not arrive.
        assert_eq!(project_namespace(None, Some("my-shop")), "my-shop");
        assert_eq!(project_namespace(Some("shop_2"), None), "shop_2");
    }

    #[test]
    fn what_a_namespace_may_hold() {
        assert!(check("my-shop").is_ok());
        assert!(check("shop_2").is_ok());
        assert!(check("a").is_ok());

        for rejected in ["My Shop!", "my/shop", "my shop", "a..b", "!!!", "---x!"] {
            assert!(check(rejected).is_err(), "{rejected} should be rejected");
        }
    }

    #[test]
    fn the_four_that_used_to_become_one_project_are_now_four_errors() {
        // `My Shop!`, `my/shop`, `my shop` and `MY-SHOP` all sanitized to
        // `my-shop`, so four different declarations addressed one namespace
        // and none of them said so.
        for collided in ["My Shop!", "my/shop", "my shop", "MY-SHOP"] {
            assert!(check(collided).is_err(), "{collided} should be rejected");
        }
    }

    #[test]
    fn uppercase_says_why_rather_than_lowercasing() {
        // The rewrite an operator is likeliest to expect, so the refusal has
        // to name itself instead of reading as an unexplained charset error.
        let reason = check("MY-SHOP").expect_err("uppercase is rejected");
        assert!(reason.contains("lowercase"), "unhelpful reason: {reason}");
    }

    #[test]
    fn a_name_made_only_of_rejected_characters_is_not_invented_into_one() {
        // `!!!` used to become the literal `project`, naming a namespace after
        // nothing the file contained.
        assert!(check("!!!").is_err());
    }
}
