use crate::modules::{
    event::EventAdapter,
    registry::{AdapterFuture, AdapterRegistration},
};

pub type EventAdapterFuture = AdapterFuture<dyn EventAdapter>;
pub type EventAdapterRegistration = AdapterRegistration<dyn EventAdapter>;

inventory::collect!(EventAdapterRegistration);
