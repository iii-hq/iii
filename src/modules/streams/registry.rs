use crate::modules::{
    registry::{AdapterFuture, AdapterRegistration},
    streams::adapters::StreamAdapter,
};

pub type StreamAdapterFuture = AdapterFuture<dyn StreamAdapter>;
pub type StreamAdapterRegistration = AdapterRegistration<dyn StreamAdapter>;

inventory::collect!(StreamAdapterRegistration);
