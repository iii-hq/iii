// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use crate::modules::{
    pubsub::PubSubAdapter,
    registry::{AdapterFuture, AdapterRegistration},
};

pub type PubSubAdapterFuture = AdapterFuture<dyn PubSubAdapter>;
pub type PubSubAdapterRegistration = AdapterRegistration<dyn PubSubAdapter>;

inventory::collect!(PubSubAdapterRegistration);
