// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use colored::Colorize;
use function_macros::{function, service};
use futures::Future;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{PubSubAdapter, config::PubSubModuleConfig, configuration};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::ErrorBody,
    trigger::{Trigger, TriggerRegistrator, TriggerType},
    workers::traits::{AdapterFactory, ConfigurableWorker, Worker},
};

/// Trigger type id for pub/sub subscriptions.
const SUBSCRIBE_TRIGGER_TYPE: &str = "subscribe";

/// Live subscriptions tracked by the worker: `trigger id -> (topic, function_id)`.
/// This is the authoritative set the adapter hot-swap rebinds (see the
/// `subscriptions` field on [`PubSubWorker`]).
type SubscriptionSet = HashMap<String, (String, String)>;

#[derive(Clone)]
pub struct PubSubWorker {
    engine: Arc<Engine>,
    /// The live pub/sub backend, swapped atomically by `apply_config`. Read on
    /// the publish/subscribe hot path via `adapter_snapshot()` so a backend swap
    /// applies on the very next read. A per-worker cell (not a process-global,
    /// so concurrent tests stay isolated).
    adapter: Arc<RwLock<Arc<dyn PubSubAdapter>>>,
    /// The live configuration, swapped atomically by `apply_config`. `adapter`
    /// is the only field today; the cell keeps the change-detection and snapshot
    /// machinery uniform with the other configurable workers.
    config: Arc<RwLock<Arc<PubSubModuleConfig>>>,
    /// The config.yaml block passed to `create()` (or built-in defaults). Used
    /// only as the seed for first-time `configuration::register` and the fetch
    /// fallback; the configuration worker entry is the runtime source of truth
    /// afterwards.
    seed: PubSubModuleConfig,
    /// The live worker shutdown receiver, stored by `start_background_tasks` so
    /// `apply_config` can refuse the adapter hot-swap once the worker is gone.
    /// `None` until started / after destroy.
    worker_shutdown_rx: Arc<Mutex<Option<tokio::sync::watch::Receiver<bool>>>>,
    /// The authoritative set of live subscriptions AND the lock that serializes
    /// every subscription mutation against an `apply_config` hot-swap.
    /// `register_trigger`/`unregister_trigger` mutate the backend and this set
    /// under the lock; `apply_config` rebinds exactly this set onto the new
    /// backend under the same lock. Tracking subscriptions here — rather than
    /// re-deriving them from the engine trigger registry — avoids the registry's
    /// subscribe-then-insert (and remove-then-unsubscribe) ordering racing the
    /// swap and stranding/leaking a subscription on the discarded backend.
    subscriptions: Arc<tokio::sync::Mutex<SubscriptionSet>>,
    /// Set once by `destroy` (under the subscriptions lock). A subscription that
    /// arrives after teardown is refused so it can't land on a backend being
    /// dropped; the trigger stays in the registry for a fresh worker to rebind.
    destroyed: Arc<AtomicBool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
pub struct PubSubInput {
    /// Topic to publish to. Subscribers registered for this topic receive the event.
    pub topic: String,
    /// JSON payload delivered to each subscriber.
    pub data: Value,
}

#[service(name = "pubsub")]
impl PubSubWorker {
    #[function(id = "publish", description = "Publishes an event")]
    pub async fn publish(&self, input: PubSubInput) -> FunctionResult<Option<Value>, ErrorBody> {
        let adapter = self.adapter_snapshot();
        let event_data = input.data;
        let topic = input.topic;

        if topic.is_empty() {
            return FunctionResult::Failure(ErrorBody {
                code: "topic_not_set".into(),
                message: "Topic is not set".into(),
                stacktrace: None,
            });
        }

        tracing::debug!(topic = %topic, event_data = %event_data, "Publishing event");
        let _ = adapter.publish(&topic, event_data).await;
        crate::workers::telemetry::collector::track_pubsub_publish();

        FunctionResult::Success(None)
    }
}

impl TriggerRegistrator for PubSubWorker {
    fn register_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let topic = trigger
            .clone()
            .config
            .get("topic")
            .unwrap_or_default()
            .as_str()
            .unwrap_or("")
            .to_string();

        tracing::info!(
            "{} PubSub subscription {} → {}",
            "[REGISTERED]".green(),
            topic.purple(),
            trigger.function_id.cyan()
        );

