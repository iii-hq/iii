// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Hidden `iii gen-cli-docs` subcommand: renders the engine's clap tree as
//! the committed MDX CLI reference (docs/next/cli-reference/iii.mdx). CI
//! regenerates the page and fails on diff, so the published reference can
//! never drift from the CLI definitions. See scripts/generate-cli-docs.sh.

use std::collections::BTreeMap;
use std::path::Path;

use iii_clap_docs::{Delegated, PageMeta};

/// The `console` and `cloud` subcommands are passthrough stubs
/// here (a bare `Vec<String>`); their real command trees live in the
/// dispatched binaries. Link to those binaries' own sections of the
/// combined page instead of rendering an empty `[ARGS]...` section.
fn delegated() -> BTreeMap<String, Delegated> {
    let mut map = BTreeMap::new();
    map.insert(
        "console".to_string(),
        Delegated {
            link: Some("#iii-console".to_string()),
            note: "Launch the iii web console.".to_string(),
        },
    );
    map.insert(
        "cloud".to_string(),
        Delegated {
            link: None,
            note: "Manage iii Cloud deployments. Dispatches to the external `iii-cloud` binary, \
                   which is temporarily maintained outside this repository; run `iii cloud --help` for its \
                   current surface."
                .to_string(),
        },
    );
    map
}

/// Docs-only callouts appended to specific command sections (keyed by full
/// command path). For text that should ALSO show in terminal `--help`, set
/// clap's `after_long_help` on the command instead; the walker renders it
/// as a `<Note>` automatically.
fn mdx_only_notes() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert(
        "iii compose logs".to_string(),
        "<Note>\n  Without `--follow`, this command prints a recent snapshot and exits. With \
         `--follow`, it long-polls and continues from per-worker cursors. Each worker has a 10 MiB \
         active file and three archives; older output is deleted after rotation, and a cursor older \
         than the retained history resumes from the most recent retained lines with a warning. See \
         [The `compose::*` functions](../using-iii/compose#the-compose-functions) for the remote \
         `compose::logs` fields: `cursors`, `tail`, `stream`, and `wait_ms`.\n</Note>"
            .to_string(),
    );
    map.insert(
        "iii trigger".to_string(),
        "<Note>\n  `iii trigger <function> --help` additionally queries a running engine for \
         the function's description and request schema. That output depends on which workers \
         are registered and is not part of this page; see [Creating Workers / \
         Functions](../creating-workers/functions#attach-request-and-response-schemas).\n</Note>"
            .to_string(),
    );
    map
}

pub fn run(cmd: clap::Command, out: Option<&Path>) -> anyhow::Result<()> {
    let meta = PageMeta {
        title: "CLI reference".to_string(),
        description: "Every flag, argument, and subcommand of the iii CLI, including iii \
                      console, generated from the CLI definitions in source."
            .to_string(),
        owner: "devrel".to_string(),
        intro: "Reference for the `iii` binary and the `iii console` runtime it dispatches \
                to. Running `iii` with no subcommand starts the \
                engine. The same information is available from the binaries themselves via \
                `iii --help` and `iii <subcommand> --help`. For a guided overview, see \
                [CLI](../using-iii/cli)."
            .to_string(),
        delegated: delegated(),
        mdx_only_notes: mdx_only_notes(),
    };
    iii_clap_docs::write_page(cmd, &meta, out)?;
    Ok(())
}
