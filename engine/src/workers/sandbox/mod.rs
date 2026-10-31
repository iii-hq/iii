// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod config;
mod router;
pub use router::SandboxRouter;

crate::register_worker!(
    "iii-sandbox",
    SandboxRouter,
    description = "Routes sandbox::* in the default namespace to the configured sandbox adapter worker (sandbox-libkrun by default).",
    mandatory
);
