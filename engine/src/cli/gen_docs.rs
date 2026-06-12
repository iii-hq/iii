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

/// The `console`, `cloud`, and `worker` subcommands are passthrough stubs
/// here (a bare `Vec<String>`); their real command trees live in the
/// dispatched binaries. Link to those binaries' own generated pages instead
/// of rendering an empty `[ARGS]...` section.
fn delegated() -> BTreeMap<String, Delegated> {
    let mut map = BTreeMap::new();
    map.insert(
        "worker".to_string(),
        Delegated {
            link: Some("./iii-worker".to_string()),
            note: "Manage workers (add, remove, list, info). Dispatches to the `iii-worker` \
                   binary; see the linked page for its full surface."
                .to_string(),
        },
    );
    map.insert(
        "console".to_string(),
        Delegated {
            link: Some("./iii-console".to_string()),
            note: "Launch the iii web console. Dispatches to the `iii-console` binary; see the \
                   linked page for its full surface."
                .to_string(),
        },
    );
    map.insert(
        "cloud".to_string(),
        Delegated {
            link: None,
            note: "Manage iii Cloud deployments. Dispatches to the external `iii-cloud` binary, \
                   which is maintained outside this repository; run `iii cloud --help` for its \
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
        "iii trigger".to_string(),
        "<Note>\n  `iii trigger <function> --help` additionally queries a running engine for \
         the function's description and request schema. That output depends on which workers \
         are registered and is not part of this page; see \
         [Functions](../using-iii/functions).\n</Note>"
            .to_string(),
    );
    map
}

pub fn run(cmd: clap::Command, out: Option<&Path>) -> anyhow::Result<()> {
    let meta = PageMeta {
        title: "iii CLI reference".to_string(),
        description: "Every flag, argument, and subcommand of the iii binary, generated from \
                      the CLI definitions in the engine source."
            .to_string(),
        owner: "devrel".to_string(),
        intro: "Reference for the `iii` binary. Running `iii` with no subcommand starts the \
                engine. The same information is available from the binary itself via `iii \
                --help` and `iii <subcommand> --help`. For a guided overview, see \
                [CLI](../using-iii/cli)."
            .to_string(),
        delegated: delegated(),
        mdx_only_notes: mdx_only_notes(),
    };
    iii_clap_docs::write_page(cmd, &meta, out)?;
    Ok(())
}
