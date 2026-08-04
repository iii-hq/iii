// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii compose` command surface.
//!
//! One command. `iii compose` connects to an engine and serves `compose::*`;
//! everything an operator does to a project goes through `iii trigger` from
//! there, naming the project with `id=`.
//!
//! The flags that remain are about where the daemon runs, not what it
//! does: `-d` puts it in the background, `--attach` puts its output back on a
//! terminal, `--pid-file` says where it records its pid. Argument parsing stays
//! separated from execution
//! ([`ComposeCli::plan`]) so mode selection is testable without touching a
//! socket.

use std::path::PathBuf;

use clap::Args;

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

/// Pid file a backgrounded daemon writes when `--pid-file` is not given: one in
/// the directory compose was started from, next to the project it serves.
pub const DEFAULT_PID_FILE: &str = "compose.pid";

#[derive(Args, Debug, Clone)]
pub struct ComposeCli {
    /// Engine WebSocket address. Falls back to III_URL, then
    /// ws://127.0.0.1:49134.
    #[arg(long, value_name = "URL")]
    pub engine: Option<String>,

    /// Run in the background and return once the daemon is serving.
    ///
    /// Returns only after `compose::list` answers, so a daemon that dies on
    /// start still fails this command rather than handing back a prompt.
    #[arg(long, short = 'd')]
    pub detach: bool,

    /// Put a detached daemon's output back on this terminal.
    #[arg(long)]
    pub attach: bool,

    /// Where the daemon records its pid. Defaults to ./compose.pid when the
    /// daemon runs in the background.
    #[arg(long, value_name = "PATH", conflicts_with = "no_pid_file")]
    pub pid_file: Option<PathBuf>,

    /// Write no pid file, not even the default one.
    #[arg(long)]
    pub no_pid_file: bool,
}

/// What an invocation resolved to, after the flag combination is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeCommand {
    /// Serve `compose::*` in the foreground.
    Serve {
        engine_url: String,
        /// Pid file this process writes while it serves, if any.
        pid_file: Option<PathBuf>,
    },
    /// Re-launch in the background and wait until it serves.
    Detach {
        engine_url: String,
        /// What the background daemon will write. Reported, not written here:
        /// the pid in the file is the daemon's own.
        pid_file: Option<PathBuf>,
    },
    /// Follow a detached daemon's log.
    Attach,
}

impl ComposeCli {
    /// Resolves the invocation, rejecting incomplete flag combinations.
    pub fn plan(&self) -> Result<ComposeCommand> {
        if self.attach {
            if self.detach {
                return Err(ComposeError::ConflictingFlags {
                    flags: "--attach and --detach",
                });
            }
            return Ok(ComposeCommand::Attach);
        }

        // The guard, not the flag: the background process is launched with the
        // same argv, so a `-d` still in it must not make it fork again.
        let backgrounded = std::env::var_os(DETACHED_GUARD).is_some();
        if self.detach && !backgrounded {
            return Ok(ComposeCommand::Detach {
                engine_url: self.engine_url(),
                pid_file: self.pid_file_path(true),
            });
        }

        Ok(ComposeCommand::Serve {
            engine_url: self.engine_url(),
            pid_file: self.pid_file_path(backgrounded),
        })
    }

    /// The pid file the serving process should write, if any.
    ///
    /// A named `--pid-file` is honoured wherever the daemon runs, because
    /// asking for one is asking for one. The default applies only in the
    /// background: a foreground `iii compose` is watched by whoever started it
    /// and has no reason to leave a file in their project directory.
    pub fn pid_file_path(&self, backgrounded: bool) -> Option<PathBuf> {
        if self.no_pid_file {
            return None;
        }
        match &self.pid_file {
            Some(path) => Some(path.clone()),
            None if backgrounded => Some(PathBuf::from(DEFAULT_PID_FILE)),
            None => None,
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
