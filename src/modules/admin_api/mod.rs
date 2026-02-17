// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod config;
pub mod functions;
pub mod middleware;
pub mod module;

pub use config::AdminApiConfig;
pub use module::AdminApiModule;

use std::sync::Arc;

use axum::Router;

use crate::engine::Engine;

pub fn create_router(engine: Arc<Engine>) -> Router {
    functions::router(engine)
}
