// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use crate::modules::{
    registry::{AdapterFuture, AdapterRegistration},
    streams::adapters::StreamAdapter,
};

pub type StreamAdapterFuture = AdapterFuture<dyn StreamAdapter>;
pub type StreamAdapterRegistration = AdapterRegistration<dyn StreamAdapter>;

inventory::collect!(StreamAdapterRegistration);