        let worker = self.clone();

        Box::pin(async move {
            if topic.is_empty() {
                tracing::warn!(
                    function_id = %trigger.function_id.purple(),
                    "Topic is not set for trigger"
                );
                return Ok(());
            }

            // Serialize the subscribe + set insert against an in-flight
            // `apply_config` hot-swap so the subscription lands on the live
            // backend and is recorded in the set the swap rebinds — never
            // stranded on a backend being torn down.
            let mut subscriptions = worker.subscriptions.lock().await;
            if worker.destroyed.load(Ordering::SeqCst) {
                // Worker torn down; leave the trigger in the registry for a fresh
                // worker to (re)subscribe. Don't touch a backend being discarded.
                return Ok(());
            }
            worker
                .adapter_snapshot()
                .subscribe(&topic, &trigger.id, &trigger.function_id)
                .await;
            subscriptions.insert(trigger.id.clone(), (topic, trigger.function_id.clone()));
            crate::workers::telemetry::collector::track_pubsub_subscribe();
            Ok(())
        })
    }

    fn unregister_trigger(
        &self,
        trigger: Trigger,
    ) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        let worker = self.clone();

        Box::pin(async move {
            tracing::debug!(trigger = %trigger.id, "Unregistering trigger");
            let topic = trigger
                .config
                .get("topic")
                .unwrap_or_default()
                .as_str()
                .unwrap_or("")
                .to_string();

            // Same lock as register/apply: unsubscribe off the live backend and
            // drop the subscription from the set atomically with respect to a swap.
            let mut subscriptions = worker.subscriptions.lock().await;
            worker
                .adapter_snapshot()
                .unsubscribe(&topic, &trigger.id)
                .await;
            subscriptions.remove(&trigger.id);
            Ok(())
        })
    }
}

#[async_trait]
impl Worker for PubSubWorker {
    fn name(&self) -> &'static str {
        "PubSubModule"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        // Inherent (macro-generated) registration of the pubsub `publish`
        // function. Inherent methods win method resolution, so this is the
        // `#[service]`-generated registrar, not this trait method.
        self.register_functions(engine.clone());
        // The internal configuration-change handler, registered in the worker
        // scope so destroy/reload removes it automatically. The hook order
        // differs by pipeline (boot runs this before start_background_tasks,
        // reload after), so start_background_tasks also registers it if absent.
        self.register_config_handler(&engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing PubSubModule");

        let trigger_type = TriggerType::new(
            SUBSCRIBE_TRIGGER_TYPE,
            "Subscribe to a topic",
            Box::new(self.clone()),
            None,
        );

        let _ = self.engine.register_trigger_type(trigger_type).await;

        Ok(())
    }

