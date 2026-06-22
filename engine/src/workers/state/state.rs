// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock as SyncRwLock},
};

use function_macros::{function, service};
use once_cell::sync::Lazy;
use serde_json::Value;
use tracing::Instrument;

use iii_sdk::types::{SetResult, UpdateResult};

use crate::{
    condition::check_condition,
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::ErrorBody,
    trigger::TriggerType,
    workers::{
        state::{
            adapters::StateAdapter,
            config::StateModuleConfig,
            configuration,
            structs::{
                StateDeleteInput, StateEventData, StateEventType, StateGetGroupInput,
                StateGetInput, StateListGroupsInput, StateListGroupsResult, StateSetInput,
                StateUpdateInput,
            },
            trigger::{StateTrigger, StateTriggers, TRIGGER_TYPE},
        },
        traits::{AdapterFactory, ConfigurableWorker, Worker},
    },
};

#[derive(Clone)]
pub struct StateWorker {
    adapter: Arc<dyn StateAdapter>,
    engine: Arc<Engine>,
    pub triggers: Arc<StateTriggers>,
    /// The live configuration, swapped atomically by `apply_config`. Read on
    /// the request/fan-out hot path via `config_snapshot()` so a configuration
    /// change applies on the very next read. Mirrors `HttpWorker`'s live
    /// config cell (a per-worker cell, not a process-global, so concurrent
    /// tests stay isolated).
    config: Arc<SyncRwLock<Arc<StateModuleConfig>>>,
    /// The config.yaml block passed to `create()` (or built-in defaults),
    /// merged with the persisted entry at boot. Used only as the seed for
    /// first-time `configuration::register` and the fetch fallback; the
    /// configuration worker entry is the runtime source of truth afterwards.
    seed: StateModuleConfig,
    /// The live worker shutdown receiver, stored by `start_background_tasks`
    /// so `apply_config` can refuse the adapter task-rebuild tier once the
    /// worker is gone. `None` until started / after destroy.
    worker_shutdown_rx: Arc<std::sync::Mutex<Option<tokio::sync::watch::Receiver<bool>>>>,
    /// Serializes concurrent `apply_config` runs (rapid configuration edits).
    apply_lock: Arc<tokio::sync::Mutex<()>>,
}

#[async_trait::async_trait]
impl Worker for StateWorker {
    fn name(&self) -> &'static str {
        "StateWorker"
    }
    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        // The persisted `iii-state` configuration entry is the runtime source of
        // truth: read it and let it replace the config.yaml block so a
        // runtime-edited adapter (or knob) survives restarts. The adapter is
        // built from the resolved config below.
        let config = configuration::resolve_boot_config(config);
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        // Inherent (macro-generated) registration of the state::* functions.
        self.register_functions(engine.clone());
        // The internal configuration-change handler, registered in the worker
        // scope so destroy/reload removes it automatically. The hook order
        // differs by pipeline (boot runs this before start_background_tasks,
        // reload after), so start_background_tasks also registers it if absent.
        self.register_config_handler(&engine);
    }

    async fn start_background_tasks(
        &self,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        // Adopt the configuration worker as the runtime source of truth.
        // `configuration::*` is callable here on both pipelines (boot registers
        // all worker functions before serving; reload starts the mandatory
        // configuration worker before optional ones). Failures degrade to the
        // static config.yaml block so state stays up. Bus calls are bounded so
        // a hung provider can't wedge the serial worker-startup loop.

        // Mark active FIRST so the adoption apply (and the catch-up) run the
        // full apply, including the adapter task-rebuild tier.
        *self
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned") = Some(shutdown_rx);

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
                "iii-state: configuration::register failed; continuing with static config"
            );
        }

        // Initial adoption: re-fetch the authoritative value and apply all
        // tiers (snapshot swap + adapter cadence). Failures keep static config.
        if let Err(err) = self.apply_config().await {
            tracing::warn!(
                error = %err,
                "iii-state: failed to read configuration; continuing with static config"
            );
        }

        // Register the handler before the trigger so an event can never fan out
        // to a missing function. On reload `register_functions` runs after this
        // hook; the `get` check avoids a spurious overwrite log on initial boot.
        if self
            .engine
            .functions
            .get(configuration::CONFIG_FN_ID)
            .is_none()
        {
            self.register_config_handler(&self.engine);
        }
        if let Err(err) = configuration::register_config_trigger(&self.engine).await {
            tracing::warn!(
                error = %err,
                "iii-state: failed to watch configuration changes; hot-reload disabled"
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
        tracing::info!("Destroying StateWorker");

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

        // Serialize with any in-flight `apply_config`, then clear the liveness
        // receiver so a later apply refuses the adapter task-rebuild tier.
        {
            let _guard = self.apply_lock.lock().await;
            self.worker_shutdown_rx
                .lock()
                .expect("worker_shutdown_rx mutex poisoned")
                .take();
        }

        self.adapter.destroy().await?;
        Ok(())
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing StateWorker");

        let _ = self
            .engine
            .register_trigger_type(TriggerType::new(
                TRIGGER_TYPE,
                "State trigger",
                Box::new(self.clone()),
                None,
            ))
            .await;

        Ok(())
    }
}

#[async_trait::async_trait]
impl ConfigurableWorker for StateWorker {
    type Config = StateModuleConfig;
    type Adapter = dyn StateAdapter;
    type AdapterRegistration = super::registry::StateAdapterRegistration;
    const DEFAULT_ADAPTER_NAME: &'static str = "kv";

