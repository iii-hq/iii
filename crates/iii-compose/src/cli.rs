// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii compose` command surface.
//!
//! One command. `iii compose` connects to an engine and serves `compose::*` in
//! the foreground; everything an operator does to a project goes through `iii
//! trigger` from there, naming the project with `file=`.
//!
//! It never backgrounds itself. That is the shape a process supervisor already
//! wants, and it hands log rotation and restart-on-failure to systemd, launchd
//! or a shell redirect rather than to a rotation policy compose would have to
//! grow. Argument parsing stays separated from execution
//! ([`ComposeCli::plan`]) so the resolved invocation is testable without
//! touching a socket.

use clap::Args;

use crate::error::{ComposeError, Result};

/// Engine address used when neither `--engine` nor `III_URL` is set.
pub const DEFAULT_ENGINE_URL: &str = "ws://127.0.0.1:49134";

/// Compose file used when `--file` is not given: the one in the current
/// directory. Running compose from inside a project should not require naming
/// the file the project is made of.
pub const DEFAULT_COMPOSE_FILE: &str = "worker-compose.yaml";

#[derive(Args, Debug, Clone)]
pub struct ComposeCli {
    /// Engine WebSocket address. Falls back to III_URL, then
    /// ws://127.0.0.1:49134.
    #[arg(long, value_name = "URL")]
    pub engine: Option<String>,

    /// Namespace this daemon answers `compose::*` in. Several attach to one
    /// engine; this is what tells them apart.
    ///
    /// It is the address an operator reaches exactly one of them with:
    /// `iii trigger compose::up --namespace <NS> file=<PATH>`. Omitted, the
    /// daemon generates one and prints it.
    /// `--ns` is kept as an alias so a daemon started by an older script keeps
    /// running; it is not advertised, and `-n` / `--namespace` is the spelling
    /// everything else in the CLI uses.
    #[arg(short = 'n', long = "namespace", alias = "ns", value_name = "NS")]
    pub ns: Option<String>,
}

/// What an invocation resolved to, after the flag combination is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeCommand {
    /// Serve `compose::*` in the foreground.
    Serve {
        engine_url: String,
        daemon_namespace: String,
    },
}

impl ComposeCli {
    /// Resolves the invocation, rejecting incomplete flag combinations.
    pub fn plan(&self) -> Result<ComposeCommand> {
        let daemon_namespace = self.daemon_namespace()?;

        Ok(ComposeCommand::Serve {
            engine_url: self.engine_url(),
            daemon_namespace,
        })
    }

    /// `--ns`, or a fresh uuid when it is absent.
    ///
    /// There is no safe well-known default. A shared one — `default`, the
    /// hostname — is the collision the namespace exists to prevent: the second
    /// daemon to claim it loses the `(namespace, compose)` lease and is
    /// refused. So an invocation that does not name itself gets a name,
    /// printed on start for an operator to capture:
    ///
    /// ```text
    /// iii compose             # prints the namespace
    /// iii trigger compose::up --namespace <ns> file=./worker-compose.yaml
    /// ```
    ///
    /// A generated one is new on every start, so a daemon meant to find its own
    /// children again after a restart passes `--ns` and keeps it.
    ///
    /// Validated here rather than at first use: it is both a namespace the
    /// engine routes on and a directory under `~/.iii/compose`, so a separator
    /// or an empty string is a daemon that half-works until the first write.
    pub fn daemon_namespace(&self) -> Result<String> {
        Ok(self
            .validated_namespace()?
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()))
    }

    /// `--ns` as given, checked. `None` when it was not given — the caller
    /// decides whether that means "generate one" or "go and find it".
    pub fn validated_namespace(&self) -> Result<Option<String>> {
        let Some(namespace) = &self.ns else {
            return Ok(None);
        };

        let namespace = namespace.trim();
        // The same rule `name:` is held to. It used to be looser here — a path
        // separator was refused but a space was not — while `name:` was
        // silently rewritten, so one string meant two different namespaces
        // depending on which of the two the operator used to say it.
        let reason = if namespace.is_empty() {
            Some("it is empty")
        } else {
            crate::namespace::check(namespace).err()
        };

        match reason {
            Some(reason) => Err(ComposeError::InvalidNamespace {
                namespace: namespace.to_string(),
                reason,
            }),
            None => Ok(Some(namespace.to_string())),
        }
    }

    /// `--engine` > `III_URL` > [`DEFAULT_ENGINE_URL`]. The env var is part of
    /// the same reserved contract the daemon injects into its children.
    pub fn engine_url(&self) -> String {
        self.engine
            .clone()
            .filter(|url| !url.trim().is_empty())
            .or_else(|| std::env::var("III_URL").ok().filter(|url| !url.is_empty()))
            .unwrap_or_else(|| DEFAULT_ENGINE_URL.to_string())
    }
}
