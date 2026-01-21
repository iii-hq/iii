use crate::modules::{
    pubsub::PubSubAdapter,
    registry::{AdapterFuture, AdapterRegistration},
};

pub type PubSubAdapterFuture = AdapterFuture<dyn PubSubAdapter>;
pub type PubSubAdapterRegistration = AdapterRegistration<dyn PubSubAdapter>;

inventory::collect!(PubSubAdapterRegistration);
