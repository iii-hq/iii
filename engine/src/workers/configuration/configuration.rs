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
use tokio::sync::Mutex as TokioMutex;
use tracing::Instrument;

use crate::{
    condition::check_condition,
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::ErrorBody,
    trigger::TriggerType,
    workers::{
        configuration::{
            adapters::{ConfigurationAdapter, ExternalChange, RegisterKind},
            config::ConfigurationModuleConfig,
            store::{ConfigurationStore, StoreError, expand_value, validate_against_schema},
            structs::{
                ConfigurationEntry, ConfigurationEventData, ConfigurationEventType,
                ConfigurationGetInput, ConfigurationGetResult, ConfigurationListInput,
                ConfigurationListResult, ConfigurationRegisterInput, ConfigurationSchemaInput,
                ConfigurationSchemaView, ConfigurationSetInput, ConfigurationSetResult,
            },
            trigger::{ConfigurationTriggers, TRIGGER_TYPE},
        },
        traits::{AdapterFactory, ConfigurableWorker, Worker},
    },
};

#[derive(Clone)]
pub struct ConfigurationWorker {
    pub(crate) store: Arc<ConfigurationStore>,
    pub(crate) engine: Arc<Engine>,
    pub(crate) triggers: Arc<ConfigurationTriggers>,
    pub(crate) ttl_seconds: u64,
    /// Holds the watcher loop handle so destroy() can stop external-edit
    /// fan-out before the adapter is torn down.
    watch_task: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
}

#[async_trait::async_trait]
impl Worker for ConfigurationWorker {
    fn name(&self) -> &'static str {
        "ConfigurationWorker"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
        Self::create_with_adapters(engine, config).await
    }

    fn register_functions(&self, engine: Arc<Engine>) {
        self.register_functions(engine);
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!("Initializing ConfigurationWorker");

        // Pull existing entries off the adapter into the in-memory cache so
        // `list` / `get` work without an extra round-trip per call.
        if let Err(err) = self.store.prime_from_adapter().await {
            tracing::warn!(
                error = %err,
                "Failed to prime configuration cache from adapter; starting empty"
            );
        }

        let _ = self
            .engine
            .register_trigger_type(TriggerType::new(
                TRIGGER_TYPE,
                "Configuration trigger — fires on register/update/delete events",
                Box::new(self.clone()),
                None,
            ))
            .await;

        // Start the adapter's external-change watcher (no-op for adapters
        // without an out-of-band edit path).
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExternalChange>();
        if let Err(err) = self.store.adapter().watch(tx).await {
            tracing::warn!(
                error = %err,
                "Adapter watch failed; external edits will not fire triggers"
            );
        } else {
            let worker = self.clone();
            let handle = tokio::spawn(async move {
                while let Some(change) = rx.recv().await {
                    worker.handle_external_change(change).await;
                }
            });
            *self.watch_task.lock().await = Some(handle);
        }

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying ConfigurationWorker");
        self.triggers.abort_all_expiries().await;
        if let Some(handle) = self.watch_task.lock().await.take() {
            handle.abort();
        }
        self.store.adapter().destroy().await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ConfigurableWorker for ConfigurationWorker {
    type Config = ConfigurationModuleConfig;
    type Adapter = dyn ConfigurationAdapter;
    type AdapterRegistration = super::registry::ConfigurationAdapterRegistration;
    const DEFAULT_ADAPTER_NAME: &'static str = super::adapters::fs::ADAPTER_NAME;

    async fn registry() -> &'static SyncRwLock<HashMap<String, AdapterFactory<Self::Adapter>>> {
        static REGISTRY: Lazy<
            SyncRwLock<HashMap<String, AdapterFactory<dyn ConfigurationAdapter>>>,
        > = Lazy::new(|| SyncRwLock::new(ConfigurationWorker::build_registry()));
        &REGISTRY
    }

    fn build(engine: Arc<Engine>, config: Self::Config, adapter: Arc<Self::Adapter>) -> Self {
        Self {
            store: Arc::new(ConfigurationStore::new(adapter)),
            engine,
            triggers: Arc::new(ConfigurationTriggers::new()),
            ttl_seconds: config.ttl_seconds,
            watch_task: Arc::new(TokioMutex::new(None)),
        }
    }

    fn adapter_name_from_config(config: &Self::Config) -> Option<String> {
        config.adapter.as_ref().map(|a| a.name.clone())
    }

    fn adapter_config_from_config(config: &Self::Config) -> Option<Value> {
        config.adapter.as_ref().and_then(|a| a.config.clone())
    }
}

