use std::{future::Future, pin::Pin, sync::Arc};

use serde_json::Value;

use crate::engine::Engine;

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

macro_rules! register_adapter {
    (<$registration:path> $class:expr, $factory:expr) => {
        inventory::submit! {
            $registration {
                class: $class,
                factory: $factory,
            }
        }
    };
    ($registration:path, $class:expr, $factory:expr) => {
        inventory::submit! {
            $registration {
                class: $class,
                factory: $factory,
            }
        }
    };
}

pub(crate) use register_adapter;
