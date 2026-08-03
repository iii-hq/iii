// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii compose` command surface.
//!
//! Three modes: a foreground daemon bound to one compose file for its whole
//! lifetime, an offline `validate` that needs no engine, and `logs`, which
//! talks to a daemon that is already running. Argument parsing is separated
//! from execution ([`ComposeCli::plan`]) so mode selection is testable without
//! touching a filesystem or a socket.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::error::{ComposeError, Result};

/// Engine address used when neither `--engine` nor `III_URL` is set.
pub const DEFAULT_ENGINE_URL: &str = "ws://127.0.0.1:49134";

/// Set in the background daemon's environment so it never detaches again,
/// whatever its arguments say.
pub const DETACHED_GUARD: &str = "III_COMPOSE_DETACHED";

/// Compose file used when `--file` is not given: the one in the current
/// directory. Running compose from inside a project should not require naming
/// the file the project is made of.
pub const DEFAULT_COMPOSE_FILE: &str = "worker-compose.yaml";

#[derive(Args, Debug, Clone)]
pub struct ComposeCli {
    #[command(subcommand)]
    pub action: Option<ComposeAction>,

    /// Daemon identity. Required in daemon mode: it names the daemon in the
    /// engine and is the `id=` every remote `compose::*` call must match.
    #[arg(long, value_name = "ID")]
    pub id: Option<String>,

    /// Engine WebSocket address. Falls back to III_URL, then
    /// ws://127.0.0.1:49134.
    #[arg(long, global = true, value_name = "URL")]
    pub engine: Option<String>,

    /// Namespace the project's workers register under. Defaults to a
    /// deterministic namespace derived from the project name and compose path.
    #[arg(long, value_name = "NS")]
    pub namespace: Option<String>,

    /// Compose file this invocation is bound to. Defaults to
    /// `worker-compose.yaml` in the current directory.
    ///
    /// Not global: `logs` reads no project, and keeping `-f` off it is what
    /// leaves the flag free to mean `--follow` there.
    #[arg(long, short = 'f', value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Run the daemon in the background and return once it is serving.
    ///
    /// Returns only after the daemon answers `compose::status`, so a failure to
    /// start is still a failure of this command: a detached daemon that dies
    /// on a duplicate `--id` must not look like a success.
    #[arg(long, short = 'd')]
    pub detach: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ComposeAction {
    /// Start the daemon and bring the project up
    Up(UpArgs),
    /// Validate a compose project without contacting an engine
    Validate(ValidateArgs),
    /// Show what the project's containers printed
    Logs(LogsArgs),
    /// Stop a running daemon and the whole project with it
    Stop(StopArgs),
}

#[derive(Args, Debug, Clone)]
pub struct StopArgs {
    /// Namespace the daemon serves in, the same one `logs` takes.
    #[arg(long, visible_alias = "ns", value_name = "NS")]
    pub namespace: String,
}

#[derive(Args, Debug, Clone, Default)]
pub struct UpArgs {
    /// Daemon identity: it names the daemon in the engine and is the namespace
    /// `stop` and `logs` address it by.
    #[arg(long, value_name = "ID")]
    pub id: Option<String>,

    /// Compose file. Defaults to `worker-compose.yaml` here.
    #[arg(long, short = 'f', value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Namespace the project's workers register under.
    #[arg(long, value_name = "NS")]
    pub namespace: Option<String>,

    /// Run in the background and return once the project is up.
    #[arg(long, short = 'd')]
    pub detach: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct ValidateArgs {
    /// Compose file to validate. Defaults to `worker-compose.yaml` here.
    #[arg(long, short = 'f', value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Report the project under this namespace instead of the derived one.
    #[arg(long, value_name = "NS")]
    pub namespace: Option<String>,
}

/// Lines returned per container when `--tail` is not given.
pub const DEFAULT_TAIL: usize = 50;

#[derive(Args, Debug, Clone)]
pub struct LogsArgs {
    /// Namespace the daemon serves in, the same one `iii trigger
    /// compose::logs --namespace` takes. A daemon registers `compose::*` under
    /// its own `--id`, so the two are the same string.
    #[arg(long, visible_alias = "ns", value_name = "NS")]
    pub namespace: String,

    /// Only this container. Defaults to every container that printed.
    #[arg(long, short = 'c', value_name = "NAME")]
    pub container: Option<String>,

    /// Lines to show per container.
    #[arg(long, short = 'n', value_name = "N", default_value_t = DEFAULT_TAIL)]
    pub tail: usize,

