// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `${VAR}` in a compose file, resolved from the environment compose runs in.
//!
//! A container gets `env_clear()` and a short whitelist, so nothing an operator
//! exported reaches a worker by accident. That is the behaviour worth keeping,
//! and it is also what makes this necessary: without a way to say "this one",
//! the only route for a host value is an env file written by hand.
//!
//! So the file names what it wants. `${RUST_LOG}` is an operator asking for one
//! value, per key, in writing — the whitelist is not widened, and reading the
//! file still tells you everything that reaches a worker.
//!
//! ## What is a reference and what is not
//!
//! `${VAR}` and `${VAR:-default}` are expanded here, before the YAML is parsed,
//! so they work in any value: a path, a version, a `config_override` entry.
//!
//! A bare `$VAR` is left alone. `scripts.run` is a shell command, and rewriting
//! `$PWD` or `$III_WORKER_NAME` before the shell sees them would break the one
//! field most likely to hold them.
//!
//! `$${VAR}` writes a literal `${VAR}`. That is not a courtesy: the
//! configuration worker resolves `${VAR}` references of its own, at read time
//! and on purpose, so that a secret is stored as a reference rather than as a
//! value. A `config_override` meant to carry one writes `$${DB_PASSWORD}` and
//! keeps it deferred.

use std::path::Path;

use crate::error::{ComposeError, Result};

/// Expands every `${...}` in `text`, reading `lookup` for each name.
///
/// Fails on a name with no value and no default, naming it: a compose file that
/// silently loses a value is one where the container starts and misbehaves
/// later, which costs more to find than a refusal at load.
pub fn expand(text: &str, path: &Path, lookup: &dyn Fn(&str) -> Option<String>) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];

        // `$$` is one literal `$`, which is how a `${VAR}` meant for somebody
        // else survives this pass.
        if let Some(after) = rest.strip_prefix("$$") {
            out.push('$');
            rest = after;
            continue;
        }

        let Some(after) = rest.strip_prefix("${") else {
            // A bare `$`, or `$VAR`: not ours.
            out.push('$');
            rest = &rest[1..];
            continue;
        };

        let Some(end) = after.find('}') else {
            return Err(ComposeError::UnterminatedReference {
                path: path.to_path_buf(),
                reference: preview(rest),
            });
        };
        let (body, remainder) = (&after[..end], &after[end + 1..]);
        out.push_str(&resolve(body, path, lookup)?);
        rest = remainder;
    }

    out.push_str(rest);
    Ok(out)
}

/// One `${...}` body: a name, and optionally `:-` and a default.
fn resolve(body: &str, path: &Path, lookup: &dyn Fn(&str) -> Option<String>) -> Result<String> {
    let (name, default) = match body.split_once(":-") {
        Some((name, default)) => (name.trim(), Some(default)),
        None => (body.trim(), None),
    };

    if name.is_empty() || !is_env_name(name) {
        return Err(ComposeError::InvalidReference {
            path: path.to_path_buf(),
            name: body.to_string(),
        });
    }

    match lookup(name) {
        Some(value) => Ok(value),
        // `${VAR:-}` is an empty default, and deliberate: it is how an operator
        // says the value is optional.
        None => default
            .map(str::to_string)
            .ok_or(ComposeError::UndefinedVariable {
                path: path.to_path_buf(),
                name: name.to_string(),
            }),
    }
}

/// What a shell would accept as a variable name. Anything else is a typo worth
/// reporting rather than a name that happens to be unset.
fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Enough of an unterminated reference to find it in the file.
fn preview(rest: &str) -> String {
    let line = rest.lines().next().unwrap_or(rest);
    line.chars().take(40).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            owned
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    fn run(text: &str, pairs: &[(&str, &str)]) -> Result<String> {
        expand(text, Path::new("worker-compose.yaml"), &env(pairs))
    }

    #[test]
    fn a_reference_takes_the_value_from_the_environment() {
        assert_eq!(
            run("RUST_LOG: ${RUST_LOG}", &[("RUST_LOG", "debug")]).unwrap(),
            "RUST_LOG: debug"
        );
    }

    #[test]
    fn a_default_covers_a_name_the_host_does_not_have() {
        assert_eq!(run("x: ${NOPE:-info}", &[]).unwrap(), "x: info");
        // Empty is a default like any other: it says the value is optional.
        assert_eq!(run("x: ${NOPE:-}", &[]).unwrap(), "x: ");
    }

    #[test]
    fn a_name_with_no_value_and_no_default_is_refused() {
        let err = run("x: ${NOPE}", &[]).unwrap_err();
        assert_eq!(err.code(), "UNDEFINED_VARIABLE");
        assert!(err.to_string().contains("NOPE"), "{err}");
    }

    #[test]
    fn a_shell_variable_is_left_for_the_shell() {
        // `scripts.run` is the field most likely to hold one, and rewriting it
        // here would break the command before the shell ever saw it.
        let text = "run: sh -c 'echo $PWD $III_WORKER_NAME'";
        assert_eq!(run(text, &[("PWD", "/tmp")]).unwrap(), text);
    }

    #[test]
    fn a_doubled_sign_defers_a_reference_to_whoever_reads_it_next() {
        // The configuration worker resolves `${VAR}` of its own, at read time,
        // which is how a secret is stored as a reference and not as a value.
        assert_eq!(
            run("password: $${DB_PASSWORD}", &[("DB_PASSWORD", "hunter2")]).unwrap(),
            "password: ${DB_PASSWORD}"
        );
        assert_eq!(run("cost: 5$$", &[]).unwrap(), "cost: 5$");
    }

    #[test]
    fn several_references_in_one_value_all_resolve() {
        assert_eq!(
            run(
                "path: ${ROOT}/${NAME}/data",
                &[("ROOT", "/srv"), ("NAME", "queue")]
            )
            .unwrap(),
            "path: /srv/queue/data"
        );
    }

    #[test]
    fn an_unterminated_reference_is_refused_rather_than_copied() {
        assert_eq!(
            run("x: ${OPEN", &[]).unwrap_err().code(),
            "UNTERMINATED_REFERENCE"
        );
    }

    #[test]
    fn a_body_that_is_not_a_name_is_a_typo_not_an_unset_variable() {
        assert_eq!(
            run("x: ${a b}", &[]).unwrap_err().code(),
            "INVALID_REFERENCE"
        );
        assert_eq!(run("x: ${}", &[]).unwrap_err().code(), "INVALID_REFERENCE");
        assert_eq!(
            run("x: ${1ST}", &[]).unwrap_err().code(),
            "INVALID_REFERENCE"
        );
    }

    #[test]
    fn a_file_with_nothing_to_expand_comes_back_unchanged() {
        let text = "containers:\n  api:\n    worker: path://./api\n";
        assert_eq!(run(text, &[]).unwrap(), text);
    }
}
