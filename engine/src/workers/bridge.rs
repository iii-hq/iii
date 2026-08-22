// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use iii_sdk::InitOptions;

/// `InitOptions` for an engine-internal bridge adapter connection.
///
/// Several adapters can run inside one engine process. Giving each connection
/// a stable, distinct name prevents the engine from rejecting later adapters
/// as duplicate workers in the same namespace.
pub(crate) fn bridge_init_options(worker_name: &str) -> InitOptions {
    InitOptions {
        metadata: Some(iii_sdk::runtime::WorkerMetadata {
            name: worker_name.to_string(),
            ..Default::default()
        }),
        ..Default::default()
    }
}