    async fn registry() -> &'static SyncRwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<SyncRwLock<HashMap<String, AdapterFactory<dyn StateAdapter>>>> =
            Lazy::new(|| SyncRwLock::new(StateWorker::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self::from_config(engine, config, adapter)
    }

    fn adapter_name_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.name.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        let mut adapter_config = config.adapter.as_ref().and_then(|a| a.config.clone());
        // The top-level `save_interval_ms` is the authoritative cadence knob.
        // Fold it into the adapter config blob the kv store is built from so the
        // boot-constructed save loop honors it (otherwise the adapter would read
        // only its inner `adapter.config.save_interval_ms` and the top-level
        // value would not take effect until a runtime reconfigure).
        if let Some(interval) = config.save_interval_ms {
            let entry = adapter_config.get_or_insert_with(|| Value::Object(Default::default()));
            if let Some(obj) = entry.as_object_mut() {
                obj.insert("save_interval_ms".to_string(), Value::from(interval));
            }
        }
        adapter_config
    }
}

/// True when a state trigger's scope/key constraints match the event. Scope
/// and key are local fields (no RPC); `condition_function_id` is evaluated
/// separately, only after a scope/key match, inside the trigger span.
fn state_trigger_matches(
    config: &super::trigger::StateTriggerConfig,
    event_scope: &str,
    event_key: &str,
) -> bool {
    if let Some(scope) = &config.scope {
        if scope != event_scope {
            return false;
        }
    }
    if let Some(key) = &config.key {
        if key != event_key {
            return false;
        }
    }
    true
}

