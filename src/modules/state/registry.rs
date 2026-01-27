// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use crate::modules::{
    registry::{AdapterFuture, AdapterRegistration},
    state::adapters::StateAdapter,
};

pub type StateAdapterFuture = AdapterFuture<dyn StateAdapter>;
pub type StateAdapterRegistration = AdapterRegistration<dyn StateAdapter>;

inventory::collect!(StateAdapterRegistration);