impl ConfigurationWorker {
    /// Construct a worker with explicit pieces — used by tests so they don't
    /// have to round-trip through `create_with_adapters`. Exposed publicly
    /// (with `#[doc(hidden)]`) so integration tests in `engine/tests/` can
    /// drive the worker without booting the full engine.
    #[doc(hidden)]
    pub fn for_test(
        engine: Arc<Engine>,
        adapter: Arc<dyn ConfigurationAdapter>,
        ttl_seconds: u64,
    ) -> Self {
        Self {
            store: Arc::new(ConfigurationStore::new(adapter)),
            engine,
            triggers: Arc::new(ConfigurationTriggers::new()),
            ttl_seconds,
            watch_task: Arc::new(TokioMutex::new(None)),
        }
    }

    /// Trigger fan-out for an event already produced by the worker
    /// (register/set) or surfaced by the adapter watcher (file edit).
    pub(crate) async fn fan_out(&self, event: ConfigurationEventData) {
        let triggers = self.triggers.matching(&event.id).await;
        if triggers.is_empty() {
            return;
        }

        let event_value = match serde_json::to_value(&event) {
            Ok(v) => v,
            Err(err) => {
                tracing::error!(error = %err, "Failed to serialise configuration event");
                return;
            }
        };
        let engine = self.engine.clone();
        let event_type_wire = match event.event_type {
            ConfigurationEventType::Registered => "configuration:registered",
            ConfigurationEventType::Updated => "configuration:updated",
            ConfigurationEventType::Deleted => "configuration:deleted",
        };
        let id = event.id.clone();
        let current_span = tracing::Span::current();

        tokio::spawn(
            async move {
                for trigger in triggers {
                    if let Some(filter) = trigger.config.event_types.as_ref()
                        && !filter.iter().any(|t| t == event_type_wire)
                    {
                        continue;
                    }

                    if let Some(condition_id) = trigger.config.condition_function_id.as_ref() {
                        match check_condition(
                            engine.as_ref(),
                            &trigger.trigger.namespace,
                            condition_id,
                            event_value.clone(),
                        )
                        .await
                        {
                            Ok(true) => {}
                            Ok(false) => {
                                tracing::debug!(
                                    function_id = %trigger.trigger.function_id,
                                    "Condition returned false, skipping handler"
                                );
                                continue;
                            }
                            Err(err) => {
                                tracing::error!(
                                    condition_function_id = %condition_id,
                                    error = ?err,
                                    "Condition function errored, skipping handler"
                                );
                                continue;
                            }
                        }
                    }

                    if let Err(err) = engine
                        .call_with_metadata_ns(
                            &trigger.trigger.namespace,
                            &trigger.trigger.function_id,
                            event_value.clone(),
                            trigger.trigger.metadata.clone(),
                        )
                        .await
                    {
                        tracing::error!(
                            function_id = %trigger.trigger.function_id,
                            id = %id,
                            error = ?err,
                            "Configuration trigger handler failed"
                        );
                    }
                }
            }
            .instrument(tracing::info_span!(parent: current_span, "configuration_triggers", "iii.function.kind" = "internal")),
        );
    }

