mod adapters;
mod config;
mod socket;

#[allow(clippy::module_inception)]
mod streams;

mod structs;

pub use self::{
    socket::StreamSocketManager,
    streams::StreamCoreModule,
    structs::{StreamIncomingMessage, StreamOutboundMessage, StreamWrapperMessage, Subscription},
};
