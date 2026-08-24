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
//! `iii compose --up` is the same daemon with the first call already made. The
//! initial compose file decides engine ownership: an `engine:` section starts
//! a managed engine, while its absence requires `--engine` or `III_URL`.
//!
//! The compose process never backgrounds itself; only the managed engine child
//! does. That is the shape a process supervisor already wants, and it hands
//! restart-on-failure for compose to systemd, launchd or a shell redirect.
//! Argument parsing stays separated from execution
//! ([`ComposeCli::plan`]) so the resolved invocation is testable without
//! touching a socket.

use std::path::PathBuf;

use clap::Args;

use crate::error::{ComposeError, Result};

/// Compose file used when `--file` is not given: the one in the current
/// directory. Running compose from inside a project should not require naming
/// the file the project is made of.
pub const DEFAULT_COMPOSE_FILE: &str = "worker-compose.yaml";

#[derive(Args, Debug, Clone)]
pub struct ComposeCli {
    /// Existing engine WebSocket address. Falls back to III_URL when the
    /// compose file does not declare an engine section.
    ///
    #[arg(long, value_name = "URL")]
    pub engine: Option<String>,

    /// Namespace this daemon answers `compose::*` in. Several attach to one
    /// engine; this is what tells them apart.
    ///
    /// It is the address an operator reaches exactly one of them with:
    /// `iii trigger compose::up --namespace <NS> file=<PATH>`. With `--up`, an
    /// omitted value inherits the initial compose file namespace. Without an
    /// initial namespace, the daemon uses `default`.
    ///
    /// Spelled the way the rest of the CLI spells it, and only that way: this
    /// has not shipped, so there is nothing calling it `--ns` to keep working.
    #[arg(short = 'n', long = "namespace", value_name = "NS")]
    pub ns: Option<String>,

    /// Serve with one project brought up first, starting its declared engine.
    #[arg(long)]
    pub up: bool,

    /// The compose file. Defaults to `./worker-compose.yaml`, the same
    /// fallback `compose::up` uses when a call names no file.
    #[arg(short = 'f', long, value_name = "PATH", requires = "up")]
    pub file: Option<PathBuf>,
}

/// What an invocation resolved to, after the flag combination is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeCommand {
    /// Serve `compose::*` in the foreground.
    Serve {
        explicit_engine_url: Option<String>,
        /// A namespace set on the CLI. `None` is resolved after the initial
        /// compose file is loaded, so `--up` can inherit the file namespace.
        explicit_daemon_namespace: Option<String>,
        /// A project to bring up before the first call arrives. `None` is a
        /// daemon that starts holding nothing.
        start: Option<PathBuf>,
    },
}

impl ComposeCli {
    /// Resolves the invocation, rejecting incomplete flag combinations.
    pub fn plan(&self) -> Result<ComposeCommand> {
        let explicit_daemon_namespace = self.validated_namespace()?;

        // A missing `--file` is not "no file": it is the same fallback a call
        // with no `file=` gets, the compose file in the working directory.
        let start = self.up.then(|| {
            self.file
                .clone()
                .unwrap_or_else(|| PathBuf::from(DEFAULT_COMPOSE_FILE))
        });

        Ok(ComposeCommand::Serve {
            // Keep the environment out of the parsed plan. An `engine:`
            // section owns its managed engine and ignores a process-wide
            // III_URL, while an explicit --engine is a contradictory request
            // we can report to the operator.
            explicit_engine_url: self.engine.clone().filter(|url| !url.trim().is_empty()),
            explicit_daemon_namespace,
            start,
        })
    }

    /// `--namespace`, or `default` when no compose file is available.
    ///
    /// Validated here rather than at first use: it is both a namespace the
    /// engine routes on and a directory under `~/.iii/compose`, so a separator
    /// or an empty string is a daemon that half-works until the first write.
    pub fn daemon_namespace(&self) -> Result<String> {
        let explicit = self.validated_namespace()?;
        Ok(crate::namespace::project_namespace(
            explicit.as_deref(),
            None,
        ))
    }

    /// `--namespace` as given, checked. `None` when it was not given — the caller
    /// decides whether to inherit the compose file namespace or use `default`.
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

    /// `--engine` > `III_URL`. There is deliberately no external-engine
    /// default: without an `engine:` section the operator must name the engine
    /// Compose is allowed to use.
    pub fn requested_engine_url(&self) -> Option<String> {
        self.engine
            .clone()
            .filter(|url| !url.trim().is_empty())
            .or_else(|| std::env::var("III_URL").ok().filter(|url| !url.is_empty()))
    }
}
