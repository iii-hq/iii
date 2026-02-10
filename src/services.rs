// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashSet, sync::Arc};

use dashmap::DashMap;
#[derive(Default)]
pub struct ServicesRegistry {
    pub services: Arc<DashMap<String, Service>>,
}
impl ServicesRegistry {
    pub fn new() -> Self {
        ServicesRegistry {
            services: Arc::new(DashMap::new()),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_services_registry_new() {
        let registry = ServicesRegistry::new();
        assert_eq!(registry.services.len(), 0);
    }

    #[test]
    fn test_register_service_from_func_path_simple() {
        let registry = ServicesRegistry::new();
        registry.register_service_from_func_path("svc.fn1");

        assert!(registry.services.contains_key("svc"));
        let service = registry.services.get("svc").unwrap();
        assert!(service.functions.contains("fn1"));
    }

    #[test]
    fn test_register_service_from_func_path_multi_part() {
        let registry = ServicesRegistry::new();
        registry.register_service_from_func_path("svc.fn1.sub");

        assert!(registry.services.contains_key("svc"));
        let service = registry.services.get("svc").unwrap();
        assert!(service.functions.contains("fn1.sub"));
    }

    #[test]
    fn test_register_service_from_func_path_no_dot() {
        let registry = ServicesRegistry::new();
        registry.register_service_from_func_path("noDot");

        assert_eq!(registry.services.len(), 0);
    }

    #[test]
    fn test_register_service_from_func_path_multiple_functions() {
        let registry = ServicesRegistry::new();
        registry.register_service_from_func_path("svc.fn1");
        registry.register_service_from_func_path("svc.fn2");

        let service = registry.services.get("svc").unwrap();
        assert_eq!(service.functions.len(), 2);
        assert!(service.functions.contains("fn1"));
        assert!(service.functions.contains("fn2"));
    }

    #[test]
    fn test_register_service_from_func_path_multiple_services() {
        let registry = ServicesRegistry::new();
        registry.register_service_from_func_path("svc1.fn1");
        registry.register_service_from_func_path("svc2.fn1");

        assert_eq!(registry.services.len(), 2);
        assert!(registry.services.contains_key("svc1"));
        assert!(registry.services.contains_key("svc2"));
    }

    #[test]
    fn test_remove_function_from_services_simple() {
        let registry = ServicesRegistry::new();
        registry.register_service_from_func_path("svc.fn1");
        registry.remove_function_from_services("svc.fn1");

        assert!(!registry.services.contains_key("svc"));
    }

    #[test]
    fn test_remove_function_from_services_keeps_service_with_other_functions() {
        let registry = ServicesRegistry::new();
        registry.register_service_from_func_path("svc.fn1");
        registry.register_service_from_func_path("svc.fn2");
        registry.remove_function_from_services("svc.fn1");

        assert!(registry.services.contains_key("svc"));
        let service = registry.services.get("svc").unwrap();
        assert!(!service.functions.contains("fn1"));
        assert!(service.functions.contains("fn2"));
    }

    #[test]
    fn test_remove_function_from_services_invalid_path() {
        let registry = ServicesRegistry::new();
        registry.register_service_from_func_path("svc.fn1");
        registry.remove_function_from_services("noDot");

        assert!(registry.services.contains_key("svc"));
    }

    #[test]
    fn test_remove_function_from_services_nonexistent_service() {
        let registry = ServicesRegistry::new();
        registry.remove_function_from_services("nonexistent.fn1");

        assert_eq!(registry.services.len(), 0);
    }

    #[test]
    fn test_insert_service() {
        let registry = ServicesRegistry::new();
        let service = Service::new("test-service".to_string(), "id-1".to_string());
        registry.insert_service(service);

        assert!(registry.services.contains_key("test-service"));
    }

    #[test]
    fn test_insert_service_overwrites_existing() {
        let registry = ServicesRegistry::new();
        let service1 = Service::new("test-service".to_string(), "id-1".to_string());
        let service2 = Service::new("test-service".to_string(), "id-2".to_string());
        registry.insert_service(service1);
        registry.insert_service(service2);

        assert_eq!(registry.services.len(), 1);
    }

    #[test]
    fn test_insert_function_to_service() {
        let registry = ServicesRegistry::new();
        let service = Service::new("test-service".to_string(), "id-1".to_string());
        registry.insert_service(service);
        registry.insert_function_to_service(&"test-service".to_string(), "fn1");

        let service = registry.services.get("test-service").unwrap();
        assert!(service.functions.contains("fn1"));
    }

    #[test]
    fn test_insert_function_to_service_nonexistent() {
        let registry = ServicesRegistry::new();
        registry.insert_function_to_service(&"nonexistent".to_string(), "fn1");

        assert_eq!(registry.services.len(), 0);
    }

    #[test]
    fn test_service_new() {
        let service = Service::new("test-service".to_string(), "id-1".to_string());
        assert_eq!(service.name, "test-service");
        assert_eq!(service.functions.len(), 0);
    }

    #[test]
    fn test_service_insert_function() {
        let mut service = Service::new("test-service".to_string(), "id-1".to_string());
        service.insert_function("fn1".to_string());
        assert!(service.functions.contains("fn1"));
    }

    #[test]
    fn test_service_insert_function_empty_string() {
        let mut service = Service::new("test-service".to_string(), "id-1".to_string());
        service.insert_function("".to_string());
        assert_eq!(service.functions.len(), 0);
    }

    #[test]
    fn test_service_insert_function_duplicate() {
        let mut service = Service::new("test-service".to_string(), "id-1".to_string());
        service.insert_function("fn1".to_string());
        service.insert_function("fn1".to_string());
        assert_eq!(service.functions.len(), 1);
        assert!(service.functions.contains("fn1"));
    }

    #[test]
    fn test_service_remove_function_from_service() {
        let mut service = Service::new("test-service".to_string(), "id-1".to_string());
        service.insert_function("fn1".to_string());
        service.remove_function_from_service("fn1");
        assert!(!service.functions.contains("fn1"));
    }

    #[test]
    fn test_service_remove_function_from_service_nonexistent() {
        let mut service = Service::new("test-service".to_string(), "id-1".to_string());
        service.remove_function_from_service("nonexistent");
        assert_eq!(service.functions.len(), 0);
    }

    #[test]
    fn test_get_service_name_from_func_path() {
        assert_eq!(
            ServicesRegistry::get_service_name_from_func_path("svc.fn1"),
            Some("svc".to_string())
        );
        assert_eq!(
            ServicesRegistry::get_service_name_from_func_path("svc.fn1.sub"),
            Some("svc".to_string())
        );
        assert_eq!(
            ServicesRegistry::get_service_name_from_func_path("noDot"),
            None
        );
        assert_eq!(ServicesRegistry::get_service_name_from_func_path(""), None);
    }

    #[test]
    fn test_get_function_name_from_func_path() {
        assert_eq!(
            ServicesRegistry::get_function_name_from_func_path("svc.fn1"),
            Some("fn1".to_string())
        );
        assert_eq!(
            ServicesRegistry::get_function_name_from_func_path("svc.fn1.sub"),
            Some("fn1.sub".to_string())
        );
        assert_eq!(
            ServicesRegistry::get_function_name_from_func_path("svc.fn1.sub.more"),
            Some("fn1.sub.more".to_string())
        );
        assert_eq!(
            ServicesRegistry::get_function_name_from_func_path("noDot"),
            None
        );
        assert_eq!(ServicesRegistry::get_function_name_from_func_path(""), None);
    }
}
