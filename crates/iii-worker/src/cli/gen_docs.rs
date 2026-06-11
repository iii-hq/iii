// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Hidden `iii worker gen-docs` subcommand: renders this binary's clap tree
//! as the committed MDX CLI reference (docs/next/cli-reference/iii-worker.mdx).
//! CI regenerates the page and fails on diff, so the published reference can
//! never drift from the CLI definitions. See scripts/generate-cli-docs.sh.

use std::collections::BTreeMap;
use std::path::Path;

use iii_clap_docs::PageMeta;

pub fn run(cmd: clap::Command, out: Option<&Path>) -> anyhow::Result<()> {
    let meta = PageMeta {
        title: "iii worker CLI reference".to_string(),
        description: "Every flag, argument, and subcommand of iii worker, generated from the \
                      CLI definitions in the worker runtime source."
            .to_string(),
        owner: "devrel".to_string(),
        intro: "Reference for the `iii worker` command surface. The `iii` binary dispatches \
                `iii worker ...` to the separately installed `iii-worker` runtime; this page \
                documents that runtime's full tree. The dispatcher itself is covered by the \
                [iii CLI reference](./iii). The same information is available from the binary \
                via `iii worker --help` and `iii worker <subcommand> --help`."
            .to_string(),
        delegated: BTreeMap::new(),
    };
    iii_clap_docs::write_page(cmd, &meta, out)?;
    Ok(())
}
