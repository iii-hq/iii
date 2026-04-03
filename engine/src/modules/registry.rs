// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{future::Future, pin::Pin, sync::Arc};

use serde_json::Value;

use crate::{engine::Engine, modules::module::Module};

pub type AdapterFuture<A> = Pin<Box<dyn Future<Output = anyhow::Result<Arc<A>>> + Send>>;

pub struct AdapterRegistration<A: ?Sized + Send + Sync + 'static> {
    pub name: &'static str,
    pub factory: fn(Arc<Engine>, Option<Value>) -> AdapterFuture<A>,
}

pub trait AdapterRegistrationEntry<A: ?Sized + Send + Sync + 'static>:
    Send + Sync + 'static
{
    fn name(&self) -> &'static str;
    fn factory(&self) -> fn(Arc<Engine>, Option<Value>) -> AdapterFuture<A>;
}

impl<A: ?Sized + Send + Sync + 'static> AdapterRegistrationEntry<A> for AdapterRegistration<A> {
    fn name(&self) -> &'static str {
        self.name
    }

    fn factory(&self) -> fn(Arc<Engine>, Option<Value>) -> AdapterFuture<A> {
        self.factory
    }
}

#[macro_export]
macro_rules! register_adapter {
    (<$registration:path> $name:expr, $factory:expr) => {
        ::inventory::submit! {
            $registration {
                name: $name,
                factory: $factory,
            }
        }
    };
    ($registration:path, $name:expr, $factory:expr) => {
        ::inventory::submit! {
            $registration {
                name: $name,
                factory: $factory,
            }
        }
    };
}

pub type ModuleFuture = Pin<Box<dyn Future<Output = anyhow::Result<Box<dyn Module>>> + Send>>;

pub struct ModuleRegistration {
    pub name: &'static str,
    pub factory: fn(Arc<Engine>, Option<Value>) -> ModuleFuture,
    pub is_default: bool,
    pub mandatory: bool,
}

#[macro_export]
macro_rules! register_module {
    ($name:expr, $module:ty, mandatory) => {
        ::inventory::submit! {
            $crate::modules::registry::ModuleRegistration {
                name: $name,
                factory: < $module as $crate::modules::module::Module >::make_module,
                is_default: true,
                mandatory: true,
            }
        }
    };
    ($name:expr, $module:ty, enabled_by_default = $enabled_by_default:expr) => {
        ::inventory::submit! {
            $crate::modules::registry::ModuleRegistration {
                name: $name,
                factory: < $module as $crate::modules::module::Module >::make_module,
                is_default: $enabled_by_default,
                mandatory: false,
            }
        }
    };
    ($name:expr, $module:ty) => {
        ::inventory::submit! {
            $crate::modules::registry::ModuleRegistration {
                name: $name,
                factory: < $module as $crate::modules::module::Module >::make_module,
                is_default: false,
                mandatory: false,
            }
        }
    };
}
inventory::collect!(ModuleRegistration);

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_adapter_factory(
        _engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> AdapterFuture<dyn Send + Sync> {
        Box::pin(async { Ok(Arc::new(()) as Arc<dyn Send + Sync>) })
    }

    #[test]
    fn adapter_registration_name_and_factory() {
        let reg = AdapterRegistration::<dyn Send + Sync> {
            name: "test-adapter",
            factory: dummy_adapter_factory,
        };
        assert_eq!(
            AdapterRegistrationEntry::<dyn Send + Sync>::name(&reg),
            "test-adapter"
        );
        let _f = AdapterRegistrationEntry::<dyn Send + Sync>::factory(&reg);
    }

    fn dummy_module_factory(_engine: Arc<Engine>, _config: Option<Value>) -> ModuleFuture {
        Box::pin(async { unimplemented!() })
    }

    #[test]
    fn module_registration_fields() {
        let reg = ModuleRegistration {
            name: "test-module",
            factory: dummy_module_factory,
            is_default: true,
            mandatory: false,
        };
        assert_eq!(reg.name, "test-module");
        assert!(reg.is_default);
        assert!(!reg.mandatory);
    }
}
