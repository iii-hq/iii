use crate::modules::{
    registry::{AdapterFuture, AdapterRegistration},
    streams::adapters::StreamAdapter,
};

pub(crate) type StreamAdapterFuture = AdapterFuture<dyn StreamAdapter>;
pub(crate) type StreamAdapterRegistration = AdapterRegistration<dyn StreamAdapter>;

inventory::collect!(StreamAdapterRegistration);
