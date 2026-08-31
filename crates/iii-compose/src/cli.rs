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
//! compose file is still read without `--up` when it exists, so its engine URL
//! and namespace configure the daemon without also starting the project.
//!
//! The compose process never backgrounds itself; only the managed engine child
//! does. That is the shape a process supervisor already wants, and it hands
//! restart-on-failure for compose to systemd, launchd or a shell redirect.
//! Argument parsing stays separated from execution
//! ([`ComposeCli::plan`]) so the resolved invocation is testable without
//! touching a socket.
//!
//! `iii compose build` is the one local action. It reads a compose file and
//! prepares its registry packages without connecting to or starting an engine.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::{
    error::{ComposeError, Result},
    logs::LogStream,
};

/// Compose file used when `--file` is not given: the one in the current
/// directory. Running compose from inside a project should not require naming
/// the file the project is made of.
pub const DEFAULT_COMPOSE_FILE: &str = "worker-compose.yaml";

#[derive(Args, Debug, Clone)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ComposeCli {
    /// Existing engine WebSocket address. Overrides the compose file and
    /// III_URL. The local default is used when none of them supplies a URL.
    ///
    #[arg(long, value_name = "URL")]
    pub engine: Option<String>,

    /// Namespace this daemon answers `compose::*` in and applies to every
    /// project it loads. Several daemons attach to one engine; this is what
    /// tells them apart.
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

    /// Serve with one project brought up first, starting its declared engine
    /// unless `--engine` selects an existing one.
    #[arg(long)]
    pub up: bool,

    /// The compose file. Only valid with `--up`. Defaults to
    /// `./worker-compose.yaml`, the same fallback `compose::up` uses when a
    /// call names no file.
    #[arg(short = 'f', long, value_name = "PATH", requires = "up")]
    pub file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<ComposeSubcommand>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ComposeSubcommand {
    /// Download every registry package declared by the compose file.
    Build(BuildCli),
    /// Read retained worker stdout and stderr from a running Compose daemon.
    Logs(ComposeLogsCli),
}

#[derive(Args, Debug, Clone)]
pub struct BuildCli {
    /// Compose file whose registry packages should be downloaded.
    #[arg(
        short = 'f',
        long,
        value_name = "PATH",
        default_value = DEFAULT_COMPOSE_FILE
    )]
    pub file: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct ComposeLogsCli {
    /// Worker to read. Omit to read every worker in the project.
    #[arg(value_name = "WORKER")]
    pub worker: Option<String>,

    /// Existing engine WebSocket address. The compose file and III_URL are
    /// used when omitted.
    #[arg(long, value_name = "URL")]
    pub engine: Option<String>,

    /// Namespace of the Compose daemon that owns the project.
    #[arg(short = 'n', long = "namespace", value_name = "NS")]
    pub namespace: Option<String>,

    /// Compose file path on the daemon host. The daemon's default file is used
    /// when omitted.
    #[arg(short = 'f', long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Number of recent lines to show before following new output.
    #[arg(long, default_value_t = crate::logs::DEFAULT_TAIL_LINES, value_parser = parse_tail)]
    pub tail: usize,

    /// Continue waiting for new output until interrupted.
    #[arg(short = 'F', long)]
    pub follow: bool,

    /// Restrict output to one process stream.
    #[arg(long, value_enum)]
    pub stream: Option<LogStream>,
}

fn parse_tail(value: &str) -> std::result::Result<usize, String> {
    let tail = value
        .parse::<usize>()
        .map_err(|_| "tail must be a non-negative integer".to_string())?;
    if tail > crate::logs::MAX_TAIL_LINES {
        return Err(format!(
            "tail must not exceed {}",
            crate::logs::MAX_TAIL_LINES
        ));
    }
    Ok(tail)
}

/// What an invocation resolved to, after the flag combination is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeCommand {
    /// Download registry packages without starting an engine or a worker.
    Build { file: PathBuf },
    /// Serve `compose::*` in the foreground.
    Serve {
        explicit_engine_url: Option<String>,
        /// A namespace set on the CLI. `None` is resolved after the initial
        /// compose file is loaded, so `--up` can inherit the file namespace.
        explicit_daemon_namespace: Option<String>,
        /// The compose file used to configure this invocation. A bare daemon
        /// tolerates it being absent; `--up` requires it.
        file: PathBuf,
        /// Whether to bring `file` up before the first call arrives.
        start: bool,
    },
    /// Read process output through the already-running daemon.
    Logs {
        explicit_engine_url: Option<String>,
        explicit_daemon_namespace: Option<String>,
        file: Option<PathBuf>,
        container: Option<String>,
        tail: usize,
        follow: bool,
        stream: Option<LogStream>,
    },
}

impl ComposeCli {
    /// Resolves the invocation, rejecting incomplete flag combinations.
    pub fn plan(&self) -> Result<ComposeCommand> {
        if let Some(command) = &self.command {
            return match command {
                ComposeSubcommand::Build(args) => {
                    if self.engine.is_some() || self.ns.is_some() || self.up || self.file.is_some()
                    {
                        return Err(ComposeError::BuildConflictsWithServeOptions);
                    }
                    Ok(ComposeCommand::Build {
                        file: args.file.clone(),
                    })
                }
                ComposeSubcommand::Logs(logs) => Ok(ComposeCommand::Logs {
                    explicit_engine_url: logs.engine.clone().filter(|url| !url.trim().is_empty()),
                    explicit_daemon_namespace: validated_namespace(logs.namespace.as_deref())?,
                    file: logs.file.clone(),
                    container: logs.worker.clone(),
                    tail: logs.tail.min(crate::logs::MAX_TAIL_LINES),
                    follow: logs.follow,
                    stream: logs.stream,
                }),
            };
        }

        if !self.up && self.file.is_some() {
            return Err(ComposeError::FileRequiresUp);
        }

        let explicit_daemon_namespace = self.validated_namespace()?;

        // A missing `--file` is not "no file": the default file still supplies
        // daemon configuration even when the project is not brought up yet.
        let file = self
            .file
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_COMPOSE_FILE));

        Ok(ComposeCommand::Serve {
            // Keep the environment out of the parsed plan. File, environment,
            // and default resolution all happen together after the file load.
            explicit_engine_url: self.engine.clone().filter(|url| !url.trim().is_empty()),
            explicit_daemon_namespace,
            file,
            start: self.up,
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
        validated_namespace(self.ns.as_deref())
    }

    /// Returns the explicit process-level engine selection. File and default
    /// resolution happens after the compose file is loaded.
    pub fn requested_engine_url(&self) -> Option<String> {
        self.engine
            .clone()
            .filter(|url| !url.trim().is_empty())
            .or_else(|| std::env::var("III_URL").ok().filter(|url| !url.is_empty()))
    }
}

fn validated_namespace(namespace: Option<&str>) -> Result<Option<String>> {
    let Some(namespace) = namespace else {
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
