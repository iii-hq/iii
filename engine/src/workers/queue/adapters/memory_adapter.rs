// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! An in-process queue transport registered only under the `test-adapters`
//! feature (enabled for this crate's own test targets via the self
//! dev-dependency in `Cargo.toml`; off in every normal build). Mechanically a
//! `builtin` adapter under a second registry name so the
//! `queue_configuration_e2e` suite can exercise the adapter hot-swap against a
//! *second* dependency-free backend — the real alternatives (`redis`,
//! `rabbitmq`) need a live server. Its `config` is intentionally open: the suite
//! uses it as an opaque marker/perturbation carrier.

use std::sync::Arc;

use serde_json::Value;

use crate::engine::Engine;
use crate::workers::queue::registry::{QueueAdapterFuture, QueueAdapterRegistration};

fn make_adapter(engine: Arc<Engine>, config: Option<Value>) -> QueueAdapterFuture {
    super::builtin::make_adapter(engine, config)
}

crate::register_adapter!(<QueueAdapterRegistration> name: "memory", make_adapter);
