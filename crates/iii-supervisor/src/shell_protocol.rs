// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Re-export of `iii_shell_proto` so consumers that depend on
//! iii-supervisor (notably iii-init's guest dispatcher) can access the
//! shell-channel wire types without pulling the standalone crate
//! directly.

pub use iii_shell_proto::*;
