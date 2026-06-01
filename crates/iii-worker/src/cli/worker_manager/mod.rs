// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod adapter;
pub mod libkrun;
pub mod oci;
pub mod platform;
pub mod state;

use std::sync::Arc;

use self::adapter::RuntimeAdapter;

/// Create the runtime adapter.
pub fn create_adapter(_runtime: &str) -> Arc<dyn RuntimeAdapter> {
    Arc::new(libkrun::LibkrunAdapter::new())
}