    /// Apply a watcher-surfaced change into the cache and broadcast.
    ///
    /// A hand-edit of a `<id>.yaml` file must not be able to clobber the running
    /// config with an invalid value. For create/update edits we validate the
    /// APPLIED (env-expanded + coerced) value against the cached schema (the
    /// adapter's watcher carries the real schema forward — disk never holds it);
    /// on failure we log a WARNING and drop the edit, leaving the previous good
    /// value in the store. We never write the file back here, so this path can't
    /// feed the watcher and create a save→reload→save loop.
    pub(crate) async fn handle_external_change(&self, change: ExternalChange) {
        let edited = match &change {
            ExternalChange::Registered(entry) | ExternalChange::Updated { entry, .. } => {
                Some(entry)
            }
            ExternalChange::Deleted { .. } => None,
        };
        if let Some(entry) = edited
            && !entry.schema.is_null()
        {
            let (applied, missing) = expand_value(&entry.value);
            if !missing.is_empty() {
                tracing::warn!(
                    id = %entry.id,
                    vars = %missing.join(", "),
                    "Configuration file edit references unset environment variable(s) with no default; keeping the previous value"
                );
                return;
            }
            if let Err(errs) = validate_against_schema(&applied, &entry.schema) {
                tracing::warn!(
                    id = %entry.id,
                    errors = %errs.join("; "),
                    "Configuration file edit is invalid against its schema; keeping the previous value"
                );
                return;
            }
        }

        self.store.apply_external(&change).await;
        let event = match change {
            ExternalChange::Registered(entry) => entry_to_event(
                &entry,
                ConfigurationEventType::Registered,
                None,
                Some(entry.value.clone()),
            ),
            ExternalChange::Updated { entry, old_value } => entry_to_event(
                &entry,
                ConfigurationEventType::Updated,
                old_value,
                Some(entry.value.clone()),
            ),
            ExternalChange::Deleted { entry } => entry_to_event(
                &entry,
                ConfigurationEventType::Deleted,
                Some(entry.value.clone()),
                None,
            ),
        };
        // Make hot-reload observable: an operator who edits a file on disk
        // should see that the engine picked it up.
        match event.event_type {
            ConfigurationEventType::Registered => tracing::info!(
                id = %event.id,
                "Loaded new configuration '{}' from a file added on disk",
                event.id
            ),
            ConfigurationEventType::Updated => tracing::info!(
                id = %event.id,
                "Reloaded configuration '{}' from an external file edit",
                event.id
            ),
            ConfigurationEventType::Deleted => tracing::info!(
                id = %event.id,
                "Dropped configuration '{}' whose file was removed from disk",
                event.id
            ),
        }
        self.fan_out(event).await;
    }

    /// TTL-driven cleanup. Called from the trigger module when the
    /// last-trigger countdown elapses.
    pub(crate) async fn expire_configuration(&self, id: &str) -> anyhow::Result<()> {
        match self.store.delete(id).await {
            Ok(Some(entry)) => {
                let event = entry_to_event(
                    &entry,
                    ConfigurationEventType::Deleted,
                    Some(entry.value.clone()),
                    None,
                );
                self.fan_out(event).await;
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

/// Build a `ConfigurationEventData` from an entry + event semantics.
/// `new_value` is expanded for the wire (subscribers should see resolved
/// `${VAR:default}` values); the caller chooses the `old_value`.
fn entry_to_event(
    entry: &ConfigurationEntry,
    event_type: ConfigurationEventType,
    old_value: Option<Value>,
    new_value_raw: Option<Value>,
) -> ConfigurationEventData {
    ConfigurationEventData {
        message_type: "configuration".to_string(),
        event_type,
        id: entry.id.clone(),
        name: entry.name.clone(),
        description: entry.description.clone(),
        schema: entry.schema.clone(),
        // Best-effort fan-out: subscribers see resolved `${VAR:default}` values.
        // Unresolved vars are surfaced on the `get` path, not here, so the
        // `missing` list is intentionally dropped.
        old_value: old_value.map(|v| expand_value(&v).0),
        new_value: new_value_raw.map(|v| expand_value(&v).0),
        metadata: entry.metadata.clone(),
    }
}

fn store_error_to_failure(err: StoreError) -> ErrorBody {
    let code = match &err {
        StoreError::NotRegistered(_) => "NOT_REGISTERED",
        StoreError::InvalidId(_) => "INVALID_ID",
        StoreError::SchemaInvalid(_) => "SCHEMA_INVALID",
        StoreError::SchemaUnavailable(_) => "SCHEMA_UNAVAILABLE",
        StoreError::Adapter(_) => "ADAPTER_ERROR",
    };
    ErrorBody {
        message: err.to_string(),
        code: code.to_string(),
        stacktrace: None,
    }
}

#[service(name = "configuration")]
impl ConfigurationWorker {
    #[function(
        id = "configuration::register",
        description = "Register a configuration id with a name, description, and JSON Schema. Idempotent — re-registering replaces metadata and (when initial_value is provided) the value. Validates initial_value against the schema."
    )]
    pub async fn register_fn(
        &self,
        input: ConfigurationRegisterInput,
    ) -> FunctionResult<ConfigurationEntry, ErrorBody> {
        let outcome = match self
            .store
            .register(
                input.id,
                input.name,
                input.description,
                input.schema,
                input.initial_value,
                input.metadata,
            )
            .await
        {
            Ok(o) => o,
            Err(err) => return FunctionResult::Failure(store_error_to_failure(err)),
        };

        let event_type = match outcome.kind {
            RegisterKind::Created => ConfigurationEventType::Registered,
            RegisterKind::Replaced => ConfigurationEventType::Updated,
        };
        let event = entry_to_event(
            &outcome.entry,
            event_type,
            outcome.old_value.clone(),
            Some(outcome.entry.value.clone()),
        );
        self.fan_out(event).await;

        FunctionResult::Success(outcome.entry)
    }

    #[function(
        id = "configuration::set",
        description = "Replace the value of an already-registered configuration. Validates the value against the registered JSON Schema and emits a configuration:updated event."
    )]
    pub async fn set_fn(
        &self,
        input: ConfigurationSetInput,
    ) -> FunctionResult<ConfigurationSetResult, ErrorBody> {
        let outcome = match self.store.set(&input.id, input.value).await {
            Ok(o) => o,
            Err(err) => return FunctionResult::Failure(store_error_to_failure(err)),
        };

        let event = entry_to_event(
            &outcome.entry,
            ConfigurationEventType::Updated,
            outcome.old_value.clone(),
            Some(outcome.entry.value.clone()),
        );
        self.fan_out(event).await;

        FunctionResult::Success(ConfigurationSetResult {
            old_value: outcome.old_value,
            new_value: outcome.entry.value,
        })
    }

