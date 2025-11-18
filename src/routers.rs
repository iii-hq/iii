use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct RouterRegistry {
    pub routers: Arc<RwLock<DashMap<String, PathRouter>>>,
}

impl RouterRegistry {
    pub fn new() -> Self {
        Self {
            routers: Arc::new(RwLock::new(DashMap::new())),
        }
    }

    pub async fn register_router(&self, router: PathRouter) {
        let key = format!("{}:{}", router.http_method, router.http_path);
        self.routers.write().await.insert(key, router);
    }
}

#[derive(Debug)]
pub struct PathRouter {
    http_path: String,
    http_method: String,
    function_path: String,
}

impl PathRouter {
    pub fn new(http_path: String, http_method: String, function_path: String) -> Self {
        Self {
            http_path,
            http_method,
            function_path,
        }
    }
}
