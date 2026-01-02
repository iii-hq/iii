use crate::modules::{
    observability::LoggerAdapter,
    registry::{AdapterFuture, AdapterRegistration},
};

pub type LoggerAdapterFuture = AdapterFuture<dyn LoggerAdapter>;
pub type LoggerAdapterRegistration = AdapterRegistration<dyn LoggerAdapter>;

inventory::collect!(LoggerAdapterRegistration);