    #[function(
        id = "configuration::get",
        description = "Read a configuration by id. Expands ${VAR:default} placeholders against the live process env unless raw=true is passed."
    )]
    pub async fn get_fn(
        &self,
        input: ConfigurationGetInput,
    ) -> FunctionResult<ConfigurationGetResult, ErrorBody> {
        match self.store.get(&input.id).await {
            Some(entry) => {
                if input.raw {
                    return FunctionResult::Success(ConfigurationGetResult {
                        id: entry.id,
                        value: entry.value,
                    });
                }

                // Validate the APPLIED (env-expanded + type-coerced) value, not
                // the raw template. On failure we do NOT hand back a broken
                // value (it would deserialize-fail in the consumer and silently
                // fall back to defaults); instead we log an ERROR naming the id
                // and return a Failure so the consumer keeps its previous /
                // default config. Consumers only special-case NOT_FOUND, so any
                // other code resolves to "not loaded".
                let (applied, missing) = expand_value(&entry.value);
                if !missing.is_empty() {
                    tracing::error!(
                        id = %entry.id,
                        vars = %missing.join(", "),
                        "Configuration references environment variable(s) with no value and no default; it will not be loaded"
                    );
                    return FunctionResult::Failure(ErrorBody {
                        message: format!(
                            "configuration '{}' references unset environment variable(s) with no default: {}",
                            entry.id,
                            missing.join(", ")
                        ),
                        code: "EXPAND_FAILED".to_string(),
                        stacktrace: None,
                    });
                }
                if !entry.schema.is_null()
                    && let Err(errs) = validate_against_schema(&applied, &entry.schema)
                {
                    tracing::error!(
                        id = %entry.id,
                        errors = %errs.join("; "),
                        "Configuration is invalid after environment expansion; it will not be loaded"
                    );
                    return FunctionResult::Failure(ErrorBody {
                        message: format!("schema validation failed: {}", errs.join("; ")),
                        code: "SCHEMA_INVALID".to_string(),
                        stacktrace: None,
                    });
                }
                FunctionResult::Success(ConfigurationGetResult {
                    id: entry.id,
                    value: applied,
                })
            }
            None => FunctionResult::Failure(ErrorBody {
                message: format!("configuration '{}' not found", input.id),
                code: "NOT_FOUND".to_string(),
                stacktrace: None,
            }),
        }
    }

    #[function(
        id = "configuration::list",
        description = "List every registered configuration with id, name, description, and schema. Sorted by id; never returns the stored value."
    )]
    pub async fn list_fn(
        &self,
        _input: ConfigurationListInput,
    ) -> FunctionResult<ConfigurationListResult, ErrorBody> {
        let configurations = self.store.list().await;
        FunctionResult::Success(ConfigurationListResult { configurations })
    }

    #[function(
        id = "configuration::schema",
        description = "Retrieve the schema, name, and description for a configuration id. Mirrors a single entry from configuration::list."
    )]
    pub async fn schema_fn(
        &self,
        input: ConfigurationSchemaInput,
    ) -> FunctionResult<ConfigurationSchemaView, ErrorBody> {
        match self.store.schema_view(&input.id).await {
            Some(view) => FunctionResult::Success(view),
            None => FunctionResult::Failure(ErrorBody {
                message: format!("configuration '{}' not found", input.id),
                code: "NOT_FOUND".to_string(),
                stacktrace: None,
            }),
        }
    }
}