    async fn start_background_tasks(
        &self,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        // Mark active FIRST so the adoption apply (and the catch-up) run the
        // full apply, including the adapter hot-swap tier.
        *self
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned") = Some(shutdown_rx);

        // Adopt the configuration worker as the runtime source of truth. Bus
        // calls are bounded so a hung provider can't wedge the serial
        // worker-startup loop; failures degrade to the static config.yaml block.
        let register = tokio::time::timeout(
            configuration::CONFIG_BUS_TIMEOUT,
            configuration::register_config(self.engine.as_ref(), Some(&self.seed)),
        )
        .await
        .map_err(|_| anyhow::anyhow!("configuration::register timed out"))
        .and_then(|result| result);
        if let Err(err) = register {
            tracing::warn!(
                error = %err,
                "iii-pubsub: configuration::register failed; continuing with static config"
            );
        }

        // Initial adoption: re-fetch the authoritative value and hot-swap the
        // backend if it differs from the seed. At fresh boot there are usually
        // no `subscribe` triggers yet; on reload the re-subscription rebinds the
        // existing ones onto the adopted backend. Failures keep the seed adapter.
        if let Err(err) = self.apply_config().await {
            tracing::warn!(
                error = %err,
                "iii-pubsub: failed to read configuration; continuing with static config"
            );
        }

        // Register the handler before the trigger so an event can never fan out
        // to a missing function. On reload `register_functions` runs after this
        // hook; the `get` check avoids a spurious overwrite log on initial boot.
        if self
            .engine
            .functions
            .get(
                crate::protocol::DEFAULT_NAMESPACE,
                configuration::CONFIG_FN_ID,
            )
            .is_none()
        {
            self.register_config_handler(&self.engine);
        }
        if let Err(err) = configuration::register_config_trigger(&self.engine).await {
            tracing::warn!(
                error = %err,
                "iii-pubsub: failed to watch configuration changes; hot-reload disabled"
            );
        } else {
            // Catch-up: replay any `configuration::set` that landed between the
            // adoption apply above and the trigger subscription. Routed through
            // `on_config_change` so a timed-out catch-up gets the same one-shot
            // retry as a trigger-driven apply.
            configuration::on_config_change(self).await;
        }

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying PubSubModule");

        // Stop new configuration-change invocations from firing during
        // shutdown. The trigger is registered outside the worker scope, so
        // remove it explicitly to keep ReloadManager restarts duplicate-free.
        let _ = self
            .engine
            .trigger_registry
            .unregister_trigger(
                configuration::CONFIG_TRIGGER_ID.to_string(),
                Some(configuration::CONFIG_TRIGGER_TYPE.to_string()),
            )
            .await;

        // Under the subscriptions lock (serializing with `apply_config` and the
        // registrator): mark destroyed so a late subscribe is refused, clear the
        // liveness receiver so a later apply (e.g. a timeout retry) refuses the
        // hot-swap, then tear down the tracked subscriptions.
        let mut subscriptions = self.subscriptions.lock().await;
        self.destroyed.store(true, Ordering::SeqCst);
        self.worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned")
            .take();

        // `PubSubAdapter` has no `destroy()`; the redis backend's per-topic
        // subscription tasks are only stopped by `unsubscribe`. Unsubscribe the
        // tracked subscriptions from the current backend so those tasks are
        // aborted instead of leaking across a reload. The triggers stay in the
        // engine registry, so a fresh worker re-subscribes them onto its backend.
        let adapter = self.adapter_snapshot();
        for (id, (topic, _function_id)) in subscriptions.drain() {
            adapter.unsubscribe(&topic, &id).await;
        }
        Ok(())
    }
}

#[async_trait]
impl ConfigurableWorker for PubSubWorker {
    type Config = PubSubModuleConfig;
    type Adapter = dyn PubSubAdapter;
    type AdapterRegistration = super::registry::PubSubAdapterRegistration;
    const DEFAULT_ADAPTER_NAME: &'static str = "local";

