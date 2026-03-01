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
                    let _ = engine.call(&function_id, event_data).await;
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

            if !sub_info.is_empty() {
                subs.insert(topic, sub_info);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use tokio::{
        sync::mpsc,
        time::{Duration, timeout},
    };

    use super::*;
    use crate::{
        engine::{Engine, Handler, RegisterFunctionRequest},
        function::FunctionResult,
        modules::observability::metrics::ensure_default_meter,
    };

    fn register_listener(
        engine: &Arc<Engine>,
        function_id: &str,
        tx: mpsc::UnboundedSender<String>,
    ) {
        let function_id_owned = function_id.to_string();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: function_id_owned.clone(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input: Value| {
                let tx = tx.clone();
                let function_id = function_id_owned.clone();
                async move {
                    tx.send(format!("{function_id}:{}", input["id"]))
                        .expect("send invoked function");
                    FunctionResult::Success(None)
                }
            }),
        );
    }

    #[tokio::test]
    async fn new_and_make_adapter_create_empty_subscription_store() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());

        let adapter = LocalAdapter::new(engine.clone())
            .await
            .expect("new local adapter");
        assert!(adapter.subscriptions.read().await.is_empty());

        let adapter = make_adapter(engine, None).await.expect("make adapter");
        adapter.subscribe("orders", "sub-1", "test::listener").await;
        adapter.unsubscribe("orders", "sub-1").await;
    }

    #[tokio::test]
    async fn publish_without_subscriptions_is_noop() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter = LocalAdapter::new(engine).await.expect("new local adapter");

        adapter.publish("orders", json!({ "id": 1 })).await;
        assert!(adapter.subscriptions.read().await.is_empty());
    }

    #[tokio::test]
    async fn unsubscribe_preserves_other_subscribers_on_same_topic() {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter = LocalAdapter::new(engine.clone())
            .await
            .expect("new local adapter");
        let (tx, mut rx) = mpsc::unbounded_channel();

        register_listener(&engine, "test::listener_a", tx.clone());
        register_listener(&engine, "test::listener_b", tx);

        adapter
            .subscribe("orders", "sub-a", "test::listener_a")
            .await;
        adapter
            .subscribe("orders", "sub-b", "test::listener_b")
            .await;
        adapter.unsubscribe("orders", "sub-a").await;

        let subscriptions = adapter.subscriptions.read().await;
        let topic_subscriptions = subscriptions
            .get("orders")
            .expect("topic should remain subscribed");
        assert_eq!(topic_subscriptions.len(), 1);
        assert_eq!(
            topic_subscriptions.get("sub-b").map(String::as_str),
            Some("test::listener_b")
        );
        drop(subscriptions);

        adapter.publish("orders", json!({ "id": 7 })).await;

        let invoked = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for publish")
            .expect("listener channel closed");
        assert_eq!(invoked, "test::listener_b:7");
        assert!(
            timeout(Duration::from_millis(100), rx.recv())
                .await
                .is_err()
        );
    }
}
