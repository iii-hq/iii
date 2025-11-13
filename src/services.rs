use std::collections::HashSet;

use dashmap::DashMap;

#[derive(Default)]
pub struct ServicesRegistry {
    pub services: DashMap<String, Service>,
}
impl ServicesRegistry {
    pub fn new() -> Self {
        ServicesRegistry {
            services: DashMap::new(),
        }
    }

    pub fn insert_service(&mut self, service: Service) {
        if self.services.contains_key(&service.name) {
            println!("Service {} already exists", service.name);
        }
        self.services.insert(service.name.clone(), service);
    }

    pub fn remove_service(&mut self, service: &Service) {
        self.services.remove(&service.name);
    }

    pub fn insert_function_to_service(&mut self, service_name: &String, function: String) {
        if let Some(mut service) = self.services.get_mut(service_name) {
            service.insert_function(function);
        }
    }
}

#[derive(Debug)]
pub struct Service {
    id: String,
    name: String,
    functions: HashSet<String>,
}

impl Service {
    pub fn new(name: String, id: String) -> Self {
        Service {
            id,
            name,
            functions: HashSet::new(),
        }
    }

    pub fn insert_function(&mut self, function: String) {
        if function.is_empty() {
            return;
        }
        if self.functions.contains(&function) {
            println!(
                "Function {} already exists in service {}",
                function, self.name
            );
        }
        self.functions.insert(function);
    }

    pub fn remove_function(&mut self, function: &String) {
        self.functions.remove(function);
    }
}
