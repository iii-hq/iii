// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii compose` command surface.
//!
//! Two modes: a foreground daemon bound to one compose file for its whole
//! lifetime, and an offline `validate` that needs no engine. Argument parsing is
//! separated from execution ([`ComposeCli::plan`]) so mode selection is
//! testable without touching a filesystem or a socket.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::error::{ComposeError, Result};

/// Engine address used when neither `--engine` nor `III_URL` is set.
pub const DEFAULT_ENGINE_URL: &str = "ws://127.0.0.1:49134";

#[derive(Args, Debug, Clone)]
pub struct ComposeCli {
    #[command(subcommand)]
    pub action: Option<ComposeAction>,

    /// Daemon identity. Required in daemon mode: it names the daemon in the
    /// engine and is the `id=` every remote `compose::*` call must match.
    #[arg(long, global = true, value_name = "ID")]
    pub id: Option<String>,

    /// Engine WebSocket address. Falls back to III_URL, then
    /// ws://127.0.0.1:49134.
    #[arg(long, global = true, value_name = "URL")]
    pub engine: Option<String>,

    /// Namespace the project's workers register under. Defaults to a
    /// deterministic namespace derived from the project name and compose path.
    #[arg(long, global = true, value_name = "NS")]
    pub namespace: Option<String>,

    /// Path to the worker-compose.yaml this invocation is bound to.
    #[arg(long, short = 'f', global = true, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Bring the project up as soon as the daemon connects, then keep serving.
    /// The daemon exits with a non-zero code if that first `up` fails, which is
    /// what makes compose usable from a script or a CI job.
    #[arg(long)]
    pub up: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ComposeAction {
    /// Validate a compose project without contacting an engine
    Validate,
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
    Daemon {
        id: String,
        file: PathBuf,
        engine_url: String,
        namespace: Option<String>,
        /// Start the project immediately and fail the process if it does not
        /// come up.
        up_on_start: bool,
    },
}

impl ComposeCli {
    /// Resolves the invocation, rejecting incomplete flag combinations.
    pub fn plan(&self) -> Result<ComposeCommand> {
        let file = self
            .file
            .clone()
            .ok_or(ComposeError::MissingFlag { flag: "--file" })?;

        match self.action {
            Some(ComposeAction::Validate) => Ok(ComposeCommand::Validate {
                file,
                namespace: self.namespace.clone(),
            }),
            None => {
                let id = self
                    .id
                    .clone()
                    .filter(|id| !id.trim().is_empty())
                    .ok_or(ComposeError::MissingFlag { flag: "--id" })?;
                Ok(ComposeCommand::Daemon {
                    id,
                    file,
                    engine_url: self.engine_url(),
                    namespace: self.namespace.clone(),
                    up_on_start: self.up,
                })
            }
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
