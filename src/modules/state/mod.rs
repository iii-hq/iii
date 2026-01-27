// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

pub mod adapters;
mod config;
#[allow(clippy::module_inception)]
mod state;

pub mod registry;
mod structs;

pub use self::state::StateCoreModule;
