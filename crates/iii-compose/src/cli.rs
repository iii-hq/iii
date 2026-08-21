// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii compose` command surface.
//!
//! Bare `iii compose` connects to an existing engine and serves `compose::*`
//! in the foreground; everything an operator does to a project goes through
//! `iii trigger` from there, naming the project with `file=`.
//!
//! `iii compose up` is the same daemon with the first call already made and a
//! managed engine started first by default. A daemon that serves nothing is
//! not what an operator wants in a project directory: they want the project
//! running, and the daemon is how it stays running. `--no-engine` keeps the
//! external-engine workflow for an engine the operator already owns.
//!
//! The compose process never backgrounds itself; only the managed engine child
//! does. That is the shape a process supervisor already wants, and it hands
//! restart-on-failure for compose to systemd, launchd or a shell redirect.
//! Argument parsing stays separated from execution
//! ([`ComposeCli::plan`]) so the resolved invocation is testable without
//! touching a socket.

use std::path::PathBuf;

use clap::{Args, Subcommand};

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
    ///
    /// Global, so it reads the same before or after a subcommand.
    #[arg(long, value_name = "URL", global = true)]
    pub engine: Option<String>,

    /// Namespace this daemon answers `compose::*` in. Several attach to one
    /// engine; this is what tells them apart.
    ///
    /// It is the address an operator reaches exactly one of them with:
    /// `iii trigger compose::up --namespace <NS> file=<PATH>`. Omitted, the
    /// daemon generates one and prints it.
    ///
    /// Spelled the way the rest of the CLI spells it, and only that way: this
    /// has not shipped, so there is nothing calling it `--ns` to keep working.
    #[arg(short = 'n', long = "namespace", value_name = "NS", global = true)]
    pub ns: Option<String>,

    /// Absent, the daemon starts holding nothing.
    #[command(subcommand)]
    pub command: Option<ComposeSub>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ComposeSub {
    /// Start an engine, then serve with one project brought up first.
    Up {
        /// The compose file. Defaults to `./worker-compose.yaml`, the same
        /// fallback `compose::up` uses when a call names no file.
        #[arg(short = 'f', long, value_name = "PATH")]
        file: Option<PathBuf>,

        /// Connect to an engine that is already running instead of starting
        /// and stopping one with this compose invocation.
        #[arg(long)]
        no_engine: bool,
    },
}

/// Who owns the engine lifecycle for this invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineOwnership {
    /// Compose starts the engine and stops it after every project is down.
    Managed,
    /// The operator owns the engine; compose only connects to it.
    External,
}

/// What an invocation resolved to, after the flag combination is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeCommand {
    /// Serve `compose::*` in the foreground.
    Serve {
        engine_url: String,
        daemon_namespace: String,
        engine_ownership: EngineOwnership,
        /// A project to bring up before the first call arrives. `None` is a
        /// daemon that starts holding nothing.
        start: Option<PathBuf>,
    },
}

impl ComposeCli {
    /// Resolves the invocation, rejecting incomplete flag combinations.
    pub fn plan(&self) -> Result<ComposeCommand> {
        let daemon_namespace = self.daemon_namespace()?;

        // A missing `--file` is not "no file": it is the same fallback a call
        // with no `file=` gets, the compose file in the working directory.
        let start = self.command.as_ref().map(|ComposeSub::Up { file, .. }| {
            file.clone()
                .unwrap_or_else(|| PathBuf::from(DEFAULT_COMPOSE_FILE))
        });
        let engine_ownership = match &self.command {
            Some(ComposeSub::Up {
                no_engine: false, ..
            }) => EngineOwnership::Managed,
            Some(ComposeSub::Up {
                no_engine: true, ..
            })
            | None => EngineOwnership::External,
        };

        Ok(ComposeCommand::Serve {
            engine_url: self.engine_url(),
            daemon_namespace,
            engine_ownership,
            start,
        })
    }

    /// `--namespace`, or a generated name when it is absent.
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
    /// children again after a restart passes `--namespace` and keeps it.
    ///
    /// Validated here rather than at first use: it is both a namespace the
    /// engine routes on and a directory under `~/.iii/compose`, so a separator
    /// or an empty string is a daemon that half-works until the first write.
    pub fn daemon_namespace(&self) -> Result<String> {
        Ok(self.validated_namespace()?.unwrap_or_else(|| {
            // A name already holding state on this machine belongs to a daemon
            // that ran before, and taking it would mean adopting what it left.
            let root = crate::state::StateStore::root().ok();
            crate::name::generate(|candidate| {
                root.as_ref()
                    .is_some_and(|root| root.join(candidate).exists())
            })
        }))
    }

    /// `--namespace` as given, checked. `None` when it was not given — the caller
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
