use std::{collections::HashSet, sync::Arc};

use tokio::sync::RwLock;

use dashmap::DashMap;

#[derive(Default)]
pub struct ServicesRegistry {
    pub services: Arc<RwLock<DashMap<String, Service>>>,
}
impl ServicesRegistry {
    pub fn new() -> Self {
        ServicesRegistry {
            services: Arc::new(RwLock::new(DashMap::new())),
        }
    }

    pub async fn register_service_from_func_path(&self, func_path: &str) {
        let parts: Vec<&str> = func_path.split(".").collect();
        if parts.len() < 2 {
            return;
        }
        let service_name = parts[0].to_string();
        let function_name = parts[1..].join(".");

        if !self.services.read().await.contains_key(&service_name) {
            let service = Service::new(service_name.clone(), "".to_string());
            self.insert_service(service).await;
        }

        self.insert_function_to_service(&service_name, &function_name)
            .await;
    }

    pub async fn insert_service(&self, service: Service) {
        if self.services.read().await.contains_key(&service.name) {
            tracing::warn!(service_name = %service.name, "Service already exists");
        }
        self.services
            .write()
            .await
            .insert(service.name.clone(), service);
    }

    pub async fn _remove_service(&self, service: &Service) {
        self.services.write().await.remove(&service.name);
    }

    pub async fn insert_function_to_service(&self, service_name: &String, function: &str) {
        if let Some(mut service) = self.services.write().await.get_mut(service_name) {
            service.insert_function(function.to_string());
        }
    }
}

#[derive(Debug)]
pub struct Service {
    _id: String,
    name: String,
    functions: HashSet<String>,
}

impl Service {
    pub fn new(name: String, id: String) -> Self {
        Service {
            _id: id,
            name,
            functions: HashSet::new(),
        }
    }

    pub fn insert_function(&mut self, function: String) {
        if function.is_empty() {
            return;
        }
        if self.functions.contains(&function) {
            tracing::warn!(
                function_name = %function,
                service_name = %self.name,
                "Function already exists in service"
            );
        }
        self.functions.insert(function);
    }

    pub fn _remove_function(&mut self, function: &str) {
        self.functions.remove(function);
    }
}
