use crate::modules::{
    event::EventAdapter,
    registry::{AdapterFuture, AdapterRegistration},
};

pub(crate) type EventAdapterFuture = AdapterFuture<dyn EventAdapter>;
pub(crate) type EventAdapterRegistration = AdapterRegistration<dyn EventAdapter>;

inventory::collect!(EventAdapterRegistration);