crate::register_worker!(
    "configuration",
    ConfigurationWorker,
    description = "Register, store, and watch typed configuration values for the engine.",
    mandatory
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::configuration::adapters::fs::FsAdapter;
    use serde_json::json;

    async fn setup() -> (Arc<Engine>, ConfigurationWorker, tempfile::TempDir) {
        crate::workers::observability::metrics::ensure_default_meter();
        let dir = tempfile::tempdir().expect("tempdir");
        let adapter = Arc::new(
            FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
                .await
                .expect("fs adapter"),
        ) as Arc<dyn ConfigurationAdapter>;
        let engine = Arc::new(Engine::new());
        let worker = ConfigurationWorker::for_test(engine.clone(), adapter, 0);
        (engine, worker, dir)
    }

    fn schema_object_required_port() -> Value {
        json!({
            "type": "object",
            "required": ["port"],
            "properties": { "port": { "type": "integer" } },
        })
    }

    fn register_input(id: &str, initial: Option<Value>) -> ConfigurationRegisterInput {
        ConfigurationRegisterInput {
            id: id.into(),
            name: format!("{} display", id),
            description: "test".into(),
            schema: schema_object_required_port(),
            initial_value: initial,
            metadata: None,
        }
    }

    #[tokio::test]
    async fn register_creates_entry_and_returns_it() {
        let (_engine, worker, _dir) = setup().await;
        let result = worker
            .register_fn(register_input("iii-stream", Some(json!({ "port": 3112 }))))
            .await;
        match result {
            FunctionResult::Success(entry) => {
                assert_eq!(entry.id, "iii-stream");
                assert_eq!(entry.value, json!({ "port": 3112 }));
            }
            _ => panic!("expected register success"),
        }
    }

    #[tokio::test]
    async fn register_without_initial_value_seeds_null() {
        let (_engine, worker, _dir) = setup().await;
        let result = worker.register_fn(register_input("iii-stream", None)).await;
        match result {
            FunctionResult::Success(entry) => assert!(entry.value.is_null()),
            _ => panic!("expected register success without initial_value"),
        }
    }

    #[tokio::test]
    async fn register_rejects_initial_value_violating_schema() {
        let (_engine, worker, _dir) = setup().await;
        let result = worker
            .register_fn(register_input(
                "iii-stream",
                Some(json!({ "port": "not-an-integer" })),
            ))
            .await;
        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "SCHEMA_INVALID"),
            _ => panic!("expected schema validation failure"),
        }
    }

    #[tokio::test]
    async fn set_validates_against_registered_schema() {
        let (_engine, worker, _dir) = setup().await;
        worker
            .register_fn(register_input("iii-stream", Some(json!({ "port": 3112 }))))
            .await;

        let bad = worker
            .set_fn(ConfigurationSetInput {
                id: "iii-stream".into(),
                value: json!({ "port": "wrong" }),
            })
            .await;
        match bad {
            FunctionResult::Failure(err) => assert_eq!(err.code, "SCHEMA_INVALID"),
            _ => panic!("expected schema validation failure on set"),
        }

        let good = worker
            .set_fn(ConfigurationSetInput {
                id: "iii-stream".into(),
                value: json!({ "port": 4242 }),
            })
            .await;
        match good {
            FunctionResult::Success(s) => {
                assert_eq!(s.old_value, Some(json!({ "port": 3112 })));
                assert_eq!(s.new_value, json!({ "port": 4242 }));
            }
            _ => panic!("expected set success"),
        }
    }

    #[tokio::test]
    async fn set_on_unregistered_id_returns_not_registered() {
        let (_engine, worker, _dir) = setup().await;
        let result = worker
            .set_fn(ConfigurationSetInput {
                id: "missing".into(),
                value: json!({}),
            })
            .await;
        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "NOT_REGISTERED"),
            _ => panic!("expected NOT_REGISTERED"),
        }
    }

    #[tokio::test]
    async fn set_without_available_schema_returns_schema_unavailable() {
        let (_engine, worker, _dir) = setup().await;
        // Register with a null schema (the shape a disk-loaded entry has before
        // its worker re-registers); no initial_value is validated.
        worker
            .register_fn(ConfigurationRegisterInput {
                id: "iii-stream".into(),
                name: "iii-stream display".into(),
                description: "test".into(),
                schema: Value::Null,
                initial_value: None,
                metadata: None,
            })
            .await;
        let result = worker
            .set_fn(ConfigurationSetInput {
                id: "iii-stream".into(),
                value: json!({ "port": 4242 }),
            })
            .await;
        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "SCHEMA_UNAVAILABLE"),
            _ => panic!("expected SCHEMA_UNAVAILABLE"),
        }
    }

    #[tokio::test]
    async fn get_expands_env_var_placeholders_by_default() {
        let (_engine, worker, _dir) = setup().await;
        unsafe {
            std::env::set_var("CFG_GET_HOST", "db.local");
        }

        // Schema accepts strings under host so the placeholder set passes validation.
        let mut input = register_input("svc", None);
        input.schema = json!({
            "type": "object",
            "properties": { "host": { "type": "string" } },
        });
        input.initial_value = Some(json!({ "host": "${CFG_GET_HOST:fallback}" }));
        worker.register_fn(input).await;

        let expanded = worker
            .get_fn(ConfigurationGetInput {
                id: "svc".into(),
                raw: false,
            })
            .await;
        match expanded {
            FunctionResult::Success(out) => assert_eq!(out.value["host"], "db.local"),
            _ => panic!("expected get success"),
        }

        let raw = worker
            .get_fn(ConfigurationGetInput {
                id: "svc".into(),
                raw: true,
            })
            .await;
        match raw {
            FunctionResult::Success(out) => {
                assert_eq!(out.value["host"], "${CFG_GET_HOST:fallback}")
            }
            _ => panic!("expected raw get success"),
        }
    }

    // #1916: a templated integer field must register (validation runs on the
    // APPLIED value) and `get` must coerce it to a real integer.
    #[tokio::test]
    async fn register_and_get_coerces_templated_integer_port() {
        let (_engine, worker, _dir) = setup().await;
        unsafe {
            std::env::remove_var("CFG_GET_TPORT");
        }
        let result = worker
            .register_fn(register_input(
                "iii-stream",
                Some(json!({ "port": "${CFG_GET_TPORT:3111}" })),
            ))
            .await;
        assert!(
            matches!(result, FunctionResult::Success(_)),
            "a templated integer port must pass register (validated against the coerced value)"
        );

        let got = worker
            .get_fn(ConfigurationGetInput {
                id: "iii-stream".into(),
                raw: false,
            })
            .await;
        match got {
            FunctionResult::Success(out) => {
                assert_eq!(out.value["port"], json!(3111));
                assert!(out.value["port"].is_i64(), "must be coerced to an integer");
            }
            _ => panic!("expected get success with a coerced integer"),
        }

        // The raw template is preserved on disk / for re-evaluation.
        let raw = worker
            .get_fn(ConfigurationGetInput {
                id: "iii-stream".into(),
                raw: true,
            })
            .await;
        match raw {
            FunctionResult::Success(out) => {
                assert_eq!(out.value["port"], json!("${CFG_GET_TPORT:3111}"))
            }
            _ => panic!("expected raw get success"),
        }
    }

    // #1916: when the applied value violates the schema, `get` must log + fail
    // with SCHEMA_INVALID ("won't be loaded"), not hand back a broken value.
    // Disk-loaded entries skip register validation, then the worker re-registers
    // the schema — replicated here by injecting the entry directly.
    #[tokio::test]
    async fn get_fails_schema_invalid_when_applied_value_violates_schema() {
        let (_engine, worker, _dir) = setup().await;
        unsafe {
            std::env::remove_var("CFG_GET_BADPORT");
        }
        worker
            .store
            .apply_external(&ExternalChange::Registered(ConfigurationEntry {
                id: "iii-stream".into(),
                name: "iii-stream".into(),
                description: String::new(),
                schema: schema_object_required_port(),
                value: json!({ "port": "${CFG_GET_BADPORT:notaport}" }),
                metadata: None,
            }))
            .await;

        let got = worker
            .get_fn(ConfigurationGetInput {
                id: "iii-stream".into(),
                raw: false,
            })
            .await;
        match got {
            FunctionResult::Failure(err) => assert_eq!(err.code, "SCHEMA_INVALID"),
            _ => panic!("expected SCHEMA_INVALID failure"),
        }
    }

    // #1916: a `${VAR}` with no env value and no default can't be evaluated at
    // read time — `get` must fail with EXPAND_FAILED (and not panic).
    #[tokio::test]
    async fn get_fails_expand_failed_when_required_var_missing() {
        let (_engine, worker, _dir) = setup().await;
        unsafe {
            std::env::remove_var("CFG_GET_REQUIRED");
        }
        worker
            .store
            .apply_external(&ExternalChange::Registered(ConfigurationEntry {
                id: "iii-stream".into(),
                name: "iii-stream".into(),
                description: String::new(),
                schema: schema_object_required_port(),
                value: json!({ "port": "${CFG_GET_REQUIRED}" }),
                metadata: None,
            }))
            .await;

        let got = worker
            .get_fn(ConfigurationGetInput {
                id: "iii-stream".into(),
                raw: false,
            })
            .await;
        match got {
            FunctionResult::Failure(err) => assert_eq!(err.code, "EXPAND_FAILED"),
            _ => panic!("expected EXPAND_FAILED failure"),
        }
    }

    // --- hot-reload: external file-edit validation ---

    fn updated_change(value: Value, old: Value) -> ExternalChange {
        ExternalChange::Updated {
            entry: ConfigurationEntry {
                id: "iii-stream".into(),
                name: "iii-stream".into(),
                description: String::new(),
                schema: schema_object_required_port(),
                value,
                metadata: None,
            },
            old_value: Some(old),
        }
    }

    #[tokio::test]
    async fn external_change_applies_valid_edit() {
        let (_engine, worker, _dir) = setup().await;
        worker
            .register_fn(register_input("iii-stream", Some(json!({ "port": 1 }))))
            .await;

        worker
            .handle_external_change(updated_change(json!({ "port": 2 }), json!({ "port": 1 })))
            .await;

        let entry = worker.store.get("iii-stream").await.expect("entry");
        assert_eq!(
            entry.value,
            json!({ "port": 2 }),
            "a valid file edit must hot-reload into the store"
        );
    }

    #[tokio::test]
    async fn external_change_rejects_invalid_edit_and_keeps_previous() {
        let (_engine, worker, _dir) = setup().await;
        worker
            .register_fn(register_input("iii-stream", Some(json!({ "port": 1 }))))
            .await;

        worker
            .handle_external_change(updated_change(
                json!({ "port": "not-an-integer" }),
                json!({ "port": 1 }),
            ))
            .await;

        let entry = worker.store.get("iii-stream").await.expect("entry");
        assert_eq!(
            entry.value,
            json!({ "port": 1 }),
            "an invalid file edit must be dropped (warned), keeping the previous value"
        );
    }

    #[tokio::test]
    async fn external_change_rejects_unresolved_env_var() {
        let (_engine, worker, _dir) = setup().await;
        unsafe {
            std::env::remove_var("CFG_EXT_MISSING");
        }
        worker
            .register_fn(register_input("iii-stream", Some(json!({ "port": 1 }))))
            .await;

        worker
            .handle_external_change(updated_change(
                json!({ "port": "${CFG_EXT_MISSING}" }),
                json!({ "port": 1 }),
            ))
            .await;

        let entry = worker.store.get("iii-stream").await.expect("entry");
        assert_eq!(
            entry.value,
            json!({ "port": 1 }),
            "an edit referencing an unresolved env var must be dropped"
        );
    }

    #[tokio::test]
    async fn external_change_applies_when_schema_unknown() {
        let (_engine, worker, _dir) = setup().await;
        // A brand-new file the owning worker hasn't claimed yet: schema is null,
        // so it can't be validated here — apply it and let `get` validate later
        // once the owner registers a real schema.
        worker
            .handle_external_change(ExternalChange::Registered(ConfigurationEntry {
                id: "fresh".into(),
                name: "fresh".into(),
                description: String::new(),
                schema: Value::Null,
                value: json!({ "port": "anything" }),
                metadata: None,
            }))
            .await;

        let entry = worker.store.get("fresh").await.expect("entry applied");
        assert_eq!(entry.value, json!({ "port": "anything" }));
    }

    // End-to-end hot-reload: register → initialize (starts the watcher) → edit
    // the file on disk → the store reflects the new value. Covers the full
    // `initialize → watcher → handle_external_change` wiring the per-handler
    // tests above don't exercise (the gap behind the "edit did nothing" report).
    #[tokio::test]
    async fn external_file_edit_hot_reloads_through_the_watcher() {
        let (_engine, worker, dir) = setup().await;
        worker
            .register_fn(register_input("iii-stream", Some(json!({ "port": 1 }))))
            .await;
        // initialize() primes the cache, registers the trigger type, and starts
        // the directory watcher — exactly as a real boot does.
        worker
            .initialize()
            .await
            .expect("initialize starts the watcher");

        // Edit the file the way an operator would, in the value-only on-disk
        // format `write_entry` produces (no schema key).
        let path = dir.path().join("iii-stream.yaml");
        tokio::fs::write(
            &path,
            "id: iii-stream\nname: iii-stream display\ndescription: test\nvalue:\n  port: 3114\n",
        )
        .await
        .expect("write the external edit");

        // The watcher debounces ~500ms; poll the store until it reflects the edit.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(entry) = worker.store.get("iii-stream").await
                && entry.value == json!({ "port": 3114 })
            {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                panic!("hot-reload did not apply the external file edit within 5s");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        worker.destroy().await.expect("destroy stops the watcher");
    }

    #[tokio::test]
    async fn get_unknown_id_returns_not_found() {
        let (_engine, worker, _dir) = setup().await;
        let result = worker
            .get_fn(ConfigurationGetInput {
                id: "missing".into(),
                raw: false,
            })
            .await;
        match result {
            FunctionResult::Failure(err) => assert_eq!(err.code, "NOT_FOUND"),
            _ => panic!("expected NOT_FOUND"),
        }
    }

    #[tokio::test]
    async fn list_returns_registered_entries_sorted_without_value() {
        let (_engine, worker, _dir) = setup().await;
        worker
            .register_fn(register_input("zebra", Some(json!({ "port": 1 }))))
            .await;
        worker
            .register_fn(register_input("alpha", Some(json!({ "port": 2 }))))
            .await;

        let result = worker.list_fn(ConfigurationListInput {}).await;
        match result {
            FunctionResult::Success(out) => {
                let ids: Vec<String> = out.configurations.iter().map(|c| c.id.clone()).collect();
                assert_eq!(ids, vec!["alpha".to_string(), "zebra".to_string()]);
                let serialised = serde_json::to_value(&out.configurations[0]).unwrap();
                assert!(
                    serialised.get("value").is_none(),
                    "list must not leak the stored value"
                );
            }
            _ => panic!("expected list success"),
        }
    }

    #[tokio::test]
    async fn schema_returns_registered_schema() {
        let (_engine, worker, _dir) = setup().await;
        worker
            .register_fn(register_input("iii-stream", Some(json!({ "port": 3112 }))))
            .await;

        let result = worker
            .schema_fn(ConfigurationSchemaInput {
                id: "iii-stream".into(),
            })
            .await;
        match result {
            FunctionResult::Success(view) => {
                assert_eq!(view.id, "iii-stream");
                assert_eq!(view.schema, schema_object_required_port());
            }
            _ => panic!("expected schema success"),
        }

        let missing = worker
            .schema_fn(ConfigurationSchemaInput {
                id: "missing".into(),
            })
            .await;
        match missing {
            FunctionResult::Failure(err) => assert_eq!(err.code, "NOT_FOUND"),
            _ => panic!("expected NOT_FOUND"),
        }
    }

    #[tokio::test]
    async fn re_register_replaces_metadata_keeps_value_when_initial_omitted() {
        let (_engine, worker, _dir) = setup().await;
        worker
            .register_fn(register_input("iii-stream", Some(json!({ "port": 3112 }))))
            .await;

        let mut update = register_input("iii-stream", None);
        update.description = "updated".into();
        worker.register_fn(update).await;

        let entry = worker.store.get("iii-stream").await.expect("entry");
        assert_eq!(entry.description, "updated");
        assert_eq!(entry.value, json!({ "port": 3112 }));
    }
}
