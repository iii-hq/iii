use crate::modules::{
    registry::{AdapterFuture, AdapterRegistration},
    state::adapters::StateAdapter,
};

pub type StateAdapterFuture = AdapterFuture<dyn StateAdapter>;
pub type StateAdapterRegistration = AdapterRegistration<dyn StateAdapter>;

inventory::collect!(StateAdapterRegistration);
