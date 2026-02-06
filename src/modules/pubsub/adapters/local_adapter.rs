// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    engine::{Engine, EngineTrait},
    modules::pubsub::{
        PubSubAdapter,
        registry::{PubSubAdapterFuture, PubSubAdapterRegistration},
    },
};

type TopicName = String;
type SubscriptionId = String;
type FunctionPath = String;

pub struct LocalAdapter {
    subscriptions: Arc<RwLock<HashMap<TopicName, HashMap<SubscriptionId, FunctionPath>>>>,
    engine: Arc<Engine>,
}

impl LocalAdapter {
    pub async fn new(engine: Arc<Engine>) -> anyhow::Result<Self> {
        Ok(Self {
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            engine,
        })
    }
}

fn make_adapter(engine: Arc<Engine>, _config: Option<Value>) -> PubSubAdapterFuture {
    Box::pin(
        async move { Ok(Arc::new(LocalAdapter::new(engine).await?) as Arc<dyn PubSubAdapter>) },
    )
}

crate::register_adapter!(<PubSubAdapterRegistration> "modules::pubsub::LocalAdapter", make_adapter);

#[async_trait]
impl PubSubAdapter for LocalAdapter {
    async fn publish(&self, topic: &str, event_data: Value) {
        let topic = topic.to_string();
        let event_data = event_data.clone();
        let subscriptions = Arc::clone(&self.subscriptions);
        let subs = subscriptions.read().await;

        if let Some(sub_info) = subs.get(&topic) {
            for (_id, function_id) in sub_info.iter() {
                tracing::debug!(function_id = %function_id, topic = %topic, "Event: Invoking function");
                let function_id = function_id.clone();
                let event_data = event_data.clone();
                let engine = Arc::clone(&self.engine);

                tokio::spawn(async move {
                    let _ = engine.invoke_function(&function_id, event_data).await;
                });
            }
        } else {
            tracing::debug!(topic = %topic, "Event: No subscriptions found");
        }
    }

    async fn subscribe(&self, topic: &str, id: &str, function_id: &str) {
        let topic = topic.to_string();
        let id = id.to_string();
        let function_id = function_id.to_string();
        let mut subs = self.subscriptions.write().await;

        subs.entry(topic)
            .or_insert_with(HashMap::new)
            .insert(id, function_id);
    }

    async fn unsubscribe(&self, topic: &str, id: &str) {
        tracing::debug!(topic = %topic, id = %id, "Unsubscribing from PubSub topic");

        let topic = topic.to_string();
        let id = id.to_string();
        let mut subs = self.subscriptions.write().await;

        if let Some(mut sub_info) = subs.remove(&topic) {
            sub_info.remove(&id);

            if sub_info.is_empty() {
                subs.remove(&topic);
            }
        }
    }
}
