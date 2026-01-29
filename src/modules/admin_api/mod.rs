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
