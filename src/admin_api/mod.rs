pub mod functions;
pub mod middleware;

use std::sync::Arc;

use axum::Router;

use crate::engine::Engine;

pub fn create_router(engine: Arc<Engine>) -> Router {
    Router::new()
        .merge(functions::router(engine.clone()))
        .with_state(engine)
}
