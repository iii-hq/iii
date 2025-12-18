mod adapters;
mod config;
mod connection;
mod socket;
#[allow(clippy::module_inception)]
mod streams;
mod trigger;
mod utils;

mod structs;

pub use self::{
    socket::StreamSocketManager,
    streams::StreamCoreModule,
    structs::{StreamIncomingMessage, StreamOutboundMessage, StreamWrapperMessage, Subscription},
};
