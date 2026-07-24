// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{any::Any, collections::HashSet, sync::Arc};

use dashmap::DashMap;
#[derive(Default)]
pub struct ServicesRegistry {
    /// Keyed by `(namespace, service_name)`. The same service name may exist
    /// once per namespace.
    pub services: Arc<DashMap<(String, String), Service>>,
    /// Module services are engine-global singletons (the HTTP module, the
    /// stream module, ...), not per-namespace worker state — they stay keyed by
    /// name alone.
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

    pub fn get(&self, namespace: &str, service_name: &str) -> Option<Service> {
        self.services
            .get(&(namespace.to_string(), service_name.to_string()))
            .map(|entry| entry.value().clone())
    }

    pub fn remove_function_from_services(&self, namespace: &str, function_id: &str) {
        let service_name = match Self::get_service_name_from_function_id(function_id) {
            Some(name) => name,
            None => {
                tracing::warn!(function_id = %function_id, "Invalid function id format");
                return;
            }
        };
        let function_name = match Self::get_function_name_from_function_id(function_id) {
            Some(name) => name,
            None => {
                tracing::warn!(function_id = %function_id, "Invalid function id format");
                return;
            }
        };

        let key = (namespace.to_string(), service_name.clone());

        let mut should_remove_service = false;
        if let Some(mut service) = self.services.get_mut(&key) {
            tracing::debug!(
                namespace = %namespace,
                service_name = %service_name,
                function_name = %function_name,
                "Removing function from service"
            );

            service.remove_function_from_service(&function_name);
            should_remove_service = service.functions.is_empty();
        }

        if should_remove_service {
            tracing::debug!(
                namespace = %namespace,
                service_name = %service_name,
                "Removing service as it has no more functions"
            );
            self.services.remove(&key);
        }
    }

    fn get_service_name_from_function_id(function_id: &str) -> Option<String> {
        let parts: Vec<&str> = function_id.split("::").collect();
        if parts.len() < 2 {
            return None;
        }
        Some(parts[0].to_string())
    }

    fn get_function_name_from_function_id(function_id: &str) -> Option<String> {
        let parts: Vec<&str> = function_id.split("::").collect();
        if parts.len() < 2 {
            return None;
        }
        Some(parts[1..].join("::"))
    }

    pub fn register_service_from_function_id(&self, namespace: &str, function_id: &str) {
        let Some(service_name) = Self::get_service_name_from_function_id(function_id) else {
            return;
        };
        let Some(function_name) = Self::get_function_name_from_function_id(function_id) else {
            return;
        };

        if !self
            .services
            .contains_key(&(namespace.to_string(), service_name.clone()))
        {
            let service = Service::new(service_name.clone(), "".to_string());
            self.insert_service(namespace, service);
        }

        self.insert_function_to_service(namespace, &service_name, &function_name);
    }

    pub fn insert_service(&self, namespace: &str, service: Service) {
        let key = (namespace.to_string(), service.name.clone());
        if self.services.contains_key(&key) {
            tracing::warn!(
                namespace = %namespace,
                service_name = %service.name,
                "Service already exists"
            );
        }
        self.services.insert(key, service);
    }

    pub fn insert_function_to_service(&self, namespace: &str, service_name: &str, function: &str) {
        if let Some(mut service) = self
            .services
            .get_mut(&(namespace.to_string(), service_name.to_string()))
        {
            service.insert_function(function.to_string());
        }
    }
}

#[derive(Debug, Clone)]
pub struct Service {
    _id: String,
    name: String,
    pub parent_service_id: Option<String>,
    functions: HashSet<String>,
}

impl Service {
    pub fn new(name: String, id: String) -> Self {
        Service {
            _id: id,
            name,
            parent_service_id: None,
            functions: HashSet::new(),
        }
    }

