use std::{future::Future, pin::Pin, sync::Arc};

use serde_json::Value;

use crate::{engine::Engine, modules::module::Module};

pub type AdapterFuture<A> = Pin<Box<dyn Future<Output = anyhow::Result<Arc<A>>> + Send>>;

pub struct AdapterRegistration<A: ?Sized + Send + Sync + 'static> {
    pub class: &'static str,
    pub factory: fn(Arc<Engine>, Option<Value>) -> AdapterFuture<A>,
}

pub trait AdapterRegistrationEntry<A: ?Sized + Send + Sync + 'static>:
    Send + Sync + 'static
{
    fn class(&self) -> &'static str;
    fn factory(&self) -> fn(Arc<Engine>, Option<Value>) -> AdapterFuture<A>;
}

impl<A: ?Sized + Send + Sync + 'static> AdapterRegistrationEntry<A> for AdapterRegistration<A> {
    fn class(&self) -> &'static str {
        self.class
    }

    fn factory(&self) -> fn(Arc<Engine>, Option<Value>) -> AdapterFuture<A> {
        self.factory
    }
}

#[macro_export]
macro_rules! register_adapter {
    (<$registration:path> $class:expr, $factory:expr) => {
        ::inventory::submit! {
            $registration {
                class: $class,
                factory: $factory,
            }
        }
    };
    ($registration:path, $class:expr, $factory:expr) => {
        ::inventory::submit! {
            $registration {
                class: $class,
                factory: $factory,
            }
        }
    };
}

pub type ModuleFuture = Pin<Box<dyn Future<Output = anyhow::Result<Box<dyn Module>>> + Send>>;

pub struct ModuleRegistration {
    pub class: &'static str,
    pub factory: fn(Arc<Engine>, Option<Value>) -> ModuleFuture,
    pub is_default: bool,
}

#[macro_export]
macro_rules! register_module {
    ($class:expr, $module:ty, enabled_by_default = $enabled_by_default:expr) => {
        ::inventory::submit! {
            $crate::modules::registry::ModuleRegistration {
                class: $class,
                factory: < $module as $crate::modules::module::Module >::make_module,
                is_default: $enabled_by_default,
            }
        }
    };
    ($class:expr, $module:ty) => {
        ::inventory::submit! {
            $crate::modules::registry::ModuleRegistration {
                class: $class,
                factory: < $module as $crate::modules::module::Module >::make_module,
                is_default: false,
            }
        }
    };
}
inventory::collect!(ModuleRegistration);