    async fn registry() -> &'static RwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<RwLock<HashMap<String, AdapterFactory<dyn PubSubAdapter>>>> =
            Lazy::new(|| RwLock::new(PubSubWorker::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self::from_config(engine, config, adapter)
    }

    fn adapter_name_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.name.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

impl PubSubWorker {
    /// Construct a worker from a parsed config and a ready adapter, wrapping the
    /// config and adapter in their live cells. The seed is the config as
    /// supplied (config.yaml block or defaults); the configuration worker entry
    /// becomes the runtime source of truth after first boot.
    fn from_config(
        engine: Arc<Engine>,
        config: PubSubModuleConfig,
        adapter: Arc<dyn PubSubAdapter>,
    ) -> Self {
        Self {
            engine,
            adapter: Arc::new(RwLock::new(adapter)),
            config: Arc::new(RwLock::new(Arc::new(config.clone()))),
            seed: config,
            worker_shutdown_rx: Arc::new(Mutex::new(None)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(SubscriptionSet::new())),
            destroyed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Test-only constructor: parse the config, build its adapter from the
    /// registry (async, like `create_with_adapters`), and wrap a worker around
    /// it. Used by the `pubsub_configuration_e2e` suite (integration tests are a
    /// separate crate, so this cannot be `#[cfg(test)]`).
    pub async fn for_test(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Self> {
        let parsed: PubSubModuleConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let adapter = Self::resolve_adapter(&engine, &parsed).await?;
        Ok(Self::from_config(engine, parsed, adapter))
    }

    /// Test-only: mark the worker active without `start_background_tasks`, so a
    /// unit test can exercise the full `apply_config` path (which gates on
    /// liveness). The watch sender is dropped immediately — the receiver stays
    /// valid and `is_active()` only checks its presence.
    #[cfg(test)]
    pub(crate) fn mark_active_for_test(&self) {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        *self
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned") = Some(rx);
    }

    /// True while the worker is running (between `start_background_tasks` and
    /// `destroy`). Gates the adapter hot-swap apply tier and the apply retry.
    pub(crate) fn is_active(&self) -> bool {
        self.worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned")
            .is_some()
    }

    /// Cheap clone of the live config.
    pub(crate) fn config_snapshot(&self) -> Arc<PubSubModuleConfig> {
        self.config.read().expect("config lock poisoned").clone()
    }

    /// The effective live configuration as an owned value (for tests / external
    /// callers). Hot paths should use [`Self::adapter_snapshot`] instead.
    pub fn current_config(&self) -> PubSubModuleConfig {
        (*self.config_snapshot()).clone()
    }

    fn set_config(&self, config: PubSubModuleConfig) {
        *self.config.write().expect("config lock poisoned") = Arc::new(config);
    }

    /// Cheap clone of the live pub/sub backend. Take one snapshot per
    /// publish/subscribe so all reads within it use one consistent backend.
    pub fn adapter_snapshot(&self) -> Arc<dyn PubSubAdapter> {
        self.adapter.read().expect("adapter lock poisoned").clone()
    }

    fn set_adapter(&self, adapter: Arc<dyn PubSubAdapter>) {
        *self.adapter.write().expect("adapter lock poisoned") = adapter;
    }

    /// The `(name, config)` pair an adapter would be built from, normalizing an
    /// absent adapter to the default — so `None` vs `Some(default)` is not a
    /// false change in `apply_config`.
    fn effective_adapter(config: &PubSubModuleConfig) -> (String, Option<Value>) {
        (
            Self::adapter_name_from_config(config)
                .unwrap_or_else(|| Self::DEFAULT_ADAPTER_NAME.to_string()),
            Self::adapter_config_from_config(config),
        )
    }

    /// Build a fresh adapter from a config, mirroring steps 2–4 of
    /// `create_with_adapters`. Returns an error for an unknown adapter name (the
    /// registry is the validation authority) or a factory failure; the caller
    /// gates on it so a bad value keeps the previous backend.
    async fn resolve_adapter(
        engine: &Arc<Engine>,
        config: &PubSubModuleConfig,
    ) -> anyhow::Result<Arc<dyn PubSubAdapter>> {
        let adapter_name = Self::adapter_name_from_config(config)
            .unwrap_or_else(|| Self::DEFAULT_ADAPTER_NAME.to_string());
        let factory = Self::get_adapter(&adapter_name)
            .await
            .ok_or_else(|| anyhow::anyhow!("PubSub adapter factory '{adapter_name}' not found"))?;
        let adapter_config = Self::adapter_config_from_config(config);
        factory(engine.clone(), adapter_config).await
    }

    /// Register the `iii-pubsub::on-config-change` handler. Idempotent
    /// (replace-by-id), so it is safe to call from both `register_functions`
    /// (worker scope, for destroy/reload cleanup) and `start_background_tasks`
    /// (which registers the trigger). Tagged `metadata.internal = true`.
    fn register_config_handler(&self, engine: &Arc<Engine>) {
        let worker = self.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: configuration::CONFIG_FN_ID.to_string(),
                description: Some(
                    "Internal: re-apply the iii-pubsub configuration when the authoritative \
                     configuration entry changes."
                        .to_string(),
                ),
                request_format: None,
                response_format: None,
                metadata: Some(serde_json::json!({ "internal": true })),
            },
            Handler::new(move |_payload: Value| {
                let worker = worker.clone();
                async move {
                    configuration::on_config_change(&worker).await;
                    FunctionResult::Success(Some(serde_json::json!({ "ok": true })))
                }
            }),
        );
    }

    /// Re-fetch the authoritative configuration and hot-apply it under the
    /// subscriptions lock so overlapping configuration events can't apply a stale
    /// value last, and so a concurrent subscribe/unsubscribe can't interleave
    /// with the swap.
    ///
    /// The pub/sub `adapter` is a full hot-swap: build the new backend FIRST
    /// (gated — a build failure keeps the previous backend and config), then
    /// re-subscribe every tracked subscription onto it, swap the live backend so
    /// new publishes route through it, and finally tear down the previous
    /// backend's subscriptions. Re-subscribing before the swap avoids a delivery
    /// gap; the brief overlap where both backends hold a subscription is accepted
    /// for fire-and-forget pub/sub.
    pub(crate) async fn apply_config(&self) -> anyhow::Result<()> {
        // The subscriptions lock is also the apply lock: holding it serializes
        // this swap against `register_trigger`/`unregister_trigger` (which mutate
        // both the backend and the tracked set under the same lock), so the set
        // read below is exactly the set live on the current backend.
        let subscriptions = self.subscriptions.lock().await;

        // Refuse to apply once the worker is gone — `destroy` clears the
        // liveness receiver under this same lock, so a post-destroy retry can
        // never rebuild a backend or re-subscribe on torn-down state.
        if !self.is_active() {
            tracing::debug!("iii-pubsub: worker not active; skipping configuration apply");
            return Ok(());
        }

        let old = self.config_snapshot();

        // Fetch the authoritative value under the lock; a malformed stored value
        // surfaces here and keeps the previous config. Bounded so a hung
        // provider can't wedge every future apply behind the lock.
        let new = match tokio::time::timeout(
            configuration::CONFIG_BUS_TIMEOUT,
            configuration::fetch_config(self.engine.as_ref(), old.as_ref()),
        )
        .await
        {
            Ok(result) => result?,
            // Keep the `Elapsed` error downcastable: `on_config_change`
            // schedules a one-shot retry for timeouts specifically.
            Err(elapsed) => {
                return Err(anyhow::Error::new(elapsed)
                    .context("configuration::get timed out; keeping previous config"));
            }
        };

        // The adapter is the only field. If it is unchanged, publish the config
        // snapshot (cheap, keeps the cell uniform) and return — no rebuild.
        if Self::effective_adapter(&old) == Self::effective_adapter(&new) {
            self.set_config(new);
            return Ok(());
        }

        // FULL HOT-SWAP. Build the new backend first; a failure returns here and
        // keeps the previous backend, config, and subscriptions intact.
        let new_adapter = Self::resolve_adapter(&self.engine, &new).await?;

        // Re-subscribe the tracked set onto the new backend BEFORE the swap so
        // there is no delivery gap for an already-subscribed topic.
        for (id, (topic, function_id)) in subscriptions.iter() {
            new_adapter.subscribe(topic, id, function_id).await;
        }

        // Swap: new publishes (which read `adapter_snapshot()`) route through the
        // new backend from here on.
        let previous_adapter = self.adapter_snapshot();
        self.set_adapter(new_adapter);
        self.set_config(new);

        // Tear down the previous backend's subscriptions so its per-topic tasks
        // (redis) are aborted rather than leaked. New publishes never reach it.
        for (id, (topic, _function_id)) in subscriptions.iter() {
            previous_adapter.unsubscribe(topic, id).await;
        }

        tracing::info!(
            subscriptions = subscriptions.len(),
            "iii-pubsub: adapter hot-swapped; subscriptions rebound onto the new backend"
        );
        Ok(())
    }
}

crate::register_worker!(
    "iii-pubsub",
    PubSubWorker,
    description = "Topic-based publish/subscribe messaging for real-time event distribution.",
    enabled_by_default = true
);

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::{
        engine::Engine,
        workers::{observability::metrics::ensure_default_meter, traits::AdapterEntry},
    };

    #[derive(Default)]
    struct RecordingPubSubAdapter {
        published: Mutex<Vec<(String, Value)>>,
        subscribed: Mutex<Vec<(String, String, String)>>,
        unsubscribed: Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl PubSubAdapter for RecordingPubSubAdapter {
        async fn publish(&self, topic: &str, pubsub_data: Value) {
            self.published
                .lock()
                .expect("lock published")
                .push((topic.to_string(), pubsub_data));
        }

        async fn subscribe(&self, topic: &str, id: &str, function_id: &str) {
            self.subscribed.lock().expect("lock subscribed").push((
                topic.to_string(),
                id.to_string(),
                function_id.to_string(),
            ));
        }

        async fn unsubscribe(&self, topic: &str, id: &str) {
            self.unsubscribed
                .lock()
                .expect("lock unsubscribed")
                .push((topic.to_string(), id.to_string()));
        }
    }

    fn build_module() -> (Arc<Engine>, PubSubWorker, Arc<RecordingPubSubAdapter>) {
        ensure_default_meter();
        let engine = Arc::new(Engine::new());
        let adapter = Arc::new(RecordingPubSubAdapter::default());
        let module = PubSubWorker::build(
            engine.clone(),
            PubSubModuleConfig::default(),
            adapter.clone(),
        );
        (engine, module, adapter)
    }

    #[tokio::test]
    async fn publish_rejects_empty_topic() {
        let (_engine, module, adapter) = build_module();

        let result = module
            .publish(PubSubInput {
                topic: String::new(),
                data: json!({ "ignored": true }),
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "topic_not_set"),
            _ => panic!("expected topic_not_set failure"),
        }
        assert!(adapter.published.lock().expect("lock published").is_empty());
    }

    #[tokio::test]
    async fn publish_delegates_to_adapter_and_returns_success() {
        let (_engine, module, adapter) = build_module();

        let result = module
            .publish(PubSubInput {
                topic: "orders".to_string(),
                data: json!({ "id": 1 }),
            })
            .await;

        assert!(matches!(result, FunctionResult::Success(None)));
        let published = adapter.published.lock().expect("lock published");
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, "orders");
        assert_eq!(published[0].1, json!({ "id": 1 }));
    }

    #[tokio::test]
    async fn register_and_unregister_trigger_delegate_to_adapter() {
        let (_engine, module, adapter) = build_module();
        let trigger = Trigger {
            id: "sub-1".to_string(),
            trigger_type: "subscribe".to_string(),
            function_id: "test::listener".to_string(),
            config: json!({ "topic": "orders" }),
            worker_id: None,
            metadata: None,
        };

        module
            .register_trigger(trigger.clone())
            .await
            .expect("register pubsub trigger");
        module
            .unregister_trigger(trigger)
            .await
            .expect("unregister pubsub trigger");

        let subscribed = adapter.subscribed.lock().expect("lock subscribed");
        assert_eq!(
            subscribed.as_slice(),
            &[(
                "orders".to_string(),
                "sub-1".to_string(),
                "test::listener".to_string(),
            )]
        );
        let unsubscribed = adapter.unsubscribed.lock().expect("lock unsubscribed");
        assert_eq!(
            unsubscribed.as_slice(),
            &[("orders".to_string(), "sub-1".to_string())]
        );
    }

    #[tokio::test]
    async fn register_trigger_without_topic_skips_subscription() {
        let (_engine, module, adapter) = build_module();

        module
            .register_trigger(Trigger {
                id: "sub-empty".to_string(),
                trigger_type: "subscribe".to_string(),
                function_id: "test::listener".to_string(),
                config: json!({}),
                worker_id: None,
                metadata: None,
            })
            .await
            .expect("register trigger without topic");

        assert!(
            adapter
                .subscribed
                .lock()
                .expect("lock subscribed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn initialize_registers_subscribe_trigger_type() {
        let (engine, module, _adapter) = build_module();

        module.initialize().await.expect("initialize pubsub module");

        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key("subscribe")
        );
        assert_eq!(module.name(), "PubSubModule");
    }

    #[test]
    fn adapter_name_and_config_are_read_from_config() {
        let config = PubSubModuleConfig {
            adapter: Some(AdapterEntry {
                name: "custom::PubSub".to_string(),
                config: Some(json!({ "url": "redis://example" })),
            }),
        };

        assert_eq!(
            PubSubWorker::adapter_name_from_config(&config).as_deref(),
            Some("custom::PubSub")
        );
        assert_eq!(
            PubSubWorker::adapter_config_from_config(&config),
            Some(json!({ "url": "redis://example" }))
        );
    }

    #[test]
    fn effective_adapter_treats_none_as_default() {
        // Absent adapter and an explicit `local` adapter resolve to the same
        // effective pair, so a `None` -> default boot does not look like a change.
        let none = PubSubModuleConfig::default();
        let local = PubSubModuleConfig {
            adapter: Some(AdapterEntry {
                name: "local".to_string(),
                config: None,
            }),
        };
        assert_eq!(
            PubSubWorker::effective_adapter(&none),
            PubSubWorker::effective_adapter(&local),
            "None must normalize to the default adapter"
        );

        // A different backend is a real change.
        let redis = PubSubModuleConfig {
            adapter: Some(AdapterEntry {
                name: "redis".to_string(),
                config: None,
            }),
        };
        assert_ne!(
            PubSubWorker::effective_adapter(&none),
            PubSubWorker::effective_adapter(&redis)
        );
    }

    /// Stub `configuration::get` so `apply_config` reads a fixed stored value.
    fn stub_config_get(engine: &Arc<Engine>, value: Value) {
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "configuration::get".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input: Value| {
                let value = value.clone();
                async move {
                    FunctionResult::Success(Some(json!({ "id": "iii-pubsub", "value": value })))
                }
            }),
        );
    }

    #[tokio::test]
    async fn apply_config_is_noop_when_worker_inactive() {
        let (engine, module, _adapter) = build_module();
        // A changed adapter is stored, but the worker was never marked active.
        stub_config_get(&engine, json!({ "adapter": { "name": "redis" } }));

        module.apply_config().await.expect("apply ok");

        // The inactive gate returned before fetching/swapping: config unchanged.
        assert!(
            module.current_config().adapter.is_none(),
            "an inactive worker must not apply a configuration change"
        );
    }

    #[tokio::test]
    async fn destroy_unsubscribes_live_subscriptions_and_marks_inactive() {
        let (_engine, module, adapter) = build_module();
        module.mark_active_for_test();
        assert!(module.is_active());

        module
            .register_trigger(Trigger {
                id: "sub-1".to_string(),
                trigger_type: "subscribe".to_string(),
                function_id: "test::listener".to_string(),
                config: json!({ "topic": "orders" }),
                worker_id: None,
                metadata: None,
            })
            .await
            .expect("register subscribe trigger");

        module.destroy().await.expect("destroy");

        assert!(!module.is_active(), "destroy must clear liveness");
        let unsubscribed = adapter.unsubscribed.lock().expect("lock unsubscribed");
        assert_eq!(
            unsubscribed.as_slice(),
            &[("orders".to_string(), "sub-1".to_string())],
            "destroy must unsubscribe the live subscription so its backend task is aborted"
        );
    }

    #[tokio::test]
    async fn hot_swap_unsubscribes_previous_and_subscribes_new_backend() {
        let (engine, module, previous) = build_module();
        module.mark_active_for_test();

        // A live subscription on the seed (previous) backend.
        module
            .register_trigger(Trigger {
                id: "sub-1".to_string(),
                trigger_type: "subscribe".to_string(),
                function_id: "test::listener".to_string(),
                config: json!({ "topic": "orders" }),
                worker_id: None,
                metadata: None,
            })
            .await
            .expect("register subscribe trigger");

        // Inject a recording adapter the swap will build, and store a value
        // pointing at it (a different effective adapter forces the rebuild).
        let next = Arc::new(RecordingPubSubAdapter::default());
        let next_for_factory = next.clone();
        PubSubWorker::add_adapter("recording::swap-target", move |_engine, _config| {
            let next = next_for_factory.clone();
            async move { Ok(next as Arc<dyn PubSubAdapter>) }
        })
        .await
        .expect("register adapter");
        stub_config_get(
            &engine,
            json!({ "adapter": { "name": "recording::swap-target" } }),
        );

        module.apply_config().await.expect("apply hot-swap");

        // The previous backend was torn down for the live subscription (so a
        // redis task would be aborted, not leaked)...
        let prev_unsub = previous.unsubscribed.lock().expect("lock unsubscribed");
        assert_eq!(
            prev_unsub.as_slice(),
            &[("orders".to_string(), "sub-1".to_string())],
            "swap must unsubscribe the live subscription from the previous backend"
        );
        drop(prev_unsub);
        // ...and the new backend received the rebound subscription.
        let next_sub = next.subscribed.lock().expect("lock subscribed");
        assert_eq!(
            next_sub.as_slice(),
            &[(
                "orders".to_string(),
                "sub-1".to_string(),
                "test::listener".to_string(),
            )],
            "swap must re-subscribe the live subscription onto the new backend"
        );
        drop(next_sub);
        // The live adapter is the swapped-in instance.
        assert!(Arc::ptr_eq(
            &module.adapter_snapshot(),
            &(next as Arc<dyn PubSubAdapter>)
        ));
    }
}
