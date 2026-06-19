// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod adapters;
mod config;

#[allow(clippy::module_inception)]
mod configuration;

pub mod registry;
/// `pub(crate)`: `expand_value` is reused by the early-boot read of the
/// persisted `iii-observability` entry (engine/src/logging.rs).
pub(crate) mod store;
pub mod structs;
mod trigger;

pub use self::configuration::ConfigurationWorker;
