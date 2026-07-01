// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! An in-process pub/sub backend registered only under the `test-adapters`
//! feature (enabled for this crate's own test targets via the self
//! dev-dependency in `Cargo.toml`; off in every normal build). Mechanically a
//! `local` adapter under a second registry name so the `pubsub_configuration_e2e`
//! suite can exercise the adapter hot-swap against a *second* dependency-free
//! backend — the real alternative, `redis`, needs a live server. Its `config` is
//! intentionally open: the suite uses it as an opaque marker/perturbation carrier
//! (`label`, `generation`), all of which this adapter ignores.

use std::sync::Arc;

use serde_json::Value;

use crate::engine::Engine;
use crate::workers::pubsub::{
    PubSubAdapter,
    adapters::local_adapter::LocalAdapter,
    registry::{PubSubAdapterFuture, PubSubAdapterRegistration},
};

fn make_adapter(engine: Arc<Engine>, _config: Option<Value>) -> PubSubAdapterFuture {
    Box::pin(
        async move { Ok(Arc::new(LocalAdapter::new(engine).await?) as Arc<dyn PubSubAdapter>) },
    )
}

crate::register_adapter!(<PubSubAdapterRegistration> name: "memory", make_adapter);
