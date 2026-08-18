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
//! `config_override` is not expanded at all. That block is not compose's
//! language: it is data compose carries to the configuration worker, which
//! resolves `${VAR}` references of its own at read time — that is how a secret
//! is stored as a reference rather than as a value. Expanding one here would
//! write the value down, permanently, in the entry that existed to avoid it.
//!
//! `$${VAR}` still writes a literal `${VAR}`, for a reference that has to sit
//! somewhere else. It should be rare: the block that carries them is already
//! left alone.

use std::path::Path;

use crate::error::{ComposeError, Result};

/// The one block compose carries without reading. See the module docs.
const NOT_OURS: &str = "config_override";

/// Expands every string in a parsed compose document, except under
/// [`NOT_OURS`].
///
/// Walking the parsed document rather than the text is what makes that
/// exception possible: `${VAR}` and the block it sits in are only
/// distinguishable once the shape is known.
pub fn expand_tree(
    value: &mut serde_yaml::Value,
    path: &Path,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<()> {
    match value {
        serde_yaml::Value::String(text) => *text = expand(text, path, lookup)?,
        serde_yaml::Value::Sequence(items) => {
            for item in items {
                expand_tree(item, path, lookup)?;
            }
        }
        serde_yaml::Value::Mapping(entries) => {
            for (key, entry) in entries.iter_mut() {
                // Keys are identities — a container name, a variable name — and
                // an operator naming one from the environment is not a thing
                // compose supports.
                if key.as_str() == Some(NOT_OURS) {
                    continue;
                }
                expand_tree(entry, path, lookup)?;
            }
        }
        _ => {}
    }
    Ok(())
}

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
    fn tree(yaml: &str, pairs: &[(&str, &str)]) -> Result<serde_yaml::Value> {
        let mut value: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        expand_tree(&mut value, Path::new("worker-compose.yaml"), &env(pairs))?;
        Ok(value)
    }

    fn at<'a>(value: &'a serde_yaml::Value, path: &[&str]) -> &'a str {
        let mut cursor = value;
        for step in path {
            cursor = &cursor[*step];
        }
        cursor.as_str().unwrap_or_default()
    }

    #[test]
    fn a_config_override_keeps_its_own_references() {
        // The configuration worker resolves these at read time, which is how a
        // secret stays a reference. Expanding one here would write the value
        // into the entry that existed to avoid exactly that.
        let value = tree(
            "containers:\n  api:\n    worker: path://${DIR}/api\n    config_override:\n      api_key: ${ANTHROPIC_API_KEY}\n      nested:\n        deep: ${ALSO_LEFT}\n",
            &[
                ("DIR", "./workers"),
                ("ANTHROPIC_API_KEY", "sk-secret"),
                ("ALSO_LEFT", "no"),
            ],
        )
        .unwrap();

        assert_eq!(
            at(&value, &["containers", "api", "worker"]),
            "path://./workers/api"
        );
        assert_eq!(
            at(&value, &["containers", "api", "config_override", "api_key"]),
            "${ANTHROPIC_API_KEY}",
            "the block compose only carries must come through untouched"
        );
        assert_eq!(
            at(
                &value,
                &["containers", "api", "config_override", "nested", "deep"]
            ),
            "${ALSO_LEFT}",
            "including below the first level"
        );
    }

    #[test]
    fn a_name_missing_under_config_override_is_not_an_error() {
        // Nothing there is compose's to resolve, so nothing there can be
        // missing as far as compose is concerned.
        let value = tree(
            "containers:\n  api:\n    config_override:\n      key: ${NOT_ON_THIS_HOST}\n",
            &[],
        )
        .unwrap();
        assert_eq!(
            at(&value, &["containers", "api", "config_override", "key"]),
            "${NOT_ON_THIS_HOST}"
        );
    }

    #[test]
    fn everything_outside_that_block_still_expands() {
        let value = tree(
            "containers:\n  api:\n    environment:\n      LEVEL: ${LEVEL}\n    env_file:\n      - ${DIR}/.env\n",
            &[("LEVEL", "debug"), ("DIR", "./cfg")],
        )
        .unwrap();
        assert_eq!(
            at(&value, &["containers", "api", "environment", "LEVEL"]),
            "debug"
        );
        assert_eq!(
            value["containers"]["api"]["env_file"][0].as_str().unwrap(),
            "./cfg/.env"
        );
    }
}