impl StateWorker {
    /// Construct a worker from a parsed config and a ready adapter, storing the
    /// normalized config in the per-worker config cell so the live gates
    /// (`triggers_enabled`, `max_value_bytes`) observe the effective config
    /// immediately — before `start_background_tasks` runs the adoption apply.
    fn from_config(
        engine: Arc<Engine>,
        config: StateModuleConfig,
        adapter: Arc<dyn StateAdapter>,
    ) -> Self {
        let seed = config.normalized();
        Self {
            adapter,
            engine,
            triggers: Arc::new(StateTriggers::new()),
            config: Arc::new(SyncRwLock::new(Arc::new(seed.clone()))),
            seed,
            worker_shutdown_rx: Arc::new(std::sync::Mutex::new(None)),
            apply_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Test-only constructor: parse the config, build an in-process kv adapter
    /// from its adapter block, and wrap a worker around it. Used by the
    /// `state_configuration_e2e` suite (integration tests are a separate crate,
    /// so this cannot be `#[cfg(test)]`).
    pub fn for_test(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Self> {
        let parsed: StateModuleConfig = config
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let adapter_config = Self::adapter_config_from_config(&parsed);
        let adapter: Arc<dyn StateAdapter> = Arc::new(
            super::adapters::kv_store::BuiltinKvStoreAdapter::new(adapter_config),
        );
        Ok(Self::from_config(engine, parsed, adapter))
    }

    /// True while the worker is running (between `start_background_tasks` and
    /// `destroy`). Gates the adapter task-rebuild apply tier and the apply
    /// retry.
    pub(crate) fn is_active(&self) -> bool {
        self.worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex poisoned")
            .is_some()
    }

    /// Cheap clone of the live config. Take one snapshot per request/function
    /// so all reads within it are consistent.
    pub(crate) fn config_snapshot(&self) -> Arc<StateModuleConfig> {
        self.config.read().expect("config lock poisoned").clone()
    }

    fn set_config(&self, config: StateModuleConfig) {
        *self.config.write().expect("config lock poisoned") = Arc::new(config);
    }

    /// The effective live configuration as an owned value (for tests / external
    /// callers). Hot paths should use [`Self::config_snapshot`] instead.
    pub fn current_config(&self) -> StateModuleConfig {
        (*self.config_snapshot()).clone()
    }

    /// Re-fetch the authoritative configuration and hot-apply it under
    /// `apply_lock` so overlapping configuration events can't apply a stale
    /// value last. All-or-nothing: any failure keeps the previous config.
    ///
    /// - LIVE: swap the snapshot (`triggers_enabled` / `max_value_bytes` are
    ///   read from it on the hot path, so the swap applies them).
    /// - TASK-REBUILD: re-tune the adapter's save cadence when
    ///   `save_interval_ms` changed.
    /// - RESTART-ONLY: an `adapter` change is logged and takes effect at the
    ///   next engine start (the persisted entry is read at boot).
    pub(crate) async fn apply_config(&self) -> anyhow::Result<()> {
        let _guard = self.apply_lock.lock().await;

        // Refuse to apply once the worker is gone — `destroy` clears the
        // liveness receiver under this same lock, so a post-destroy retry can
        // never respawn the adapter save loop on torn-down state.
        if !self.is_active() {
            tracing::debug!("iii-state: worker not active; skipping configuration apply");
            return Ok(());
        }

        let old = self.config_snapshot();

        // Fetch the authoritative value under the lock; a malformed stored
        // value surfaces here and keeps the previous config. Bounded so a hung
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

        let reconfigure_cadence = old.save_interval_ms != new.save_interval_ms;
        let adapter_changed = old.adapter != new.adapter;
        let new_save_interval = new.save_interval_ms;

        // LIVE
        self.set_config(new);

        // TASK-REBUILD
        if reconfigure_cadence {
            match self
                .adapter
                .reconfigure(&serde_json::json!({ "save_interval_ms": new_save_interval }))
                .await
            {
                Ok(()) => tracing::info!(
                    save_interval_ms = ?new_save_interval,
                    "iii-state: save cadence retuned"
                ),
                Err(err) => tracing::warn!(
                    error = %err,
                    "iii-state: adapter reconfigure failed; save cadence unchanged"
                ),
            }
        }

        // RESTART-ONLY
        if adapter_changed {
            tracing::warn!(
                "iii-state: `adapter` changed; this is restart-tier and applies at the next \
                 engine start (the persisted entry is read at boot)"
            );
        }

        Ok(())
    }

    /// Register the `iii-state::on-config-change` handler. Idempotent
    /// (replace-by-id), so it is safe to call from both `register_functions`
    /// (worker scope, for destroy/reload cleanup) and `start_background_tasks`
    /// (which registers the trigger). Tagged `metadata.internal = true`: this is
    /// surfaced by `engine::functions::info` and is forward-compatible with a
    /// metadata-aware listing filter, but the current default listing filter
    /// hides rows by the `engine::` function-id prefix, so this handler (and the
    /// `iii-state::config-watch` trigger) are still listed by default — the
    /// handler is, by design, a discoverable bus function.
    fn register_config_handler(&self, engine: &Arc<Engine>) {
        let worker = self.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: configuration::CONFIG_FN_ID.to_string(),
                description: Some(
                    "Internal: re-apply the iii-state configuration when the authoritative \
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

    /// Invoke triggers for a given event type with condition checks.
    ///
    /// Triggers are pre-filtered by scope/key BEFORE the `state_triggers` span
    /// is created, so a write whose scope/key matches no registered trigger
    /// emits NO span and spawns NO task. The engine fires one trigger
    /// evaluation per state write, so gating here avoids the span (and its
    /// CPU/memory/export cost) for the common no-match case. Matching triggers
    /// still have their `condition_function_id` evaluated inside the span.
    async fn invoke_triggers(&self, event_data: StateEventData) {
        // LIVE gate: `triggers_enabled` globally pauses/resumes fan-out without
        // a restart. Defaults to enabled (absent => true). Checked before any
        // trigger collection so a disabled worker does no fan-out work.
        if !self.config_snapshot().triggers_enabled.unwrap_or(true) {
            tracing::debug!("state trigger fan-out disabled via configuration; skipping");
            return;
        }

        let event_scope = event_data.scope.clone();
        let event_key = event_data.key.clone();

        // Collect only the triggers whose scope/key match, releasing the lock
        // before spawning.
        let triggers: Vec<StateTrigger> = {
            let triggers_guard = self.triggers.list.read().await;
            triggers_guard
                .values()
                .filter(|t| state_trigger_matches(&t.config, &event_scope, &event_key))
                .cloned()
                .collect()
        };

        // No matching trigger → skip the eval span and the spawn entirely.
        if triggers.is_empty() {
            return;
        }

        let engine = self.engine.clone();
        let event_type = event_data.event_type.clone();

        // The engine attaches the writer's OTel context for the state write
        // (even for suppressed builtins — see invocation::handle_invocation), so
        // parent the spawned trigger fan-out to it. Otherwise `state_triggers`
        // (and the handlers it invokes, e.g. turn::on_approval) would root a new
        // trace disconnected from the writer (e.g. approval::resolve).
        let parent_cx = opentelemetry::Context::current();

        if let Ok(event_data) = serde_json::to_value(event_data) {
            let trigger_span = {
                let _guard = parent_cx.attach();
                tracing::info_span!(
                    "state_triggers",
                    "iii.function.kind" = "internal",
                    otel.status_code = tracing::field::Empty
                )
            };
            tokio::spawn(
                async move {
                    tracing::debug!("Invoking triggers for event type {:?}", event_type);
                    let mut has_error = false;

                    for trigger in triggers {
                        // Scope/key already matched in the pre-filter above.
                        if let Some(ref condition_id) = trigger.config.condition_function_id {
                            tracing::debug!(
                                condition_function_id = %condition_id,
                                "Checking trigger conditions"
                            );
                            match check_condition(engine.as_ref(), condition_id, event_data.clone())
                                .await
                            {
                                Ok(true) => {}
                                Ok(false) => {
                                    tracing::debug!(
                                        function_id = %trigger.trigger.function_id,
                                        "Condition check failed, skipping handler"
                                    );
                                    continue;
                                }
                                Err(err) => {
                                    has_error = true;
                                    tracing::error!(
                                        condition_function_id = %condition_id,
                                        error = ?err,
                                        "Error invoking condition function"
                                    );
                                    continue;
                                }
                            }
                        }

                        // Invoke the handler function
                        tracing::info!(
                            function_id = %trigger.trigger.function_id,
                            "Invoking trigger"
                        );

                        let call_result = engine
                            .call(&trigger.trigger.function_id, event_data.clone())
                            .await;

                        match call_result {
                            Ok(_) => {
                                tracing::debug!(
                                    function_id = %trigger.trigger.function_id,
                                    "Trigger handler invoked successfully"
                                );
                            }
                            Err(err) => {
                                has_error = true;
                                tracing::error!(
                                    function_id = %trigger.trigger.function_id,
                                    error = ?err,
                                    "Error invoking trigger handler"
                                );
                            }
                        }
                    }

                    if has_error {
                        tracing::Span::current().record("otel.status_code", "ERROR");
                    } else {
                        tracing::Span::current().record("otel.status_code", "OK");
                    }
                }
                .instrument(trigger_span),
            );
        } else {
            tracing::error!("Failed to convert event data to value");
        }
    }
}

#[service(name = "state")]
impl StateWorker {
    #[function(id = "state::set", description = "Set a value in state")]
    pub async fn set(&self, input: StateSetInput) -> FunctionResult<SetResult, ErrorBody> {
        crate::workers::telemetry::collector::track_state_set();

        // LIVE guard: reject oversized values before touching the adapter.
        // `max_value_bytes` is read from the live snapshot, so the limit can be
        // tuned at runtime. Unset => no limit.
        if let Some(limit) = self.config_snapshot().max_value_bytes {
            let size = serde_json::to_vec(&input.value)
                .map(|bytes| bytes.len())
                .unwrap_or(0);
            if size > limit {
                return FunctionResult::Failure(ErrorBody {
                    message: format!(
                        "value of {size} bytes exceeds the configured max_value_bytes limit of {limit}"
                    ),
                    code: "VALUE_TOO_LARGE".to_string(),
                    stacktrace: None,
                });
            }
        }

        match self
            .adapter
            .set(&input.scope, &input.key, input.value.clone())
            .await
        {
            Ok(value) => {
                let old_value = value.old_value.clone();
                let new_value = value.new_value.clone();
                let is_create = old_value.is_none();
                // Invoke triggers after successful set
                let event_data = StateEventData {
                    message_type: "state".to_string(),
                    event_type: if is_create {
                        StateEventType::Created
                    } else {
                        StateEventType::Updated
                    },
                    scope: input.scope,
                    key: input.key,
                    old_value,
                    new_value: new_value.clone(),
                };

                self.invoke_triggers(event_data).await;

                FunctionResult::Success(value)
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to set value: {}", e),
                code: "SET_ERROR".to_string(),
                stacktrace: None,
            }),
        }
    }

    #[function(id = "state::get", description = "Get a value from state")]
    pub async fn get(&self, input: StateGetInput) -> FunctionResult<Option<Value>, ErrorBody> {
        crate::workers::telemetry::collector::track_state_get();
        match self.adapter.get(&input.scope, &input.key).await {
            Ok(value) => FunctionResult::Success(value),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to get value: {}", e),
                code: "GET_ERROR".to_string(),
                stacktrace: None,
            }),
        }
    }

    #[function(id = "state::delete", description = "Delete a value from state")]
    pub async fn delete(
        &self,
        input: StateDeleteInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        let value = match self.adapter.get(&input.scope, &input.key).await {
            Ok(v) => v,
            Err(e) => {
                return FunctionResult::Failure(ErrorBody {
                    message: format!("Failed to get value before delete: {}", e),
                    code: "GET_ERROR".to_string(),
                    stacktrace: None,
                });
            }
        };

        crate::workers::telemetry::collector::track_state_delete();
        match self.adapter.delete(&input.scope, &input.key).await {
            Ok(_) => {
                // Invoke triggers after successful delete
                let event_data = StateEventData {
                    message_type: "state".to_string(),
                    event_type: StateEventType::Deleted,
                    scope: input.scope,
                    key: input.key,
                    old_value: value.clone(),
                    new_value: Value::Null,
                };

                self.invoke_triggers(event_data).await;

                FunctionResult::Success(value)
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to delete value: {}", e),
                code: "DELETE_ERROR".to_string(),
                stacktrace: None,
            }),
        }
    }

    #[function(id = "state::update", description = "Update a value in state")]
    pub async fn update(&self, input: StateUpdateInput) -> FunctionResult<UpdateResult, ErrorBody> {
        crate::workers::telemetry::collector::track_state_update();
        match self
            .adapter
            .update(&input.scope, &input.key, input.ops)
            .await
        {
            Ok(value) => {
                let old_value = value.old_value.clone();
                let new_value = value.new_value.clone();
                let is_create = old_value.is_none();
                let event_data = StateEventData {
                    message_type: "state".to_string(),
                    event_type: if is_create {
                        StateEventType::Created
                    } else {
                        StateEventType::Updated
                    },
                    scope: input.scope,
                    key: input.key,
                    old_value,
                    new_value,
                };

                self.invoke_triggers(event_data).await;

                FunctionResult::Success(value)
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to update value: {}", e),
                code: "UPDATE_ERROR".to_string(),
                stacktrace: None,
            }),
        }
    }