    pub fn with_parent(name: String, id: String, parent_service_id: Option<String>) -> Self {
        Service {
            _id: id,
            name,
            parent_service_id,
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
    use crate::protocol::DEFAULT_NAMESPACE;

    // =========================================================================
    // Service struct tests
    // =========================================================================

    #[test]
    fn service_new() {
        let svc = Service::new("my_svc".to_string(), "id1".to_string());
        assert_eq!(svc.name, "my_svc");
        assert!(svc.functions.is_empty());
    }

    #[test]
    fn service_insert_function() {
        let mut svc = Service::new("svc".to_string(), "".to_string());
        svc.insert_function("func_a".to_string());
        assert!(svc.functions.contains("func_a"));
    }

    #[test]
    fn service_insert_empty_function_name_is_ignored() {
        let mut svc = Service::new("svc".to_string(), "".to_string());
        svc.insert_function("".to_string());
        assert!(svc.functions.is_empty());
    }

    #[test]
    fn service_insert_duplicate_function_still_exists() {
        let mut svc = Service::new("svc".to_string(), "".to_string());
        svc.insert_function("func_a".to_string());
        svc.insert_function("func_a".to_string()); // duplicate, should not panic
        assert!(svc.functions.contains("func_a"));
        assert_eq!(svc.functions.len(), 1);
    }

    #[test]
    fn service_remove_function() {
        let mut svc = Service::new("svc".to_string(), "".to_string());
        svc.insert_function("func_a".to_string());
        svc.remove_function_from_service("func_a");
        assert!(!svc.functions.contains("func_a"));
    }

    #[test]
    fn service_remove_nonexistent_function_does_not_panic() {
        let mut svc = Service::new("svc".to_string(), "".to_string());
        svc.remove_function_from_service("nonexistent");
    }

    // =========================================================================
    // ServicesRegistry tests
    // =========================================================================

    #[test]
    fn registry_new_is_empty() {
        let reg = ServicesRegistry::new();
        assert!(reg.services.is_empty());
    }

    #[test]
    fn registry_default_is_empty() {
        let reg = ServicesRegistry::default();
        assert!(reg.services.is_empty());
    }

    #[test]
    fn registry_register_and_get_module_service() {
        let reg = ServicesRegistry::new();
        let service_data = Arc::new(42u32);
        reg.register_service("my_svc", service_data);
        let retrieved: Option<Arc<u32>> = reg.get_service("my_svc");
        assert_eq!(*retrieved.unwrap(), 42);
    }

    #[test]
    fn registry_get_nonexistent_module_service_returns_none() {
        let reg = ServicesRegistry::new();
        let result: Option<Arc<u32>> = reg.get_service("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn registry_get_service_wrong_type_returns_none() {
        let reg = ServicesRegistry::new();
        let service_data = Arc::new(42u32);
        reg.register_service("my_svc", service_data);
        // Try to get as a different type
        let result: Option<Arc<String>> = reg.get_service("my_svc");
        assert!(result.is_none());
    }

    #[test]
    fn registry_insert_service() {
        let reg = ServicesRegistry::new();
        let svc = Service::new("svc_a".to_string(), "id_a".to_string());
        reg.insert_service(DEFAULT_NAMESPACE, svc);
        assert!(
            reg.services
                .contains_key(&(DEFAULT_NAMESPACE.to_string(), "svc_a".to_string()))
        );
    }

    #[test]
    fn distinct_namespaces_coexist() {
        let reg = ServicesRegistry::new();
        reg.insert_service(
            "orders",
            Service::new("state".to_string(), "s1".to_string()),
        );
        reg.insert_service(
            "analytics",
            Service::new("state".to_string(), "s2".to_string()),
        );
        assert!(reg.get("orders", "state").is_some());
        assert!(reg.get("analytics", "state").is_some());
    }

    #[test]
    fn registry_insert_function_to_service() {
        let reg = ServicesRegistry::new();
        let svc = Service::new("svc".to_string(), "".to_string());
        reg.insert_service(DEFAULT_NAMESPACE, svc);
        reg.insert_function_to_service(DEFAULT_NAMESPACE, "svc", "func");
        let entry = reg.get(DEFAULT_NAMESPACE, "svc").unwrap();
        assert!(entry.functions.contains("func"));
    }

    #[test]
    fn registry_insert_function_to_nonexistent_service() {
        let reg = ServicesRegistry::new();
        // Should not panic
        reg.insert_function_to_service(DEFAULT_NAMESPACE, "nonexistent", "func");
    }

    // =========================================================================
    // get_service_name_from_function_id / get_function_name_from_function_id
    // =========================================================================

    #[test]
    fn get_service_name_valid() {
        let result = ServicesRegistry::get_service_name_from_function_id("service::function");
        assert_eq!(result, Some("service".to_string()));
    }

    #[test]
    fn get_service_name_deep_path() {
        let result = ServicesRegistry::get_service_name_from_function_id("service::sub::function");
        assert_eq!(result, Some("service".to_string()));
    }

    #[test]
    fn get_service_name_no_dot() {
        let result = ServicesRegistry::get_service_name_from_function_id("noDot");
        assert_eq!(result, None);
    }

    #[test]
    fn get_function_name_valid() {
        let result = ServicesRegistry::get_function_name_from_function_id("service::function");
        assert_eq!(result, Some("function".to_string()));
    }

    #[test]
    fn get_function_name_deep_path() {
        let result = ServicesRegistry::get_function_name_from_function_id("service::sub::function");
        assert_eq!(result, Some("sub::function".to_string()));
    }

    #[test]
    fn get_function_name_no_dot() {
        let result = ServicesRegistry::get_function_name_from_function_id("noDot");
        assert_eq!(result, None);
    }

    // =========================================================================
    // register_service_from_function_id
    // =========================================================================

    #[test]
    fn register_service_from_function_id_creates_service_and_function() {
        let reg = ServicesRegistry::new();
        reg.register_service_from_function_id(DEFAULT_NAMESPACE, "my_svc::my_func");
        assert!(
            reg.services
                .contains_key(&(DEFAULT_NAMESPACE.to_string(), "my_svc".to_string()))
        );
        let entry = reg.get(DEFAULT_NAMESPACE, "my_svc").unwrap();
        assert!(entry.functions.contains("my_func"));
    }

    #[test]
    fn register_service_from_function_id_adds_to_existing_service() {
        let reg = ServicesRegistry::new();
        reg.register_service_from_function_id(DEFAULT_NAMESPACE, "svc::func_a");
        reg.register_service_from_function_id(DEFAULT_NAMESPACE, "svc::func_b");
        let entry = reg.get(DEFAULT_NAMESPACE, "svc").unwrap();
        assert!(entry.functions.contains("func_a"));
        assert!(entry.functions.contains("func_b"));
    }

    #[test]
    fn register_service_from_function_id_invalid_id_no_panic() {
        let reg = ServicesRegistry::new();
        reg.register_service_from_function_id(DEFAULT_NAMESPACE, "no_dot");
        assert!(reg.services.is_empty());
    }

    // =========================================================================
    // remove_function_from_services
    // =========================================================================

    #[test]
    fn remove_function_removes_function_from_service() {
        let reg = ServicesRegistry::new();
        reg.register_service_from_function_id(DEFAULT_NAMESPACE, "svc::func_a");
        reg.register_service_from_function_id(DEFAULT_NAMESPACE, "svc::func_b");
        reg.remove_function_from_services(DEFAULT_NAMESPACE, "svc::func_a");
        let entry = reg.get(DEFAULT_NAMESPACE, "svc").unwrap();
        assert!(!entry.functions.contains("func_a"));
        assert!(entry.functions.contains("func_b"));
    }

    #[test]
    fn remove_function_removes_service_when_empty() {
        let reg = ServicesRegistry::new();
        reg.register_service_from_function_id(DEFAULT_NAMESPACE, "svc::func_a");
        reg.remove_function_from_services(DEFAULT_NAMESPACE, "svc::func_a");
        assert!(
            !reg.services
                .contains_key(&(DEFAULT_NAMESPACE.to_string(), "svc".to_string()))
        );
    }

    #[test]
    fn remove_function_invalid_id_no_panic() {
        let reg = ServicesRegistry::new();
        reg.remove_function_from_services(DEFAULT_NAMESPACE, "no_dot");
    }

    #[test]
    fn remove_function_nonexistent_service_no_panic() {
        let reg = ServicesRegistry::new();
        reg.remove_function_from_services(DEFAULT_NAMESPACE, "nonexistent::func");
    }
}
