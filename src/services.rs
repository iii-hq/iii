// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{any::Any, collections::HashSet, sync::Arc};

use dashmap::DashMap;
#[derive(Default)]
pub struct ServicesRegistry {
    pub services: Arc<DashMap<String, Service>>,
    module_services: Arc<DashMap<String, Arc<dyn Any + Send + Sync>>>,
}
impl ServicesRegistry {
    pub fn new() -> Self {
        ServicesRegistry {
            services: Arc::new(DashMap::new()),
            module_services: Arc::new(DashMap::new()),
        }
    }

    pub fn register_service<T: Send + Sync + 'static>(&self, name: &str, service: Arc<T>) {
        self.module_services
            .insert(name.to_string(), service as Arc<dyn Any + Send + Sync>);
    }

    pub fn get_service<T: Send + Sync + 'static>(&self, name: &str) -> Option<Arc<T>> {
        self.module_services
            .get(name)?
            .value()
            .clone()
            .downcast::<T>()
            .ok()
    }

    pub fn remove_function_from_services(&self, func_path: &str) {
        let service_name = match Self::get_service_name_from_func_path(func_path) {
            Some(name) => name,
            None => {
                tracing::warn!(func_path = %func_path, "Invalid function path format");
                return;
            }
        };
        let function_name = match Self::get_function_name_from_func_path(func_path) {
            Some(name) => name,
            None => {
                tracing::warn!(func_path = %func_path, "Invalid function path format");
                return;
            }
        };

        let mut should_remove_service = false;
        if let Some(mut service) = self.services.get_mut(&service_name) {
            tracing::debug!(
                service_name = %service_name,
                function_name = %function_name,
                "Removing function from service"
            );

            service.remove_function_from_service(&function_name);
            should_remove_service = service.functions.is_empty();
        }

        if should_remove_service {
            tracing::debug!(
                service_name = %service_name,
                "Removing service as it has no more functions"
            );
            self.services.remove(&service_name);
        }
    }

    fn get_service_name_from_func_path(func_path: &str) -> Option<String> {
        let parts: Vec<&str> = func_path.split(".").collect();
        if parts.len() < 2 {
            return None;
        }
        Some(parts[0].to_string())
    }

    fn get_function_name_from_func_path(func_path: &str) -> Option<String> {
        let parts: Vec<&str> = func_path.split(".").collect();
        if parts.len() < 2 {
            return None;
        }
        Some(parts[1..].join("."))
    }

    pub fn register_service_from_func_path(&self, func_path: &str) {
        let parts: Vec<&str> = func_path.split(".").collect();
        if parts.len() < 2 {
            return;
        }
        let service_name = parts[0].to_string();
        let function_name = parts[1..].join(".");

        if !self.services.contains_key(&service_name) {
            let service = Service::new(service_name.clone(), "".to_string());
            self.insert_service(service);
        }

        self.insert_function_to_service(&service_name, &function_name);
    }

    pub fn insert_service(&self, service: Service) {
        if self.services.contains_key(&service.name) {
            tracing::warn!(service_name = %service.name, "Service already exists");
        }
        self.services.insert(service.name.clone(), service);
    }

    pub fn _remove_service(&self, service: &Service) {
        self.services.remove(&service.name);
    }

    pub fn insert_function_to_service(&self, service_name: &String, function: &str) {
        if let Some(mut service) = self.services.get_mut(service_name) {
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

    pub fn remove_function_from_service(&mut self, function: &str) {
        self.functions.remove(function);
    }
}
