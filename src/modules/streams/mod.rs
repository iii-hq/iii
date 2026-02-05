// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod adapters;
mod config;
mod connection;
mod socket;
#[allow(clippy::module_inception)]
mod streams;
mod trigger;
mod utils;

pub mod registry;
mod structs;

pub use self::{
    socket::StreamSocketManager,
    streams::StreamCoreModule,
    structs::{
        StreamIncomingMessage, StreamMetadata, StreamOutboundMessage, StreamWrapperMessage,
        Subscription,
    },
};
