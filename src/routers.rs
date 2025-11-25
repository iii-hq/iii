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

    pub async fn get_router(&self, http_method: &str, http_path: &str) -> Option<String> {
        let method = http_method.to_uppercase();
        let key = format!("{}:{}", method, http_path);
        tracing::info!("Looking up router for key: {}", key);
        let routers = self.routers.read().await;
        let router = routers.get(&key);
        match router {
            Some(r) => Some(r.function_path.clone()),
            None => None,
        }
    }

    pub async fn register_router(&self, router: PathRouter) {
        let method = router.http_method.to_uppercase();
        let key = format!("{}:{}", method, router.http_path);
        tracing::info!("Registering router: {}", key);
        self.routers.write().await.insert(key, router);
    }

    pub async fn unregister_router(&self, http_method: &str, http_path: &str) -> bool {
        let key = format!("{}:{}", http_method.to_uppercase(), http_path);
        tracing::info!("Unregistering router: {}", key);
        self.routers.write().await.remove(&key).is_some()
    }
}

#[derive(Debug)]
pub struct PathRouter {
    pub http_path: String,
    pub http_method: String,
    pub function_path: String,
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
