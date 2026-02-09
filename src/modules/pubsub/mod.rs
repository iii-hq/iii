// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

mod adapters;
mod config;
#[allow(clippy::module_inception)]
mod pubsub;
pub mod registry;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

pub use self::pubsub::PubSubCoreModule;
pub use self::pubsub::PubSubInput;

#[async_trait::async_trait]
pub trait PubSubAdapter: Send + Sync + 'static {
    async fn publish(&self, topic: &str, pubsub_data: Value);
    async fn subscribe(&self, topic: &str, id: &str, function_id: &str);
    async fn unsubscribe(&self, topic: &str, id: &str);
}

#[derive(Serialize, Deserialize)]
pub struct SubscribeTrigger {
    pub topic: String,
}
