// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! iii-supervisor library facade.
//!
//! The binary (`src/main.rs`) is thin: parse args, hand off to the
//! pieces exposed here. Tests and host-side integrators import these
//! modules directly — notably `protocol` is consumed by `iii-worker`'s
//! `supervisor_ctl` for wire-compatible RPC.

pub mod child;
pub mod control;
pub mod protocol;
