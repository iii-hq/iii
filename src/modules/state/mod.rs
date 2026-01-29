pub mod adapters;
mod config;
#[allow(clippy::module_inception)]
mod state;

pub mod registry;
mod structs;
mod trigger;

pub use self::state::StateCoreModule;