    /// Keep printing new lines until interrupted.
    #[arg(long, short = 'f')]
    pub follow: bool,
}

/// What an invocation resolved to, after the flag combination is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeCommand {
    Validate {
        file: PathBuf,
        /// Reported back so the operator can see which namespace the project
        /// would land in before starting anything.
        namespace: Option<String>,
    },
    Logs {
        /// Which daemon to ask. `compose::*` lives in the namespace of the
        /// daemon's own id, so this is what addresses one.
        namespace: String,
        engine_url: String,
        container: Option<String>,
        tail: usize,
        follow: bool,
    },
    Stop {
        namespace: String,
        engine_url: String,
    },
    Daemon {
        id: String,
        file: PathBuf,
        engine_url: String,
        namespace: Option<String>,
        /// Start the project immediately and fail the process if it does not
        /// come up. Set by the `up` subcommand.
        up_on_start: bool,
        /// Re-launch in the background instead of serving in the foreground.
        detach: bool,
    },
}

impl ComposeCli {
    /// Resolves the invocation, rejecting incomplete flag combinations.
    pub fn plan(&self) -> Result<ComposeCommand> {
        match &self.action {
            // Same shape as daemon mode, and the reason it is a subcommand
            // rather than a flag: `up` and `stop` are the pair an operator
            // reaches for, and a flag on one side of that pair reads as an
            // afterthought.
            Some(ComposeAction::Up(args)) => Ok(ComposeCommand::Daemon {
                id: args
                    .id
                    .clone()
                    .filter(|id| !id.trim().is_empty())
                    .map_or_else(|| self.require_id(), Ok)?,
                file: self.compose_file_or(args.file.as_ref())?,
                engine_url: self.engine_url(),
                namespace: args.namespace.clone().or_else(|| self.namespace.clone()),
                up_on_start: true,
                detach: args.detach && std::env::var_os(DETACHED_GUARD).is_none(),
            }),
            // `--file` before or after the subcommand both work: the flag moved
            // off the parent for `logs`'s sake, not to break a habit.
            Some(ComposeAction::Validate(args)) => Ok(ComposeCommand::Validate {
                file: self.compose_file_or(args.file.as_ref())?,
                namespace: args.namespace.clone().or_else(|| self.namespace.clone()),
            }),
            // No compose file is read here: the daemon holds the project, and
            // this only asks it a question. Requiring one would stop `logs`
            // from working from anywhere but the project directory.
            Some(ComposeAction::Logs(args)) => Ok(ComposeCommand::Logs {
                namespace: args.namespace.clone(),
                engine_url: self.engine_url(),
                container: args.container.clone(),
                tail: args.tail,
                follow: args.follow,
            }),
            Some(ComposeAction::Stop(args)) => Ok(ComposeCommand::Stop {
                namespace: args.namespace.clone(),
                engine_url: self.engine_url(),
            }),
            None => {
                let file = self.compose_file()?;
                Ok(ComposeCommand::Daemon {
                    id: self.require_id()?,
                    file,
                    engine_url: self.engine_url(),
                    namespace: self.namespace.clone(),
                    // Bare daemon mode serves without starting anything: that
                    // is `iii compose up`, a separate decision.
                    up_on_start: false,
                    // The child is launched with this set, so a stray `-d` in
                    // its own argv can never make it fork again.
                    detach: self.detach && std::env::var_os(DETACHED_GUARD).is_none(),
                })
            }
        }
    }

    /// The daemon identity, which both daemon mode and `logs` need.
    fn require_id(&self) -> Result<String> {
        self.id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .ok_or(ComposeError::MissingFlag { flag: "--id" })
    }

    /// `--file`, else `worker-compose.yaml` in the current directory.
    ///
    /// When the default is used and there is no such file, say so directly: the
    /// operator did not name a path, so reporting one back as "unreadable"
    /// would answer a question they never asked.
    fn compose_file(&self) -> Result<PathBuf> {
        self.compose_file_or(None)
    }

    /// Same, preferring a path the subcommand carried.
    fn compose_file_or(&self, from_action: Option<&PathBuf>) -> Result<PathBuf> {
        if let Some(file) = from_action.or(self.file.as_ref()) {
            return Ok(file.clone());
        }
        let default = PathBuf::from(DEFAULT_COMPOSE_FILE);
        if default.is_file() {
            return Ok(default);
        }
        Err(ComposeError::NoComposeFileHere {
            expected: DEFAULT_COMPOSE_FILE,
        })
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
