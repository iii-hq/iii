// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii worker init` -- scaffold a standalone worker repo from scratch.
//!
//! Uses shared helpers from `scaffolder_core::cli` so behavior stays in sync
//! with `iii project init` (engine/src/cli/project/mod.rs). Worker-specific
//! glue (persisting `.iii/worker.ini`, templating the worker name into
//! `iii.worker.yaml`, the success message) lives here.

use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// Worker directory (positional). Equivalent to --directory.
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// Target directory (defaults to current directory). Takes precedence over the positional name.
    #[arg(short, long)]
    pub directory: Option<String>,

    /// Local directory to use for templates instead of fetching from remote
    /// (for template development and tests).
    #[arg(long = "template-dir")]
    pub template_dir: Option<String>,

    /// Allow scaffolding into a non-empty directory. Re-running init in a
    /// directory with `.iii/worker.ini` is always allowed (idempotent re-init).
    #[arg(long = "allow-non-empty")]
    pub allow_non_empty: bool,
}

impl InitArgs {
    pub(crate) fn target_dir(&self) -> Option<&str> {
        self.directory.as_deref().or(self.name.as_deref())
    }
}

pub async fn run(_args: InitArgs) -> i32 {
    eprintln!("iii worker init: not yet implemented");
    1
}