    #[function(
        id = "state::list",
        description = "List key/value entries from a state scope"
    )]
    pub async fn list(
        &self,
        input: StateGetGroupInput,
    ) -> FunctionResult<Option<Value>, ErrorBody> {
        match self.adapter.list(&input.scope).await {
            Ok(values) => FunctionResult::Success(serde_json::to_value(values).ok()),
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to list values: {}", e),
                code: "LIST_ERROR".to_string(),
                stacktrace: None,
            }),
        }
    }

    #[function(id = "state::list_groups", description = "List all state groups")]
    pub async fn list_groups(
        &self,
        _input: StateListGroupsInput,
    ) -> FunctionResult<StateListGroupsResult, ErrorBody> {
        match self.adapter.list_groups().await {
            Ok(groups) => {
                let mut normalized_groups: Vec<String> = groups
                    .into_iter()
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();
                normalized_groups.sort();
                FunctionResult::Success(StateListGroupsResult {
                    groups: normalized_groups,
                })
            }
            Err(e) => FunctionResult::Failure(ErrorBody {
                message: format!("Failed to list groups: {}", e),
                code: "LIST_GROUPS_ERROR".to_string(),
                stacktrace: None,
            }),
        }
    }
}

crate::register_worker!(
    "iii-state",
    StateWorker,
    description = "Distributed key-value state management with reactive change triggers.",
    enabled_by_default = true
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::{
        observability::metrics::ensure_default_meter,
        state::{adapters::kv_store::BuiltinKvStoreAdapter, structs::StateListItem},
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn state_trigger_matches_respects_scope_and_key() {
        use super::super::trigger::StateTriggerConfig;
        let cfg = |scope: Option<&str>, key: Option<&str>| StateTriggerConfig {
            scope: scope.map(String::from),
            key: key.map(String::from),
            condition_function_id: None,
        };
        // No scope/key constraints → matches any write.
        assert!(state_trigger_matches(&cfg(None, None), "orders", "o1"));
        // Scope constraint only.
        assert!(state_trigger_matches(
            &cfg(Some("orders"), None),
            "orders",
            "o1"
        ));
        assert!(!state_trigger_matches(
            &cfg(Some("orders"), None),
            "users",
            "o1"
        ));
        // Key constraint only.
        assert!(state_trigger_matches(
            &cfg(None, Some("special")),
            "s",
            "special"
        ));
        assert!(!state_trigger_matches(
            &cfg(None, Some("special")),
            "s",
            "other"
        ));
        // Both constraints.
        assert!(state_trigger_matches(
            &cfg(Some("orders"), Some("o1")),
            "orders",
            "o1"
        ));
        assert!(!state_trigger_matches(
            &cfg(Some("orders"), Some("o1")),
            "orders",
            "o2"
        ));
        assert!(!state_trigger_matches(
            &cfg(Some("orders"), Some("o1")),
            "users",
            "o1"
        ));
    }

    struct FakeStateAdapter {
        set_error: Option<&'static str>,
        get_error: Option<&'static str>,
        delete_error: Option<&'static str>,
        update_error: Option<&'static str>,
        list_error: Option<&'static str>,
        list_groups_error: Option<&'static str>,
        destroy_error: Option<&'static str>,
        set_result: SetResult,
        get_result: Option<Value>,
        update_result: UpdateResult,
        list_values: Vec<StateListItem>,
        groups: Vec<String>,
        destroy_called: AtomicBool,
    }

    impl Default for FakeStateAdapter {
        fn default() -> Self {
            Self {
                set_error: None,
                get_error: None,
                delete_error: None,
                update_error: None,
                list_error: None,
                list_groups_error: None,
                destroy_error: None,
                set_result: SetResult {
                    old_value: None,
                    new_value: serde_json::json!({"ok": true}),
                },
                get_result: None,
                update_result: UpdateResult {
                    old_value: None,
                    new_value: serde_json::json!({"ok": true}),
                    errors: Vec::new(),
                },
                list_values: Vec::new(),
                groups: Vec::new(),
                destroy_called: AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl StateAdapter for FakeStateAdapter {
        async fn set(&self, _scope: &str, _key: &str, _value: Value) -> anyhow::Result<SetResult> {
            match self.set_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.set_result.clone()),
            }
        }

        async fn get(&self, _scope: &str, _key: &str) -> anyhow::Result<Option<Value>> {
            match self.get_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.get_result.clone()),
            }
        }

        async fn delete(&self, _scope: &str, _key: &str) -> anyhow::Result<()> {
            match self.delete_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(()),
            }
        }

        async fn update(
            &self,
            _scope: &str,
            _key: &str,
            _ops: Vec<iii_sdk::UpdateOp>,
        ) -> anyhow::Result<UpdateResult> {
            match self.update_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.update_result.clone()),
            }
        }

        async fn list(&self, _scope: &str) -> anyhow::Result<Vec<StateListItem>> {
            match self.list_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.list_values.clone()),
            }
        }

        async fn list_groups(&self) -> anyhow::Result<Vec<String>> {
            match self.list_groups_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(self.groups.clone()),
            }
        }

        async fn destroy(&self) -> anyhow::Result<()> {
            self.destroy_called.store(true, Ordering::Relaxed);
            match self.destroy_error {
                Some(message) => Err(anyhow::anyhow!(message)),
                None => Ok(()),
            }
        }
    }

    fn setup() -> (Arc<Engine>, StateWorker) {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let adapter: Arc<dyn StateAdapter> = Arc::new(BuiltinKvStoreAdapter::new(None));
        let module =
            StateWorker::from_config(engine.clone(), StateModuleConfig::default(), adapter);
        (engine, module)
    }

    fn setup_with_adapter(adapter: Arc<dyn StateAdapter>) -> (Arc<Engine>, StateWorker) {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let module =
            StateWorker::from_config(engine.clone(), StateModuleConfig::default(), adapter);
        (engine, module)
    }

    fn setup_with_config(config: StateModuleConfig) -> (Arc<Engine>, StateWorker) {
        ensure_default_meter();
        let engine = Arc::new(crate::engine::Engine::new());
        let adapter: Arc<dyn StateAdapter> = Arc::new(BuiltinKvStoreAdapter::new(None));
        let module = StateWorker::from_config(engine.clone(), config, adapter);
        (engine, module)
    }

    #[tokio::test]
    async fn triggers_disabled_skips_fan_out() {
        let (engine, module) = setup_with_config(StateModuleConfig {
            triggers_enabled: Some(false),
            ..Default::default()
        });
        register_panic_handler(&engine); // "test::handler" panics if invoked

        let trigger = crate::trigger::Trigger {
            id: "state-trig-disabled".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::handler".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
        };
        let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
            trigger.config.clone(),
        )
        .unwrap();
        module.triggers.list.write().await.insert(
            trigger.id.clone(),
            super::super::trigger::StateTrigger { config, trigger },
        );

        // triggers_enabled=false gates fan-out entirely; the panic handler must
        // never be invoked.
        module
            .invoke_triggers(super::super::structs::StateEventData {
                message_type: "state".to_string(),
                event_type: super::super::structs::StateEventType::Created,
                scope: "s".to_string(),
                key: "k".to_string(),
                old_value: None,
                new_value: serde_json::json!(1),
            })
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn set_rejects_value_over_max_bytes() {
        let (_engine, module) = setup_with_config(StateModuleConfig {
            max_value_bytes: Some(10),
            ..Default::default()
        });

        let oversized = module
            .set(StateSetInput {
                scope: "s".to_string(),
                key: "big".to_string(),
                value: serde_json::json!("a value that is definitely longer than ten bytes"),
            })
            .await;
        match oversized {
            FunctionResult::Failure(err) => assert_eq!(err.code, "VALUE_TOO_LARGE"),
            _ => panic!("expected VALUE_TOO_LARGE"),
        }

        // A value within the limit still writes.
        let small = module
            .set(StateSetInput {
                scope: "s".to_string(),
                key: "ok".to_string(),
                value: serde_json::json!(1),
            })
            .await;
        assert!(matches!(small, FunctionResult::Success(_)));
    }

    #[tokio::test]
    async fn is_active_reflects_worker_shutdown_rx() {
        let (_engine, module) = setup();
        assert!(!module.is_active());

        let (_tx, rx) = tokio::sync::watch::channel(false);
        *module
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex") = Some(rx);
        assert!(module.is_active());

        module
            .worker_shutdown_rx
            .lock()
            .expect("worker_shutdown_rx mutex")
            .take();
        assert!(!module.is_active());
    }

    #[test]
    fn adapter_config_folds_top_level_save_interval() {
        // The authoritative top-level save_interval_ms is injected into the
        // adapter config blob so the boot-constructed kv store honors it.
        let config = StateModuleConfig {
            adapter: Some(crate::workers::traits::AdapterEntry {
                name: "kv".to_string(),
                config: Some(serde_json::json!({ "store_method": "file_based" })),
            }),
            save_interval_ms: Some(750),
            ..Default::default()
        };
        let adapter_config =
            <StateWorker as ConfigurableWorker>::adapter_config_from_config(&config)
                .expect("adapter config present");
        assert_eq!(adapter_config["save_interval_ms"], 750);
        assert_eq!(adapter_config["store_method"], "file_based");
    }

    #[test]
    fn adapter_config_injects_save_interval_without_inner_block() {
        // Even when the adapter has no inner config, the top-level knob lands.
        let config = StateModuleConfig {
            adapter: Some(crate::workers::traits::AdapterEntry {
                name: "kv".to_string(),
                config: None,
            }),
            save_interval_ms: Some(900),
            ..Default::default()
        };
        let adapter_config =
            <StateWorker as ConfigurableWorker>::adapter_config_from_config(&config)
                .expect("adapter config present");
        assert_eq!(adapter_config["save_interval_ms"], 900);
    }

    fn register_panic_handler(engine: &Arc<Engine>) {
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::handler".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(|_input: Value| async move {
                panic!("handler should not be called")
            }),
        );
    }

    // ---- set/get/delete/list operations ----

    #[tokio::test]
    async fn test_set_and_get() {
        let (_engine, module) = setup();

        let set_input = StateSetInput {
            scope: "test-scope".to_string(),
            key: "key1".to_string(),
            value: serde_json::json!({"name": "Alice"}),
        };
        let result = module.set(set_input).await;
        assert!(matches!(result, FunctionResult::Success(_)));

        let get_input = StateGetInput {
            scope: "test-scope".to_string(),
            key: "key1".to_string(),
        };
        let result = module.get(get_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!({"name": "Alice"}));
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_get_nonexistent_key() {
        let (_engine, module) = setup();

        let get_input = StateGetInput {
            scope: "test-scope".to_string(),
            key: "nonexistent".to_string(),
        };
        let result = module.get(get_input).await;
        assert!(matches!(result, FunctionResult::Success(None)));
    }

    #[tokio::test]
    async fn test_set_overwrites_existing() {
        let (_engine, module) = setup();

        let set_input = StateSetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            value: serde_json::json!(1),
        };
        module.set(set_input).await;

        let set_input = StateSetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            value: serde_json::json!(2),
        };
        let result = module.set(set_input).await;

        // Verify the old value is in the set result
        if let FunctionResult::Success(val) = &result {
            assert_eq!(val.old_value, Some(serde_json::json!(1)));
            assert_eq!(val.new_value, serde_json::json!(2));
        } else {
            panic!("Expected Success with SetResult");
        }

        // Verify get returns updated value
        let get_input = StateGetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
        };
        let result = module.get(get_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!(2));
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_delete_existing_key() {
        let (_engine, module) = setup();

        let set_input = StateSetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            value: serde_json::json!("hello"),
        };
        module.set(set_input).await;

        let delete_input = StateDeleteInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
        };
        let result = module.delete(delete_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value, serde_json::json!("hello"));
            }
            _ => panic!("Expected Success with deleted value"),
        }

        // Verify it's gone
        let get_input = StateGetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
        };
        let result = module.get(get_input).await;
        assert!(matches!(result, FunctionResult::Success(None)));
    }

    #[tokio::test]
    async fn test_delete_nonexistent_key() {
        let (_engine, module) = setup();

        let delete_input = StateDeleteInput {
            scope: "scope".to_string(),
            key: "nonexistent".to_string(),
        };
        let result = module.delete(delete_input).await;
        // Should succeed with None (nothing was there to delete)
        assert!(matches!(result, FunctionResult::Success(None)));
    }

    #[tokio::test]
    async fn test_list_scope() {
        let (_engine, module) = setup();

        // Set multiple items in the same scope
        for i in 0..3 {
            let set_input = StateSetInput {
                scope: "my-scope".to_string(),
                key: format!("item-{}", i),
                value: serde_json::json!({"index": i}),
            };
            module.set(set_input).await;
        }

        let list_input = StateGetGroupInput {
            scope: "my-scope".to_string(),
        };
        let result = module.list(list_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                let arr = value.as_array().expect("list should return array");
                assert_eq!(arr.len(), 3);
                let mut entries = arr.clone();
                entries.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
                for (i, entry) in entries.iter().enumerate() {
                    assert_eq!(entry["key"], format!("item-{i}"));
                    assert_eq!(entry["value"], serde_json::json!({"index": i}));
                }
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_list_empty_scope() {
        let (_engine, module) = setup();

        let list_input = StateGetGroupInput {
            scope: "empty-scope".to_string(),
        };
        let result = module.list(list_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                let arr = value.as_array().expect("list should return array");
                assert!(arr.is_empty());
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_list_groups() {
        let (_engine, module) = setup();

        // Set items in different scopes
        for scope in &["alpha", "beta", "gamma"] {
            let set_input = StateSetInput {
                scope: scope.to_string(),
                key: "k".to_string(),
                value: serde_json::json!(1),
            };
            module.set(set_input).await;
        }

        let result = module.list_groups(StateListGroupsInput {}).await;
        match result {
            FunctionResult::Success(value) => {
                assert_eq!(value.groups.len(), 3);
                // They should be sorted
                assert_eq!(value.groups, vec!["alpha", "beta", "gamma"]);
            }
            _ => panic!("Expected Success with StateListGroupsResult"),
        }
    }

    // ---- invoke_triggers tests ----

    #[tokio::test]
    async fn test_invoke_triggers_scope_filter() {
        let (_engine, module) = setup();
        register_panic_handler(&_engine);

        // Register a trigger that only matches scope "orders"
        let trigger = crate::trigger::Trigger {
            id: "state-trig-1".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::handler".to_string(),
            config: serde_json::json!({
                "scope": "orders"
            }),
            worker_id: None,
            metadata: None,
        };

        {
            let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
                trigger.config.clone(),
            )
            .unwrap();
            module.triggers.list.write().await.insert(
                trigger.id.clone(),
                super::super::trigger::StateTrigger {
                    config,
                    trigger: trigger.clone(),
                },
            );
        }

        // Event with different scope should be filtered out
        let event_data = super::super::structs::StateEventData {
            message_type: "state".to_string(),
            event_type: super::super::structs::StateEventType::Created,
            scope: "users".to_string(),
            key: "user-1".to_string(),
            old_value: None,
            new_value: serde_json::json!({"name": "Alice"}),
        };

        // This should not panic even though the function doesn't exist -
        // the scope filter should skip it
        module.invoke_triggers(event_data).await;

        // Give the spawned task time to complete
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_invoke_triggers_key_filter() {
        let (_engine, module) = setup();
        register_panic_handler(&_engine);

        // Register a trigger that only matches key "special-key"
        let trigger = crate::trigger::Trigger {
            id: "state-trig-2".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::handler".to_string(),
            config: serde_json::json!({
                "key": "special-key"
            }),
            worker_id: None,
            metadata: None,
        };

        {
            let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
                trigger.config.clone(),
            )
            .unwrap();
            module.triggers.list.write().await.insert(
                trigger.id.clone(),
                super::super::trigger::StateTrigger {
                    config,
                    trigger: trigger.clone(),
                },
            );
        }

        // Event with different key should be filtered out
        let event_data = super::super::structs::StateEventData {
            message_type: "state".to_string(),
            event_type: super::super::structs::StateEventType::Updated,
            scope: "scope".to_string(),
            key: "other-key".to_string(),
            old_value: Some(serde_json::json!(1)),
            new_value: serde_json::json!(2),
        };

        module.invoke_triggers(event_data).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_invoke_triggers_matching_trigger_calls_function() {
        let (engine, module) = setup();

        // Register a function that the trigger will call
        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::on_state_change".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        // Register a trigger with no scope/key filter (matches everything)
        let trigger = crate::trigger::Trigger {
            id: "state-trig-all".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::on_state_change".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
        };

        {
            let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
                trigger.config.clone(),
            )
            .unwrap();
            module.triggers.list.write().await.insert(
                trigger.id.clone(),
                super::super::trigger::StateTrigger {
                    config,
                    trigger: trigger.clone(),
                },
            );
        }

        let event_data = super::super::structs::StateEventData {
            message_type: "state".to_string(),
            event_type: super::super::structs::StateEventType::Created,
            scope: "test-scope".to_string(),
            key: "test-key".to_string(),
            old_value: None,
            new_value: serde_json::json!({"data": true}),
        };

        module.invoke_triggers(event_data).await;

        let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx)
            .await
            .expect("timed out waiting for trigger")
            .expect("channel closed");

        assert_eq!(received["scope"], "test-scope");
        assert_eq!(received["key"], "test-key");
    }

    #[tokio::test]
    async fn test_update_operation() {
        let (_engine, module) = setup();

        // First set a value
        let set_input = StateSetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            value: serde_json::json!({"count": 0, "name": "test"}),
        };
        module.set(set_input).await;

        // Update using ops
        let update_input = StateUpdateInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
            ops: vec![iii_sdk::UpdateOp::Set {
                path: "count".to_string(),
                value: Some(serde_json::json!(42)),
            }],
        };
        let result = module.update(update_input).await;
        assert!(matches!(result, FunctionResult::Success(_)));

        // Verify the update
        let get_input = StateGetInput {
            scope: "scope".to_string(),
            key: "k".to_string(),
        };
        let result = module.get(get_input).await;
        match result {
            FunctionResult::Success(Some(value)) => {
                assert_eq!(value["count"], 42);
                assert_eq!(value["name"], "test");
            }
            _ => panic!("Expected Success with Some value"),
        }
    }

    #[tokio::test]
    async fn test_initialize_registers_trigger_type_and_destroy_calls_adapter() {
        let adapter = Arc::new(FakeStateAdapter::default());
        let (engine, module) = setup_with_adapter(adapter.clone());

        module.initialize().await.unwrap();
        assert!(
            engine
                .trigger_registry
                .trigger_types
                .contains_key(TRIGGER_TYPE)
        );

        module.destroy().await.unwrap();
        assert!(adapter.destroy_called.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_set_returns_failure_when_adapter_errors() {
        let adapter = Arc::new(FakeStateAdapter {
            set_error: Some("set exploded"),
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module
            .set(StateSetInput {
                scope: "scope".to_string(),
                key: "key".to_string(),
                value: serde_json::json!(1),
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "SET_ERROR"),
            _ => panic!("expected set failure"),
        }
    }

    #[tokio::test]
    async fn test_delete_returns_get_error_when_lookup_fails() {
        let adapter = Arc::new(FakeStateAdapter {
            get_error: Some("missing backend"),
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module
            .delete(StateDeleteInput {
                scope: "scope".to_string(),
                key: "key".to_string(),
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "GET_ERROR"),
            _ => panic!("expected delete lookup failure"),
        }
    }

    #[tokio::test]
    async fn test_update_returns_failure_when_adapter_errors() {
        let adapter = Arc::new(FakeStateAdapter {
            update_error: Some("update exploded"),
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module
            .update(StateUpdateInput {
                scope: "scope".to_string(),
                key: "key".to_string(),
                ops: vec![],
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "UPDATE_ERROR"),
            _ => panic!("expected update failure"),
        }
    }

    #[tokio::test]
    async fn test_list_returns_failure_when_adapter_errors() {
        let adapter = Arc::new(FakeStateAdapter {
            list_error: Some("list exploded"),
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module
            .list(StateGetGroupInput {
                scope: "scope".to_string(),
            })
            .await;

        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "LIST_ERROR"),
            _ => panic!("expected list failure"),
        }
    }

    #[tokio::test]
    async fn test_list_groups_deduplicates_and_sorts_adapter_values() {
        let adapter = Arc::new(FakeStateAdapter {
            groups: vec![
                "beta".to_string(),
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
            ],
            ..Default::default()
        });
        let (_engine, module) = setup_with_adapter(adapter);

        let result = module.list_groups(StateListGroupsInput {}).await;

        match result {
            FunctionResult::Success(value) => {
                assert_eq!(value.groups, vec!["alpha", "beta", "gamma"]);
            }
            _ => panic!("expected list_groups success"),
        }
    }

    #[tokio::test]
    async fn test_invoke_triggers_condition_none_allows_handler() {
        // The centralized check_condition treats Ok(None) as Ok(true),
        // so a condition function returning Success(None) does NOT skip the handler.
        let (engine, module) = setup();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::condition_none".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(
                |_input: Value| async move { FunctionResult::Success(None) },
            ),
        );

        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::state_handler".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        let trigger = crate::trigger::Trigger {
            id: "state-trig-condition-none".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::state_handler".to_string(),
            config: serde_json::json!({
                "condition_function_id": "test::condition_none"
            }),
            worker_id: None,
            metadata: None,
        };

        let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
            trigger.config.clone(),
        )
        .unwrap();
        module.triggers.list.write().await.insert(
            trigger.id.clone(),
            super::super::trigger::StateTrigger { config, trigger },
        );

        module
            .invoke_triggers(super::super::structs::StateEventData {
                message_type: "state".to_string(),
                event_type: super::super::structs::StateEventType::Updated,
                scope: "scope".to_string(),
                key: "key".to_string(),
                old_value: Some(serde_json::json!(1)),
                new_value: serde_json::json!(2),
            })
            .await;

        // Handler should be invoked because check_condition returns Ok(true) for None
        let result = tokio::time::timeout(std::time::Duration::from_millis(500), rx).await;
        assert!(
            result.is_ok(),
            "handler should have been called since condition None means Ok(true)"
        );
    }

    #[tokio::test]
    async fn test_invoke_triggers_condition_error_skips_handler() {
        let (engine, module) = setup();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::condition_error".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(|_input: Value| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "COND".to_string(),
                    message: "bad".to_string(),
                    stacktrace: None,
                })
            }),
        );

        let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
        let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
        let tx_clone = tx.clone();
        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::state_handler_error_skip".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(move |input: Value| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(input);
                    }
                    FunctionResult::Success(None)
                }
            }),
        );

        let trigger = crate::trigger::Trigger {
            id: "state-trig-condition-error".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::state_handler_error_skip".to_string(),
            config: serde_json::json!({
                "condition_function_id": "test::condition_error"
            }),
            worker_id: None,
            metadata: None,
        };

        let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
            trigger.config.clone(),
        )
        .unwrap();
        module.triggers.list.write().await.insert(
            trigger.id.clone(),
            super::super::trigger::StateTrigger { config, trigger },
        );

        module
            .invoke_triggers(super::super::structs::StateEventData {
                message_type: "state".to_string(),
                event_type: super::super::structs::StateEventType::Updated,
                scope: "scope".to_string(),
                key: "key".to_string(),
                old_value: Some(serde_json::json!(1)),
                new_value: serde_json::json!(2),
            })
            .await;

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), rx)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_invoke_triggers_handler_error_is_tolerated() {
        let (engine, module) = setup();

        engine.register_function_handler(
            crate::engine::RegisterFunctionRequest {
                function_id: "test::failing_state_handler".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(|_input: Value| async move {
                FunctionResult::Failure(ErrorBody {
                    code: "HANDLER".to_string(),
                    message: "failed".to_string(),
                    stacktrace: None,
                })
            }),
        );

        let trigger = crate::trigger::Trigger {
            id: "state-trig-handler-error".to_string(),
            trigger_type: "state".to_string(),
            function_id: "test::failing_state_handler".to_string(),
            config: serde_json::json!({}),
            worker_id: None,
            metadata: None,
        };

        let config = serde_json::from_value::<super::super::trigger::StateTriggerConfig>(
            trigger.config.clone(),
        )
        .unwrap();
        module.triggers.list.write().await.insert(
            trigger.id.clone(),
            super::super::trigger::StateTrigger { config, trigger },
        );

        module
            .invoke_triggers(super::super::structs::StateEventData {
                message_type: "state".to_string(),
                event_type: super::super::structs::StateEventType::Created,
                scope: "scope".to_string(),
                key: "key".to_string(),
                old_value: None,
                new_value: serde_json::json!(true),
            })
            .await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
