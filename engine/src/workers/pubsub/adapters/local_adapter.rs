// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    engine::{Engine, EngineTrait},
    workers::pubsub::{
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

crate::register_adapter!(<PubSubAdapterRegistration> name: "local", make_adapter);

#[async_trait]
impl PubSubAdapter for LocalAdapter {
    async fn publish(&self, topic: &str, event_data: Value) {
        let topic = topic.to_string();
        let event_data = event_data.clone();
        let subscriptions = Arc::clone(&self.subscriptions);
        let subs = subscriptions.read().await;

        if let Some(sub_info) = subs.get(&topic) {
            for (id, function_id) in sub_info.iter() {
                tracing::debug!(function_id = %function_id, topic = %topic, "Event: Invoking function");
                let function_id = function_id.clone();
                let event_data = event_data.clone();
                let engine = Arc::clone(&self.engine);
                // Resolve the subscribing trigger's namespace LIVE by id.
                let namespace = engine.trigger_registry.namespace_of(id);

                tokio::spawn(async move {
                    let _ = engine
                        .call_with_metadata_ns(&namespace, &function_id, event_data, None)
                        .await;
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
        workers::observability::metrics::ensure_default_meter,
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

    /// BUG 1 (real-user path): a pubsub `subscribe` trigger registered by a
    /// namespaced worker must fire its target in that worker's namespace,
    /// resolved LIVE by trigger id at fire time. `pubsub::react` exists in both
    /// `orders` ("from-orders") and `default` ("from-default"); the trigger bound
    /// in `orders` must fire "from-orders". Drives the real mechanism:
    /// `adapter.publish`.
    #[tokio::test]
    async fn publish_invokes_target_in_registering_namespace() {
        use crate::engine::EngineTrait;
        ensure_default_meter();
        let engine = Arc::new(Engine::new());

        // The registry is the source of truth for the trigger's namespace (in
        // production the RegisterTrigger handler writes it); insert the binding.
        engine.trigger_registry.triggers.insert(
            "sub-orders".to_string(),
            crate::trigger::Trigger {
                id: "sub-orders".to_string(),
                trigger_type: "subscribe".to_string(),
                function_id: "pubsub::react".to_string(),
                config: json!({ "topic": "orders" }),
                worker_id: None,
                metadata: None,
                namespace: "orders".to_string(),
            },
        );

        let fired = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        for (ns, tag) in [("orders", "from-orders"), ("default", "from-default")] {
            let fired = fired.clone();
            engine.register_function_handler_ns(
                ns,
                RegisterFunctionRequest {
                    function_id: "pubsub::react".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(move |_input: Value| {
                    let fired = fired.clone();
                    async move {
                        fired.lock().unwrap().push(tag.to_string());
                        FunctionResult::Success(None)
                    }
                }),
            );
        }

        let adapter = LocalAdapter::new(engine.clone())
            .await
            .expect("new local adapter");
        adapter
            .subscribe("orders", "sub-orders", "pubsub::react")
            .await;
        adapter.publish("orders", json!({ "id": 1 })).await;

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let fired = fired.lock().unwrap().clone();
        assert_eq!(
            fired,
            vec!["from-orders".to_string()],
            "the orders pubsub trigger must fire the orders target; got: {fired:?}"
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
    async fn unsubscribe_removes_topic_entry_from_map() {
        // The engine's unsubscribe does `subs.remove(&topic)` first, removes the
        // subscription id from the returned map, then only re-checks emptiness
        // but never re-inserts remaining subscribers. So unsubscribing any id
        // on a topic causes the entire topic entry to be dropped from the map.
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

        // The topic entry is completely removed from the subscriptions map
        let subscriptions = adapter.subscriptions.read().await;
        assert!(
            subscriptions.get("orders").is_none(),
            "topic should be removed from subscriptions after unsubscribe"
        );
        drop(subscriptions);

        // Publishing should have no effect since the topic is gone
        adapter.publish("orders", json!({ "id": 7 })).await;
        assert!(
            timeout(Duration::from_millis(100), rx.recv())
                .await
                .is_err(),
            "no listener should be invoked after topic is removed"
        );
    }
}
