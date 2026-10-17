// Main JSON outbound envelope: 16 MiB (16,777,216 bytes).
pub(crate) const MAX_JSON_FRAME_BYTES: usize = 16 * 1024 * 1024;
// Main JSON inbound frame/message: 64 MiB (67,108,864 bytes).
const MAX_JSON_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

#[cfg(test)]
#[allow(deprecated)]
mod json_size_tests {
    use super::*;

    #[tokio::test]
    async fn local_rejection_preserves_result_fallback_and_following_messages() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let first = ws.next().await.unwrap().unwrap();
            let second = ws.next().await.unwrap().unwrap();
            (first, second)
        });
        let (stream, _) = connect_async(format!("ws://{address}")).await.unwrap();
        let (mut tx, _) = stream.split();
        let sdk = IIIClient::new("ws://127.0.0.1:0");
        let rejected =
            IIIClient::prepare_envelope(&invocation("x".repeat(MAX_JSON_FRAME_BYTES))).unwrap_err();
        assert_eq!(
            rejected.invocation_error().unwrap().code,
            "payload_too_large"
        );
        let mut queue = vec![
            IIIClient::prepare_envelope(&Message::InvocationResult {
                invocation_id: Uuid::nil(),
                function_id: "f".into(),
                result: Some(Value::String("x".repeat(MAX_JSON_FRAME_BYTES))),
                error: None,
                traceparent: None,
                baggage: None,
            })
            .unwrap(),
            IIIClient::prepare_envelope(&Message::Pong).unwrap(),
        ];
        sdk.flush_queue(&mut tx, &mut queue).await.unwrap();
        assert!(queue.is_empty());
        let (first, second) = server.await.unwrap();
        let parsed: Value = serde_json::from_str(first.to_text().unwrap()).unwrap();
        assert_eq!(parsed["error"]["code"], "payload_too_large");
        assert_eq!(second.to_text().unwrap(), "{\"type\":\"pong\"}");
    }

    #[tokio::test]
    async fn local_argument_rejection_clears_pending_before_connection() {
        let sdk = IIIClient::new("ws://127.0.0.1:0");
        let error = sdk
            .trigger(crate::protocol::TriggerRequest {
                function_id: "size::echo".into(),
                payload: Value::String("x".repeat(MAX_JSON_FRAME_BYTES)),
                action: None,
                timeout_ms: Some(100),
            })
            .await
            .unwrap_err();
        assert_eq!(error.invocation_error().unwrap().code, "payload_too_large");
        assert!(sdk.inner.pending.lock_or_recover().is_empty());
    }

    struct TestTriggerHandler;

    #[async_trait::async_trait]
    impl TriggerHandler for TestTriggerHandler {
        async fn register_trigger(&self, _: TriggerConfig) -> Result<(), Error> {
            Ok(())
        }

        async fn unregister_trigger(&self, _: TriggerConfig) -> Result<(), Error> {
            Ok(())
        }
    }

    fn register_test_type(sdk: &IIIClient, description: String) {
        sdk.register_trigger_type(RegisterTriggerType::new(
            "size::custom",
            description,
            TestTriggerHandler,
        ));
    }

    #[test]
    fn rejected_replacement_preserves_accepted_trigger_type_ownership() {
        let sdk = IIIClient::new("ws://127.0.0.1:0");
        sdk.inner.running.store(true, Ordering::SeqCst);
        register_test_type(&sdk, "accepted".into());
        let accepted_handler = sdk.inner.trigger_types.lock_or_recover()["size::custom"]
            .handler
            .clone();
        let mut receiver = sdk.inner.receiver.lock_or_recover().take().unwrap();
        assert!(matches!(receiver.try_recv(), Ok(Outbound::Message(_))));

        register_test_type(&sdk, "x".repeat(MAX_JSON_FRAME_BYTES));

        let types = sdk.inner.trigger_types.lock_or_recover();
        let accepted = &types["size::custom"];
        assert_eq!(accepted.message.description, "accepted");
        assert!(Arc::ptr_eq(&accepted.handler, &accepted_handler));
        assert!(receiver.try_recv().is_err());
        drop(types);
        register_test_type(&sdk, "following".into());
        assert_eq!(
            sdk.inner.trigger_types.lock_or_recover()["size::custom"]
                .message
                .description,
            "following"
        );
        assert!(matches!(receiver.try_recv(), Ok(Outbound::Message(_))));
    }

    #[test]
    fn rejected_function_handle_cannot_unregister_an_accepted_owner() {
        let sdk = IIIClient::new("ws://127.0.0.1:0");
        sdk.inner.running.store(true, Ordering::SeqCst);
        let handler: RemoteFunctionHandlerWithMetadata =
            Arc::new(|_, _| Box::pin(async { Ok(Value::Null) }));
        sdk.register_function_inner(
            RegisterFunctionMessage::with_id("size::handler".into()),
            Some(handler.clone()),
        );
        let mut receiver = sdk.inner.receiver.lock_or_recover().take().unwrap();
        assert!(matches!(receiver.try_recv(), Ok(Outbound::Message(_))));
        let rejected = sdk.register_function_inner(
            RegisterFunctionMessage::with_id("size::handler".into())
                .with_description("x".repeat(MAX_JSON_FRAME_BYTES)),
            None,
        );
        rejected.unregister();
        let functions = sdk.inner.functions.lock_or_recover();
        let accepted = &functions["size::handler"];
        assert!(Arc::ptr_eq(accepted.handler.as_ref().unwrap(), &handler));
        assert!(accepted.message.description.is_none());
        assert!(receiver.try_recv().is_err());
        drop(functions);
        sdk.register_function_inner(
            RegisterFunctionMessage::with_id("size::following".into()),
            None,
        );
        assert!(matches!(receiver.try_recv(), Ok(Outbound::Message(_))));
    }

    #[test]
    fn oversized_registrations_are_neither_retained_nor_enqueued() {
        let sdk = IIIClient::new("ws://127.0.0.1:0");
        sdk.inner.running.store(true, Ordering::SeqCst);
        let rejected = sdk.register_function_inner(
            RegisterFunctionMessage::with_id("size::rejected".into())
                .with_description("x".repeat(MAX_JSON_FRAME_BYTES)),
            None,
        );
        rejected.unregister();
        register_test_type(&sdk, "x".repeat(MAX_JSON_FRAME_BYTES));
        let error = sdk
            .register_trigger(RegisterTriggerInput::new(
                "size::custom",
                "size::handler",
                Value::String("x".repeat(MAX_JSON_FRAME_BYTES)),
            ))
            .err()
            .expect("oversized trigger must fail");
        assert_eq!(error.invocation_error().unwrap().code, "payload_too_large");
        assert!(sdk.inner.functions.lock_or_recover().is_empty());
        assert!(sdk.inner.trigger_types.lock_or_recover().is_empty());
        assert!(sdk.inner.triggers.lock_or_recover().is_empty());
        assert!(
            sdk.inner
                .receiver
                .lock_or_recover()
                .as_mut()
                .unwrap()
                .try_recv()
                .is_err()
        );
    }

    #[test]
    fn rejected_trigger_type_preflight_does_not_acquire_ownership_mutex() {
        let sdk = IIIClient::new("ws://127.0.0.1:0");
        sdk.inner.running.store(true, Ordering::SeqCst);
        register_test_type(&sdk, "accepted".into());
        let rejected_sdk = sdk.clone();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let owned = sdk.inner.trigger_types.lock_or_recover();
        let thread = std::thread::spawn(move || {
            register_test_type(&rejected_sdk, "x".repeat(MAX_JSON_FRAME_BYTES));
            finished_tx.send(()).unwrap();
        });
        // Holding the mutex makes the old insertion-before-validation path
        // deterministically unable to finish; this is a timeout, not a sleep.
        let completed_without_ownership = finished_rx.recv_timeout(Duration::from_secs(10)).is_ok();
        assert_eq!(owned["size::custom"].message.description, "accepted");
        drop(owned);
        thread.join().unwrap();
        assert!(
            completed_without_ownership,
            "rejected registration touched ownership"
        );
    }

    #[test]
    fn delayed_send_rejection_cannot_remove_a_newer_accepted_generation() {
        let sdk = IIIClient::new("ws://127.0.0.1:0");
        sdk.inner.running.store(true, Ordering::SeqCst);
        register_test_type(&sdk, "previous".into());
        let rejected_message = sdk.inner.trigger_types.lock_or_recover()["size::custom"]
            .message
            .clone();
        let mut rejected_message = rejected_message.to_message();
        if let Message::RegisterTriggerType { description, .. } = &mut rejected_message {
            *description = "x".repeat(MAX_JSON_FRAME_BYTES);
        }
        let rejected_sdk = sdk.clone();
        let (proceed_tx, proceed_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            proceed_rx.recv().unwrap();
            rejected_sdk.send_message(rejected_message).unwrap_err()
        });
        // Order the same shared-helper rejection after a newer accepted owner,
        // exactly the harmful rollback interleaving, without timing guesses.
        register_test_type(&sdk, "newer".into());
        let newer_handler = sdk.inner.trigger_types.lock_or_recover()["size::custom"]
            .handler
            .clone();
        proceed_tx.send(()).unwrap();
        let error = thread.join().unwrap();
        assert_eq!(error.invocation_error().unwrap().code, "payload_too_large");
        let types = sdk.inner.trigger_types.lock_or_recover();
        assert_eq!(types["size::custom"].message.description, "newer");
        assert!(Arc::ptr_eq(&types["size::custom"].handler, &newer_handler));
        drop(types);
        let mut receiver = sdk.inner.receiver.lock_or_recover().take().unwrap();
        assert!(matches!(receiver.try_recv(), Ok(Outbound::Message(_))));
        assert!(matches!(receiver.try_recv(), Ok(Outbound::Message(_))));
        assert!(receiver.try_recv().is_err());
        sdk.send_message(Message::Pong).unwrap();
        match receiver.try_recv().unwrap() {
            Outbound::Message(envelope) => {
                assert_eq!(envelope.payload.as_str(), "{\"type\":\"pong\"}");
            }
            Outbound::Shutdown => panic!("expected pong"),
        }
    }

    fn invocation(data: String) -> Message {
        Message::InvokeFunction {
            invocation_id: Some(Uuid::nil()),
            function_id: "size::echo".into(),
            data: Value::String(data),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        }
    }

    #[test]
    fn complete_utf8_envelope_has_inclusive_boundary() {
        let overhead = serde_json::to_string(&invocation(String::new()))
            .unwrap()
            .len();
        for size in [
            MAX_JSON_FRAME_BYTES - 1,
            MAX_JSON_FRAME_BYTES,
            MAX_JSON_FRAME_BYTES + 1,
        ] {
            let message = invocation("x".repeat(size - overhead));
            let result = IIIClient::prepare_envelope(&message);
            if size <= MAX_JSON_FRAME_BYTES {
                assert_eq!(result.unwrap().payload.len(), size);
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.invocation_error().unwrap().code, "payload_too_large");
            }
        }
    }

    #[test]
    fn unicode_and_escapes_use_serialized_bytes() {
        let input = "😀é\n\"\\".repeat(1_700_000);
        assert!(input.chars().count() < MAX_JSON_FRAME_BYTES);
        assert!(IIIClient::prepare_envelope(&invocation(input)).is_err());
    }

    #[test]
    fn result_fallback_preserves_correlation_and_discards_large_context() {
        let message = Message::InvocationResult {
            invocation_id: Uuid::nil(),
            function_id: "size::echo".into(),
            result: Some(Value::String("x".repeat(MAX_JSON_FRAME_BYTES))),
            error: None,
            traceparent: None,
            baggage: Some("secret".repeat(1000)),
        };
        let payload = IIIClient::prepare_envelope(&message).unwrap().payload;
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["invocation_id"], Uuid::nil().to_string());
        assert_eq!(parsed["error"]["code"], "payload_too_large");
        assert!(parsed.get("result").is_none());
        assert!(!payload.contains("secret"));
    }

    #[test]
    fn explicit_receive_capacity_is_bounded() {
        let config = IIIClient::receive_config();
        assert_eq!(config.max_frame_size, Some(67_108_864));
        assert_eq!(config.max_message_size, Some(67_108_864));
    }
}

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

/// Extension trait for Mutex that recovers from poisoning instead of panicking.
/// This is safe when the protected data is still valid after a panic in another thread.
pub(crate) trait MutexExt<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    sync::{Notify, mpsc, oneshot},
    time::{Instant, interval, sleep, sleep_until},
};
#[cfg(test)]
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::{Message as WsMessage, Utf8Bytes, protocol::WebSocketConfig};
use uuid::Uuid;

const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

use iii_helpers::http::HttpInvocationConfig;

use arc_swap::ArcSwap;

use crate::{
    channels::{
        ChannelReader, ChannelWriter, StreamChannelRef, handshake_headers, handshake_request,
    },
    context::{InvocationCtx, with_current},
    dispatch::{error_body, execute, run_reporter},
    error::Error,
    observable::{FunctionEntry, HookOp, Observe, Observer},
    observe::{ObserverEntry, ObserverReason},
    protocol::{
        Activation, Caller, Dispatch, ErrorBody, FEATURE_OBSERVE_V1, FUNCTION_NAMESPACE_CONFLICT,
        Message, ObserverState, ParentCaller, RegisterFunctionMessage, RegisterTriggerInput,
        RegisterTriggerMessage, RegisterTriggerTypeMessage, SDK_FEATURES, TriggerAction,
        TriggerRequest, TriggerRequestWithMetadata, UnregisterTriggerMessage,
        UnregisterTriggerTypeMessage, WORKER_NAMESPACE_CONFLICT, observe_codes,
    },
    reporter::Reporter,
    transport::{self, WsConnectResult, WsStream},
    triggers::{Trigger, TriggerConfig, TriggerHandler},
    types::{
        Channel, RemoteFunctionData, RemoteFunctionHandlerWithMetadata, RemoteTriggerTypeData,
    },
};

use iii_helpers::observability as telemetry;
use iii_helpers::observability::OtelConfig;

pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Where the deprecated trigger APIs point to.
const MIGRATION_GUIDE: &str = "https://iii.dev/docs/upgrading/from-0-24-x";

/// Logs `message` the first time `warned` is seen unset: one warning per
/// process per deprecated API.
fn warn_deprecated_once(warned: &AtomicBool, message: &str) {
    if !warned.swap(true, Ordering::Relaxed) {
        tracing::warn!("{message} Migration guide: {MIGRATION_GUIDE}");
    }
}

static WARNED_REGISTER_TRIGGER: AtomicBool = AtomicBool::new(false);
static WARNED_REGISTER_TRIGGER_TYPE: AtomicBool = AtomicBool::new(false);

fn warn_register_trigger() {
    warn_deprecated_once(
        &WARNED_REGISTER_TRIGGER,
        "register_trigger is deprecated and will be removed in a future release. Use \
         iii.observe(Source::function(..).args(..), target) instead.",
    );
}

fn warn_register_trigger_type() {
    warn_deprecated_once(
        &WARNED_REGISTER_TRIGGER_TYPE,
        "register_trigger_type is deprecated and will be removed in a future release. Register \
         a function with an observe descriptor (RegisterFunction::observe(Observe::owner()..)) \
         instead.",
    );
}

/// Wall-clock milliseconds since the Unix epoch.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `caller` as the `parent` of a call made from the handler it invoked; `None`
/// when it names nothing.
pub(crate) fn parent_of(caller: &Caller) -> Option<ParentCaller> {
    let parent = caller.strip();
    (parent != ParentCaller::default()).then_some(parent)
}

/// An invocation sent through the engine.
pub(crate) struct RemoteCall {
    pub function_id: String,
    pub payload: Value,
    pub action: Option<TriggerAction>,
    pub timeout: Option<Duration>,
    pub metadata: Option<Value>,
    pub namespace: Option<String>,
    pub caller: Option<Box<Caller>>,
    pub observe_depth: Option<u32>,
    pub activation: Option<Activation>,
}

type ObserveSupportFn = Arc<dyn Fn(bool) + Send + Sync>;

/// A listener of [`IIIClient::on_engine_supports_observe`].
struct SupportListener {
    id: u64,
    /// Cleared by `unsubscribe`: a call already queued does not start.
    active: Arc<AtomicBool>,
    /// The value it was last told (or found when added).
    heard: Option<bool>,
    call: ObserveSupportFn,
}

/// Listeners of the engine's observe support.
#[derive(Default)]
struct ObserveSupport {
    next_id: u64,
    listeners: Vec<SupportListener>,
}

impl ObserveSupport {
    /// Tells each listener `observe`; enqueued under the lock, so listeners
    /// hear changes in order.
    fn notify(listeners: Vec<(Arc<AtomicBool>, ObserveSupportFn)>, observe: bool) {
        crate::observe::deliver(Box::new(move || {
            for (active, listener) in listeners {
                if !active.load(Ordering::Acquire) {
                    continue;
                }
                // one listener's panic does not cost the others the change
                let call = std::panic::AssertUnwindSafe(|| listener(observe));
                if let Err(panic) = std::panic::catch_unwind(call) {
                    tracing::warn!(
                        "engine observe support listener panicked: {}",
                        crate::observable::panic_message(&panic)
                    );
                }
            }
        }));
    }
}

/// A listener added with [`IIIClient::on_engine_supports_observe`].
#[derive(Debug)]
pub struct ObserveSupportListener {
    support: std::sync::Weak<Mutex<ObserveSupport>>,
    id: u64,
    active: Arc<AtomicBool>,
}

impl ObserveSupportListener {
    /// Stops calling the listener: once this returns no call starts, queued
    /// ones included (a call already running finishes).
    pub fn unsubscribe(self) {
        self.active.store(false, Ordering::Release);
        if let Some(support) = self.support.upgrade() {
            support
                .lock_or_recover()
                .listeners
                .retain(|listener| listener.id != self.id);
        }
    }
}

/// What the engine said it supports, on the current connection.
const ENGINE_FEATURES_UNKNOWN: u8 = 0;
const ENGINE_WITHOUT_OBSERVE: u8 = 1;
const ENGINE_WITH_OBSERVE: u8 = 2;

/// Observe messages (acks, unobserves) held at most across a reconnect, by
/// count and by bytes; the rest are dropped. Reports go back in the report
/// buffer instead, under its own bound.
const MAX_HELD_FRAMES: usize = 4096;
const MAX_HELD_BYTES: usize = 4 * 1024 * 1024;

/// Registration key prefix of `observe` frames.
const OBSERVER_KEY_PREFIX: &str = "observer:";

/// Worker information returned by `engine::workers::list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub id: String,
    pub name: Option<String>,
    pub runtime: Option<String>,
    pub version: Option<String>,
    pub os: Option<String>,
    pub ip_address: Option<String>,
    pub status: String,
    pub connected_at_ms: u64,
    pub function_count: usize,
    pub functions: Vec<String>,
    pub active_invocations: usize,
    #[serde(default)]
    pub isolation: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Function information returned by `engine::functions::list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub function_id: String,
    pub description: Option<String>,
    pub request_format: Option<Value>,
    pub response_format: Option<Value>,
    pub metadata: Option<Value>,
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Trigger information returned by `engine::triggers::list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub id: String,
    pub trigger_type: String,
    pub function_id: String,
    pub config: Value,
    pub metadata: Option<Value>,
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Builder for registering a custom trigger type with optional format schemas.
///
/// **Deprecated since 0.25.0** together with
/// [`IIIClient::register_trigger_type`]: register a function with an observe
/// descriptor ([`RegisterFunction::observe`]) instead. Migration guide:
/// <https://iii.dev/docs/upgrading/from-0-24-x>.
///
/// Type parameters:
/// - `C` tracks the trigger registration type (set via `.trigger_request_format::<T>()`)
/// - `R` tracks the call request type (set via `.call_request_format::<T>()`)
///
/// Both default to `Value` (untyped) and change when the respective builder
/// method is called. This allows [`IIIClient::register_trigger_type`] to return a
/// [`TriggerTypeRef<C, R>`] with compile-time safety for both config and
/// function input types.
pub struct RegisterTriggerType<H, C = Value, R = Value> {
    id: String,
    description: String,
    handler: H,
    trigger_request_format: Option<Value>,
    call_request_format: Option<Value>,
    _phantom: std::marker::PhantomData<(C, R)>,
}

impl<H: TriggerHandler> RegisterTriggerType<H> {
    pub fn new(id: impl Into<String>, description: impl Into<String>, handler: H) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            handler,
            trigger_request_format: None,
            call_request_format: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<H: TriggerHandler, C, R> RegisterTriggerType<H, C, R> {
    /// Set the trigger request format schema from a type.
    /// Changes `C`, enabling compile-time validation on
    /// [`TriggerTypeRef::register_trigger`].
    pub fn trigger_request_format<T: schemars::JsonSchema + Serialize>(
        self,
    ) -> RegisterTriggerType<H, T, R> {
        RegisterTriggerType {
            id: self.id,
            description: self.description,
            handler: self.handler,
            trigger_request_format: json_schema_for::<T>(),
            call_request_format: self.call_request_format,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set the call request format schema from a type.
    /// Changes `R`, enabling compile-time validation on
    /// [`TriggerTypeRef::register_function`].
    pub fn call_request_format<T: schemars::JsonSchema>(self) -> RegisterTriggerType<H, C, T> {
        RegisterTriggerType {
            id: self.id,
            description: self.description,
            handler: self.handler,
            trigger_request_format: self.trigger_request_format,
            call_request_format: json_schema_for::<T>(),
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Typed handle returned by [`IIIClient::register_trigger_type`].
///
/// **Deprecated since 0.25.0** with it; see
/// <https://iii.dev/docs/upgrading/from-0-24-x>.
///
/// Type parameters:
/// - `C`: trigger registration type for [`register_trigger`](Self::register_trigger)
/// - `R`: call request type for [`register_function`](Self::register_function)
#[derive(Clone)]
pub struct TriggerTypeRef<C = Value, R = Value> {
    iii: IIIClient,
    trigger_type_id: String,
    _phantom: std::marker::PhantomData<(C, R)>,
}

impl<C: Serialize, R> TriggerTypeRef<C, R> {
    /// Register a trigger with compile-time validated trigger config.
    #[deprecated(
        since = "0.25.0",
        note = "use iii.observe(Source::function(..).args(..), target) instead; see https://iii.dev/docs/upgrading/from-0-24-x"
    )]
    pub fn register_trigger(
        &self,
        function_id: impl Into<String>,
        config: C,
    ) -> Result<Trigger, Error> {
        #[allow(deprecated)]
        self.register_trigger_with_metadata(function_id, config, None)
    }

    /// Register a trigger with compile-time validated trigger config and optional metadata.
    ///
    /// This typed helper pairs a function with its trigger, so it defaults the
    /// trigger's namespace to this worker's — otherwise the function lands in the
    /// worker's namespace and the trigger in `default`, never resolving it. The
    /// low-level [`IIIClient::register_trigger`] keeps the engine default.
    #[deprecated(
        since = "0.25.0",
        note = "use iii.observe_with(Source::function(..).args(..), target, ObserverOptions::default().metadata(..)) instead; see https://iii.dev/docs/upgrading/from-0-24-x"
    )]
    pub fn register_trigger_with_metadata(
        &self,
        function_id: impl Into<String>,
        config: C,
        metadata: Option<Value>,
    ) -> Result<Trigger, Error> {
        let mut input = RegisterTriggerInput::new(
            self.trigger_type_id.clone(),
            function_id,
            serde_json::to_value(config).map_err(|e| Error::Handler(e.to_string()))?,
        );
        // This typed helper pairs a function with its trigger, so it names the
        // worker's namespace for the target; the low-level path resolves the
        // same thing, and saying it here keeps the two from drifting.
        input.metadata = metadata;
        input.namespace = self.iii.namespace();
        #[allow(deprecated)]
        self.iii.register_trigger(input)
    }
}

impl<C, R> TriggerTypeRef<C, R>
where
    R: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
{
    /// Register a sync function whose input type must match
    /// the call request format `R`.
    #[deprecated(
        since = "0.25.0",
        note = "use iii.register_function(id, RegisterFunction::new(f)) instead; see https://iii.dev/docs/upgrading/from-0-24-x"
    )]
    pub fn register_function<O, F>(&self, id: impl Into<String>, f: F) -> FunctionRef
    where
        O: Serialize + schemars::JsonSchema + Send + 'static,
        F: Fn(R) -> Result<O, Error> + Send + Sync + 'static,
    {
        self.iii.register_function(id, RegisterFunction::new(f))
    }

    /// Register an async function whose input type must match
    /// the call request format `R`.
    #[deprecated(
        since = "0.25.0",
        note = "use iii.register_function(id, RegisterFunction::new_async(f)) instead; see https://iii.dev/docs/upgrading/from-0-24-x"
    )]
    pub fn register_function_async<O, F, Fut>(&self, id: impl Into<String>, f: F) -> FunctionRef
    where
        O: Serialize + schemars::JsonSchema + Send + 'static,
        F: Fn(R) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<O, Error>> + Send + 'static,
    {
        self.iii
            .register_function(id, RegisterFunction::new_async(f))
    }
}

/// Worker metadata reported to the engine (language, framework, project).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelemetryOptions {
    /// Programming language of the worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Name of the project this worker belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    /// Framework name, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
}

/// Worker metadata for auto-registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetadata {
    pub runtime: String,
    pub version: String,
    /// Worker name reported to the engine. In managed identity mode, a
    /// non-empty `III_WORKER_NAME` overrides this value when the client is
    /// created.
    pub name: String,
    pub os: String,
    /// One-line, human/LLM-readable summary of what this worker does.
    /// Surfaces in `engine::workers::list` / `engine::workers::info`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<TelemetryOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    /// Namespace this worker belongs to, and therefore the one its calls and
    /// its trigger bindings resolve in unless they name another. Absent means
    /// the engine applies its default namespace. Managed clients also use
    /// `III_NAMESPACE` when no explicit namespace is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

impl Default for WorkerMetadata {
    fn default() -> Self {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let pid = std::process::id();
        let os_info = format!(
            "{} {} ({})",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::consts::FAMILY
        );

        let language = std::env::var("LANG")
            .or_else(|_| std::env::var("LC_ALL"))
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.split('.').next().unwrap_or(&s).to_string());

        let project_name = detect_project_name(None);

        Self {
            runtime: "rust".to_string(),
            version: SDK_VERSION.to_string(),
            name: format!("{}:{}", hostname, pid),
            os: os_info,
            description: None,
            pid: Some(pid),
            telemetry: Some(TelemetryOptions {
                language,
                project_name,
                ..Default::default()
            }),
            isolation: std::env::var("III_ISOLATION")
                .ok()
                .filter(|s| !s.is_empty()),
            namespace: None,
        }
    }
}

/// Selects where a worker connection gets its engine identity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkerIdentityMode {
    /// Use the supervisor-managed `III_WORKER_NAME` and `III_NAMESPACE` values
    /// when present. This is the default for normal worker connections.
    #[default]
    Managed,
    /// Keep the name from [`WorkerMetadata`] and the namespace from explicit
    /// options or metadata. Process-wide identity environment variables are
    /// ignored. Use this for auxiliary connections created by one worker.
    Explicit,
}

fn worker_name_from_env() -> Option<String> {
    std::env::var("III_WORKER_NAME")
        .ok()
        .filter(|name| !name.is_empty())
}

fn apply_managed_identity_from_env(metadata: &mut WorkerMetadata) {
    if let Some(managed_name) = worker_name_from_env() {
        metadata.name = managed_name;
    }
    if metadata.namespace.is_none() {
        metadata.namespace = std::env::var("III_NAMESPACE")
            .ok()
            .filter(|namespace| !namespace.trim().is_empty());
    }
}

/// Resolve the effective worker namespace: an explicit `InitOptions.namespace`
/// wins, then the `III_NAMESPACE` env var, then `None` (the engine applies its
/// default namespace).
/// `namespace` option > `III_NAMESPACE` > `None` (the engine then applies its
/// `default` namespace).
///
/// A declared-but-blank namespace is refused rather than read as "no
/// namespace". The two mean opposite things: absent asks for the engine's
/// default, blank names a namespace and gives nothing to name it with. Read as
/// absent, the worker registers in `default`, and every call and trigger it
/// makes now follows it there — a whole project quietly serving from the wrong
/// namespace, and the one thing an operator cannot see by reading the
/// declaration.
///
/// # Panics
///
/// When either source is present and holds only whitespace. This runs before
/// any connection, so it fails the worker at startup the way `iii compose`
/// refuses `--ns ""`, rather than producing a client that serves in a place
/// nobody asked for.
/// Refuses a namespace that was named and left blank, whatever named it.
///
/// Absent and blank ask for opposite things. Absent asks for the engine's
/// `default`; blank names a namespace and gives nothing to name it with. Read
/// as absent, the worker registers in `default`, and since a worker's calls and
/// triggers follow its namespace, the whole project quietly serves from a place
/// its declaration never named.
///
/// Panics rather than returning an error: this is a mistake in the program
/// text, made once at construction, and there is nothing a caller could do with
/// a `Result` here except unwrap it.
pub(crate) fn reject_blank_namespace(declared: &str, source: &str) {
    if declared.trim().is_empty() {
        panic!(
            "namespace is empty: `{source}` was set to {declared:?}. \
             Give it a name, or leave it unset to register in `default`."
        );
    }
}

/// The namespace one call or one binding resolves in.
///
/// `None` inherits the worker's. `Some("")` is refused: a namespace named and
/// left blank asks for the opposite of what absent asks for, and the two are
/// only ever confused by accident -- `??`, `or_else` and `or` each disagree
/// about the empty string, which is how the SDKs ended up forwarding it,
/// coercing it and dropping it respectively.
///
/// An `Err` rather than a panic: unlike a namespace declared once in
/// `InitOptions`, a per-call one can come from data, and a caller can do
/// something with the error.
pub(crate) fn call_namespace(
    explicit: Option<String>,
    worker: Option<String>,
    source: &str,
) -> Result<Option<String>, Error> {
    match explicit {
        Some(declared) if declared.trim().is_empty() => Err(Error::Handler(format!(
            "namespace is empty: `{source}` was set to {declared:?}. Give it a name, or leave \
             it unset to stay in this worker's namespace."
        ))),
        Some(declared) => Ok(Some(declared)),
        None => Ok(worker),
    }
}

/// The namespace in which one function invocation resolves.
///
/// An explicit namespace always wins. Without one, engine-owned builtins
/// (`engine::*` and `configuration::*`) stay in `default`; all other calls
/// inherit the worker's namespace.
pub(crate) fn invocation_namespace(
    explicit: Option<String>,
    worker: Option<String>,
    function_id: &str,
    source: &str,
) -> Result<Option<String>, Error> {
    let is_implicit = explicit.is_none();
    let namespace = call_namespace(explicit, worker, source)?;

    if is_implicit
        && (function_id.starts_with("engine::") || function_id.starts_with("configuration::"))
    {
        Ok(Some("default".to_string()))
    } else {
        Ok(namespace)
    }
}

pub(crate) fn resolve_namespace(explicit: Option<String>) -> Option<String> {
    if let Some(declared) = explicit {
        reject_blank_namespace(&declared, "InitOptions.namespace");
        return Some(declared);
    }

    // III_NAMESPACE is left alone when blank. `FOO=` is how a shell says "not
    // set" -- `III_NAMESPACE=${NS}` with NS unset produces exactly that -- so
    // reading it as absent is what the caller meant, and absent is a namespace
    // a worker may legitimately have none of. Only the option is a mistake:
    // nobody writes a namespace parameter and passes nothing on purpose.
    std::env::var("III_NAMESPACE")
        .ok()
        .filter(|managed| !managed.trim().is_empty())
}

/// Returns a project identifier for telemetry, derived from the current
/// working directory. Reads `[package] name` from `Cargo.toml` if present at
/// `cwd`; otherwise falls back to the basename of `cwd`. Returns `None`
/// only when both signals are unavailable.
///
/// No directory walking, only inspects `cwd` itself, so the SDK never
/// reads files outside the user's explicit working directory.
pub(crate) fn detect_project_name(cwd: Option<std::path::PathBuf>) -> Option<String> {
    let cwd = cwd.or_else(|| std::env::current_dir().ok())?;

    let manifest = cwd.join("Cargo.toml");
    if let Ok(content) = std::fs::read_to_string(&manifest) {
        if let Some(name) = parse_cargo_package_name(&content) {
            return Some(name);
        }
    }

    cwd.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Minimal parser for the `name` key inside the `[package]` table of a
/// `Cargo.toml` file. Avoids adding a TOML dependency for a single field.
fn parse_cargo_package_name(content: &str) -> Option<String> {
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(stripped) = trimmed.strip_prefix('[') {
            in_package = stripped.trim_end_matches(']').trim() == "package";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("name") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim().strip_prefix('"')?;
        let end = rest.find('"')?;
        let name = rest[..end].trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

// Bounds reservations for deliberately retained String spare capacity,
// not payload volume, allocator overhead, or physical heap/RSS.
const MAX_OUTBOUND_SPARE_BYTES: usize = 128 * 1024;

#[derive(Debug)]
struct SpareCapacityLease {
    used: Arc<AtomicUsize>,
    bytes: usize,
}

impl SpareCapacityLease {
    fn reserve(used: &Arc<AtomicUsize>, bytes: usize) -> Option<Self> {
        if bytes == 0 || bytes > MAX_OUTBOUND_SPARE_BYTES {
            return None;
        }
        let mut current = used.load(Ordering::Relaxed);
        loop {
            let next = current.checked_add(bytes)?;
            if next > MAX_OUTBOUND_SPARE_BYTES {
                return None;
            }
            match used.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
        Some(Self {
            used: Arc::clone(used),
            bytes,
        })
    }
}

impl Drop for SpareCapacityLease {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub(crate) struct PreparedEnvelope {
    pub(crate) payload: Utf8Bytes,
    registration_key: Option<String>,
    /// An observe message (`observe`, `unobserve`, `observerack`, reports,
    /// stats): only for an engine that advertised observe.
    observe_family: bool,
    /// A report frame's bytes, held against the report buffer's bound until
    /// the frame is written or dropped.
    report_lease: Option<crate::reporter::ReportLease>,
    /// A write of it failed: it may have reached the peer anyway.
    write_failed: bool,
    // The payload is dropped before its lease; send_ws borrows the envelope
    // throughout its shared-byte clone and network await.
    _spare_capacity: Option<SpareCapacityLease>,
}

impl PreparedEnvelope {
    pub(crate) fn from_json(
        mut payload: String,
        registration_key: Option<String>,
        budget: Option<&Arc<AtomicUsize>>,
    ) -> Self {
        let spare = payload.capacity() - payload.len();
        let lease = budget.and_then(|used| SpareCapacityLease::reserve(used, spare));
        if spare != 0 && lease.is_none() {
            // Compact disconnected/control messages and exhausted budgets.
            payload.shrink_to_fit();
        }
        Self {
            payload: payload.into(),
            registration_key,
            observe_family: false,
            report_lease: None,
            write_failed: false,
            _spare_capacity: lease,
        }
    }

    /// Holds the frame's bytes against the report bound while it waits.
    pub(crate) fn with_report_lease(mut self, lease: crate::reporter::ReportLease) -> Self {
        self.report_lease = Some(lease);
        self
    }

    /// Marks an observe message; see [`IIIClient::hold_observe_frames`].
    pub(crate) fn observe_family(mut self) -> Self {
        self.observe_family = true;
        self
    }
}

fn is_observe_family(message: &Message) -> bool {
    matches!(
        message,
        Message::Observe { .. }
            | Message::Unobserve { .. }
            | Message::ObserverAck { .. }
            | Message::InvocationsObserved { .. }
            | Message::LocalInvocationStats { .. }
    )
}

pub(crate) enum Outbound {
    Message(PreparedEnvelope),
    Shutdown,
}

type PendingInvocation = oneshot::Sender<Result<Value, Error>>;

// WebSocket transmitter type alias
type WsTx = futures_util::stream::SplitSink<WsStream, WsMessage>;

/// An `invokefunction` received from the engine.
struct InboundInvocation {
    invocation_id: Option<Uuid>,
    function_id: String,
    data: Value,
    traceparent: Option<String>,
    baggage: Option<String>,
    metadata: Option<Value>,
    caller: Option<Box<Caller>>,
    observe_depth: Option<u32>,
    activation: Option<Activation>,
}

/// Inject trace context headers for outbound messages.
pub(crate) fn inject_trace_headers() -> (Option<String>, Option<String>) {
    use iii_helpers::observability as context;
    (context::inject_traceparent(), context::inject_baggage())
}

/// Connection state for the III WebSocket client
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IIIConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

#[derive(Clone)]
pub struct FunctionRef {
    pub id: String,
    unregister_fn: Arc<dyn Fn() + Send + Sync>,
    state: Arc<FunctionEntry>,
}

impl FunctionRef {
    pub fn unregister(&self) {
        (self.unregister_fn)();
    }

    /// Whether any observer is active on this function, in one atomic load and
    /// without allocating. Check it before building an event nobody would
    /// receive.
    ///
    /// ```rust,no_run
    /// # fn demo(row_changed: &iii_sdk::runtime::FunctionRef) {
    /// if !row_changed.is_observed() {
    ///     return; // nothing serialized, nothing sent
    /// }
    /// # }
    /// ```
    #[inline]
    pub fn is_observed(&self) -> bool {
        self.state.is_observed()
    }

    /// The active observers, as the owner sees them.
    pub fn observers(&self) -> Vec<Observer> {
        self.state.observers()
    }

    /// Whether calls to this function from this worker currently run
    /// in-process (the engine confirmed it with `local_dispatch: true`).
    pub fn local_dispatch_active(&self) -> bool {
        self.state.local_dispatch()
    }
}

pub(crate) fn json_schema_for<T: schemars::JsonSchema>() -> Option<Value> {
    serde_json::to_value(
        schemars::r#gen::SchemaSettings::draft07()
            .into_generator()
            .into_root_schema_for::<T>(),
    )
    .ok()
}

/// Helper trait used internally to convert a sync function into a
/// [`RemoteFunctionHandlerWithMetadata`].
#[doc(hidden)]
pub trait IntoSyncHandler<Marker>: Send + Sync + 'static {
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata;
    fn request_format() -> Option<Value> {
        None
    }
    fn response_format() -> Option<Value> {
        None
    }
}

// 1-arg sync, deserializes the entire JSON input as T.
//
// Error type is fixed to [`Error`] (instead of generic `E: Display`) so
// closures using bare `Ok(...)` infer cleanly without explicit error
// annotations, required for ergonomic registration of `Fn(Value) -> ...`
// handlers. Other error types convert via `From<E> for Error` (impls
// for `String` / `&str` / `serde_json::Error` ship with the SDK).
impl<F, T, R> IntoSyncHandler<(T, R)> for F
where
    F: Fn(T) -> Result<R, Error> + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata {
        Arc::new(move |input: Value, _metadata: Option<Value>| {
            let output = serde_json::from_value::<T>(input)
                .map_err(|e| Error::Serde(e.to_string()))
                .and_then(&self)
                .and_then(|val| {
                    serde_json::to_value(&val).map_err(|e| Error::Serde(e.to_string()))
                });
            Box::pin(async move { output })
        })
    }

    fn request_format() -> Option<Value> {
        json_schema_for::<T>()
    }

    fn response_format() -> Option<Value> {
        json_schema_for::<R>()
    }
}

// 2-arg sync, deserializes the entire JSON input as T and passes the
// per-invocation metadata sidecar as the second handler argument.
impl<F, T, R> IntoSyncHandler<(T, Option<Value>, R)> for F
where
    F: Fn(T, Option<Value>) -> Result<R, Error> + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata {
        Arc::new(move |input: Value, metadata: Option<Value>| {
            let output = serde_json::from_value::<T>(input)
                .map_err(|e| Error::Serde(e.to_string()))
                .and_then(|arg| self(arg, metadata))
                .and_then(|val| {
                    serde_json::to_value(&val).map_err(|e| Error::Serde(e.to_string()))
                });
            Box::pin(async move { output })
        })
    }

    fn request_format() -> Option<Value> {
        json_schema_for::<T>()
    }

    fn response_format() -> Option<Value> {
        json_schema_for::<R>()
    }
}

// =============================================================================
// IntoAsyncHandler, async function schema-extraction trait
// =============================================================================

/// Helper trait used internally to convert an async function into a
/// [`RemoteFunctionHandlerWithMetadata`].
#[doc(hidden)]
pub trait IntoAsyncHandler<Marker>: Send + Sync + 'static {
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata;
    fn request_format() -> Option<Value> {
        None
    }
    fn response_format() -> Option<Value> {
        None
    }
}

/// Build the dispatchable handler for a typed async function: deserialize
/// the JSON input as `T`, run `f`, serialize the result. Deserialization
/// failures surface as [`Error::Serde`].
fn async_handler<F, T, Fut, R>(f: F) -> RemoteFunctionHandlerWithMetadata
where
    F: Fn(T) -> Fut + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + Send + 'static,
    Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    R: serde::Serialize + Send + 'static,
{
    Arc::new(
        move |input: Value,
              _metadata: Option<Value>|
              -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Value, Error>> + Send>,
        > {
            match serde_json::from_value::<T>(input) {
                Ok(arg) => {
                    let fut = f(arg);
                    Box::pin(async move {
                        fut.await.and_then(|val| {
                            serde_json::to_value(&val).map_err(|e| Error::Serde(e.to_string()))
                        })
                    })
                }
                Err(e) => {
                    let err = Error::Serde(e.to_string());
                    Box::pin(async move { Err(err) })
                }
            }
        },
    )
}

/// Build the dispatchable handler for a typed async function that also accepts
/// per-invocation metadata as its second argument.
fn async_handler_with_metadata<F, T, Fut, R>(f: F) -> RemoteFunctionHandlerWithMetadata
where
    F: Fn(T, Option<Value>) -> Fut + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + Send + 'static,
    Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    R: serde::Serialize + Send + 'static,
{
    Arc::new(
        move |input: Value,
              metadata: Option<Value>|
              -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Value, Error>> + Send>,
        > {
            match serde_json::from_value::<T>(input) {
                Ok(arg) => {
                    let fut = f(arg, metadata);
                    Box::pin(async move {
                        fut.await.and_then(|val| {
                            serde_json::to_value(&val).map_err(|e| Error::Serde(e.to_string()))
                        })
                    })
                }
                Err(e) => {
                    let err = Error::Serde(e.to_string());
                    Box::pin(async move { Err(err) })
                }
            }
        },
    )
}

// 1-arg async, deserializes the entire JSON input as T.
//
// Error type is fixed to [`Error`] (see [`IntoSyncHandler`] for the
// rationale). Use `From<E> for Error` to lift custom error types,
// or `?` propagation in the closure body.
impl<F, T, Fut, R> IntoAsyncHandler<(T, Fut, R)> for F
where
    F: Fn(T) -> Fut + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata {
        async_handler(self)
    }

    fn request_format() -> Option<Value> {
        json_schema_for::<T>()
    }

    fn response_format() -> Option<Value> {
        json_schema_for::<R>()
    }
}

// 2-arg async, deserializes the entire JSON input as T and passes the
// per-invocation metadata sidecar as the second handler argument.
impl<F, T, Fut, R> IntoAsyncHandler<(T, Option<Value>, Fut, R)> for F
where
    F: Fn(T, Option<Value>) -> Fut + Send + Sync + 'static,
    T: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    Fut: std::future::Future<Output = Result<R, Error>> + Send + 'static,
    R: serde::Serialize + schemars::JsonSchema + Send + 'static,
{
    fn into_handler(self) -> RemoteFunctionHandlerWithMetadata {
        async_handler_with_metadata(self)
    }

    fn request_format() -> Option<Value> {
        json_schema_for::<T>()
    }

    fn response_format() -> Option<Value> {
        json_schema_for::<R>()
    }
}

// =============================================================================
// RegisterFunction, single registration builder
// =============================================================================

fn empty_message() -> RegisterFunctionMessage {
    RegisterFunctionMessage {
        id: String::new(),
        description: None,
        request_format: None,
        response_format: None,
        metadata: None,
        invocation: None,
        observe: None,
    }
}

/// Function registration builder.
///
/// The function ID is supplied separately at registration time via
/// [`IIIClient::register_function`], `RegisterFunction` only carries the handler
/// and optional metadata.
///
/// Constructors:
/// - [`RegisterFunction::new`][]: sync function. Accepts both typed handlers
///   (schemas auto-extracted via `schemars`) and `Fn(Value, Option<Value>) -> Result<Value, Error>`
///   closures. The second argument is the per-invocation metadata sidecar and
///   is `None` when absent.
/// - [`RegisterFunction::new_async`][]: async equivalent of `new`.
/// - [`RegisterFunction::http`][]: function invoked over HTTP (Lambda,
///   Cloudflare Workers, etc.).
///
/// Builder methods (all consume `self`):
/// - [`description`](Self::description)
/// - [`metadata`](Self::metadata)
/// - [`request_format`](Self::request_format): overrides any auto-extracted schema.
/// - [`response_format`](Self::response_format): overrides any auto-extracted schema.
/// - [`observe`](Self::observe): how observers of the function are delivered,
///   and the hooks that run when one is added or removed.
/// - [`local_dispatch`](Self::local_dispatch): opt out of in-process calls.
pub struct RegisterFunction {
    message: RegisterFunctionMessage,
    handler: Option<RemoteFunctionHandlerWithMetadata>,
    observe: Option<Observe>,
    local_dispatch: bool,
}

/// The parts of a [`RegisterFunction`].
pub(crate) struct RegistrationParts {
    pub message: RegisterFunctionMessage,
    pub handler: Option<RemoteFunctionHandlerWithMetadata>,
    pub observe: Option<Observe>,
    pub local_dispatch: bool,
}

impl RegisterFunction {
    /// Create a registration for a **sync** typed function.
    ///
    /// Auto-extracts `request_format` / `response_format` from the function's
    /// argument and return types via `schemars`.
    pub fn new<F, M>(f: F) -> Self
    where
        F: IntoSyncHandler<M>,
    {
        let mut message = empty_message();
        message.request_format = F::request_format();
        message.response_format = F::response_format();
        Self {
            message,
            handler: Some(f.into_handler()),
            observe: None,
            local_dispatch: true,
        }
    }

    /// Create a registration for an **async** typed function.
    ///
    /// Auto-extracts `request_format` / `response_format` from the function's
    /// argument and return types via `schemars`.
    pub fn new_async<F, M>(f: F) -> Self
    where
        F: IntoAsyncHandler<M>,
    {
        let mut message = empty_message();
        message.request_format = F::request_format();
        message.response_format = F::response_format();
        Self {
            message,
            handler: Some(f.into_handler()),
            observe: None,
            local_dispatch: true,
        }
    }

    /// Create a registration for an **HTTP-invoked** function (Lambda,
    /// Cloudflare Workers, etc.). No local handler runs.
    pub fn http(config: HttpInvocationConfig) -> Self {
        let mut message = empty_message();
        message.invocation = Some(config);
        Self {
            message,
            handler: None,
            observe: None,
            local_dispatch: true,
        }
    }

    /// Set the function description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.message.description = Some(desc.into());
        self
    }

    /// Set function metadata.
    pub fn metadata(mut self, meta: Value) -> Self {
        self.message.metadata = Some(meta);
        self
    }

    /// Set the request format schema. Overrides any auto-extracted schema.
    pub fn request_format(mut self, schema: Value) -> Self {
        self.message.request_format = Some(schema);
        self
    }

    /// Set the response format schema. Overrides any auto-extracted schema.
    pub fn response_format(mut self, schema: Value) -> Self {
        self.message.response_format = Some(schema);
        self
    }

    /// How observers of this function are delivered, and the hooks that run
    /// when one is added or removed ([`Observe::owner`] / [`Observe::auto`]).
    /// Every function is observable without it, as `auto` without hooks.
    ///
    /// Against an engine without observe support the function registers as a
    /// plain function and its hooks never run (one warning is logged).
    pub fn observe(mut self, observe: Observe) -> Self {
        self.message.observe = Some(observe.descriptor.clone());
        self.observe = Some(observe);
        self
    }

    /// Whether calls to this function from this same worker may run
    /// in-process once the engine allows it. Default `true`.
    ///
    /// A local call runs on the caller's task, and a sync handler
    /// ([`RegisterFunction::new`]) runs inline on it. The call's timeout then
    /// cannot interrupt it (it never yields), and the caller waits for it
    /// however long it takes: make a handler that can block async, or opt it
    /// out here.
    pub fn local_dispatch(mut self, enabled: bool) -> Self {
        self.local_dispatch = enabled;
        self
    }

    pub(crate) fn into_parts(self) -> RegistrationParts {
        RegistrationParts {
            message: self.message,
            handler: self.handler,
            observe: self.observe,
            local_dispatch: self.local_dispatch,
        }
    }
}

/// Connection-loop timings.
///
/// Private knobs — unit tests shorten them to exercise reconnect paths
/// quickly.
// ponytail: knobs stay private; promote to InitOptions when an operator asks.
#[derive(Clone, Copy, Debug)]
struct ConnTimings {
    /// Cap on a single WS connect (TCP + TLS + HTTP upgrade). Without it a
    /// stalled socket wedges the reconnect loop forever (MOT-3857).
    connect_timeout: Duration,
    /// How often to send a WS ping so an idle link produces traffic.
    ping_interval: Duration,
    /// Reconnect if no frame (pongs included) arrives for this long —
    /// detects half-open sockets where the engine already dropped us and
    /// unregistered our functions.
    idle_timeout: Duration,
    /// Delay between reconnect attempts.
    retry_delay: Duration,
}

impl Default for ConnTimings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            ping_interval: Duration::from_secs(20),
            idle_timeout: Duration::from_secs(60),
            retry_delay: Duration::from_secs(2),
        }
    }
}

pub(crate) struct IIIInner {
    address: String,
    outbound: mpsc::UnboundedSender<Outbound>,
    spare_capacity_budget: Arc<AtomicUsize>,
    pub(crate) receiver: Mutex<Option<mpsc::UnboundedReceiver<Outbound>>>,
    pub(crate) running: AtomicBool,
    started: AtomicBool,
    pending: Mutex<HashMap<Uuid, PendingInvocation>>,
    functions: Mutex<HashMap<String, RemoteFunctionData>>,
    trigger_types: Mutex<HashMap<String, RemoteTriggerTypeData>>,
    triggers: Mutex<HashMap<String, RegisterTriggerMessage>>,
    worker_metadata: Mutex<Option<WorkerMetadata>>,
    connection_state: Mutex<IIIConnectionState>,
    /// Set when the engine rejects registration (fatal, no reconnect).
    fatal_error: Mutex<Option<Error>>,
    connection_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    headers: Mutex<Option<HashMap<String, String>>>,
    otel_config: Mutex<Option<OtelConfig>>,
    timings: Mutex<ConnTimings>,
    /// Engine-assigned worker id from `WorkerRegistered`; presented back via
    /// `Message::Reattach` on reconnect so the engine retires the previous
    /// connection before the registration replay.
    worker_id: Mutex<Option<String>>,
    /// Secret paired with `worker_id` (from the same `WorkerRegistered`
    /// frame); required by the engine to authorize the reattach — ids alone
    /// are publicly discoverable.
    reattach_token: Mutex<Option<String>>,
    identity_mode: WorkerIdentityMode,
    registration_changed: Notify,
    /// Observers this worker created, replayed on every connection that
    /// supports observe.
    observers: Mutex<HashMap<String, Arc<ObserverEntry>>>,
    /// `ENGINE_*` on the current connection: unknown until `WorkerRegistered`.
    engine_features: AtomicU8,
    /// The last engine advertised observe: outgoing calls carry `caller`.
    engine_observe_last: AtomicBool,
    /// Listeners of [`IIIClient::on_engine_supports_observe`].
    observe_support: Arc<Mutex<ObserveSupport>>,
    /// Functions already named in the warning that the engine leaves their
    /// observe hooks inactive; cleared by an engine that supports observe.
    warned_no_observe: Mutex<HashSet<String>>,
    /// Incremented on every established connection; a function confirmed on
    /// an older one waits for its new `functionregistered`.
    pub(crate) connection_epoch: AtomicU64,
    /// This worker as a caller: `worker_id` from the engine, `worker_name`.
    identity: ArcSwap<Caller>,
    reporter: Reporter,
    /// `on_observer_added` timeout; shortened by tests.
    hook_timeout_ms: AtomicU64,
    /// Observe messages queued on a connection that dropped, waiting for the
    /// next engine's features.
    held_frames: Mutex<Vec<PreparedEnvelope>>,
}

/// WebSocket client for communication with the III Engine.
///
/// Create with [`register_worker`](crate::register_worker).
#[derive(Clone)]
pub struct IIIClient {
    pub(crate) inner: Arc<IIIInner>,
}

impl IIIClient {
    /// Create a new III with default worker metadata (auto-detected runtime, os, hostname)
    pub fn new(address: &str) -> Self {
        Self::with_metadata(address, WorkerMetadata::default())
    }

    /// Create a new III with custom worker metadata.
    ///
    /// Non-empty managed identity environment variables override matching
    /// metadata fields so an orchestrator can assign the worker identity.
    pub fn with_metadata(address: &str, metadata: WorkerMetadata) -> Self {
        Self::with_identity(address, metadata, WorkerIdentityMode::Managed)
    }

    /// Create a client with explicit control over managed environment identity.
    pub(crate) fn with_identity(
        address: &str,
        mut metadata: WorkerMetadata,
        identity_mode: WorkerIdentityMode,
    ) -> Self {
        if identity_mode == WorkerIdentityMode::Managed {
            apply_managed_identity_from_env(&mut metadata);
        }
        let identity = Caller {
            worker_name: Some(metadata.name.clone()),
            ..Caller::default()
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let inner = IIIInner {
            address: address.into(),
            outbound: tx,
            spare_capacity_budget: Arc::new(AtomicUsize::new(0)),
            receiver: Mutex::new(Some(rx)),
            running: AtomicBool::new(false),
            started: AtomicBool::new(false),
            pending: Mutex::new(HashMap::new()),
            functions: Mutex::new(HashMap::new()),
            trigger_types: Mutex::new(HashMap::new()),
            triggers: Mutex::new(HashMap::new()),
            worker_metadata: Mutex::new(Some(metadata)),
            connection_state: Mutex::new(IIIConnectionState::Disconnected),
            fatal_error: Mutex::new(None),
            connection_thread: Mutex::new(None),
            headers: Mutex::new(None),
            otel_config: Mutex::new(None),
            timings: Mutex::new(ConnTimings::default()),
            worker_id: Mutex::new(None),
            reattach_token: Mutex::new(None),
            identity_mode,
            registration_changed: Notify::new(),
            observers: Mutex::new(HashMap::new()),
            engine_features: AtomicU8::new(ENGINE_FEATURES_UNKNOWN),
            engine_observe_last: AtomicBool::new(false),
            observe_support: Arc::default(),
            warned_no_observe: Mutex::new(HashSet::new()),
            connection_epoch: AtomicU64::new(0),
            identity: ArcSwap::from_pointee(identity),
            reporter: Reporter::default(),
            hook_timeout_ms: AtomicU64::new(crate::observable::HOOK_TIMEOUT.as_millis() as u64),
            held_frames: Mutex::new(Vec::new()),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    pub(crate) fn from_inner(inner: Arc<IIIInner>) -> Self {
        Self { inner }
    }

    pub(crate) fn functions(&self) -> MutexGuard<'_, HashMap<String, RemoteFunctionData>> {
        self.inner.functions.lock_or_recover()
    }

    pub(crate) fn function_entry(&self, function_id: &str) -> Option<Arc<FunctionEntry>> {
        self.functions()
            .get(function_id)
            .map(|data| data.observe.clone())
    }

    pub(crate) fn reporter(&self) -> &Reporter {
        &self.inner.reporter
    }

    pub(crate) fn hook_timeout(&self) -> Duration {
        Duration::from_millis(self.inner.hook_timeout_ms.load(Ordering::Relaxed))
    }

    pub(crate) fn connection_epoch(&self) -> u64 {
        self.inner.connection_epoch.load(Ordering::Acquire)
    }

    pub(crate) fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::SeqCst)
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.get_connection_state() == IIIConnectionState::Connected
    }

    /// The engine on the current connection advertised `observe.v1`.
    pub(crate) fn engine_observes(&self) -> bool {
        self.inner.engine_features.load(Ordering::SeqCst) == ENGINE_WITH_OBSERVE
    }

    /// This worker as a caller: `{ worker_id, worker_name }`.
    pub(crate) fn self_caller(&self) -> Caller {
        Caller::clone(&self.inner.identity.load())
    }

    fn update_identity(&self, worker_id: Option<String>, worker_name: Option<String>) {
        self.inner.identity.rcu(|current| Caller {
            worker_id: worker_id.clone().or_else(|| current.worker_id.clone()),
            worker_name: worker_name.clone().or_else(|| current.worker_name.clone()),
            ..Caller::default()
        });
    }

    /// `caller` and `observe_depth` for a call made through the engine from
    /// the current task: the handler it runs in, and that handler's caller as
    /// `parent`. Only towards an engine that advertised observe.
    pub(crate) fn outgoing_caller(&self) -> (Option<Box<Caller>>, Option<u32>) {
        if !self.inner.engine_observe_last.load(Ordering::Relaxed) {
            return (None, None);
        }
        with_current(|current| match current {
            None => (None, None),
            Some(cur) => (
                Some(Box::new(Caller {
                    worker_id: None,
                    worker_name: None,
                    function_id: Some(cur.function_id.clone()),
                    invocation_id: Some(cur.invocation_id.clone()),
                    parent: cur.caller.as_ref().and_then(parent_of),
                })),
                (cur.observe_depth > 0).then_some(cur.observe_depth),
            ),
        })
    }

    /// Get the engine WebSocket address this client connects to.
    pub fn address(&self) -> &str {
        &self.inner.address
    }

    /// Set custom worker metadata (call before connect).
    ///
    /// In managed identity mode, process-wide identity environment variables
    /// override the matching metadata fields.
    pub fn set_metadata(&self, mut metadata: WorkerMetadata) {
        if self.inner.identity_mode == WorkerIdentityMode::Managed {
            apply_managed_identity_from_env(&mut metadata);
        }
        self.update_identity(None, Some(metadata.name.clone()));
        *self.inner.worker_metadata.lock_or_recover() = Some(metadata);
    }

    /// Override the worker's target namespace (call before connect). Applied by
    /// [`register_worker`](crate::register_worker) after resolving
    /// `InitOptions.namespace` and `III_NAMESPACE`.
    ///
    /// # Panics
    ///
    /// If `namespace` is empty or only whitespace. `register_worker` resolves
    /// its own namespace before calling this, so the check is here for the
    /// callers that reach it directly: a blank one is the same mistake
    /// wherever it is made, and this entry point skipped it.
    pub fn set_namespace(&self, namespace: impl Into<String>) {
        let namespace = namespace.into();
        reject_blank_namespace(&namespace, "IIIClient::set_namespace");
        if let Some(md) = self.inner.worker_metadata.lock_or_recover().as_mut() {
            md.namespace = Some(namespace);
        }
    }

    /// The effective worker namespace (resolved from `InitOptions.namespace` /
    /// `III_NAMESPACE`), or `None` for the engine's `default`.
    pub fn namespace(&self) -> Option<String> {
        self.inner
            .worker_metadata
            .lock_or_recover()
            .as_ref()
            .and_then(|md| md.namespace.clone())
    }

    /// Fatal error that stopped the worker, if any. Set when the engine rejects
    /// registration (see [`Error::RegistrationRejected`]); the worker does not
    /// reconnect once this is populated.
    pub fn fatal_error(&self) -> Option<Error> {
        self.inner.fatal_error.lock_or_recover().clone()
    }

    /// Wait until the engine accepts this worker's initial registration.
    ///
    /// Returns [`Error::RegistrationRejected`] for a fatal identity conflict,
    /// [`Error::Timeout`] when the deadline expires, and
    /// [`Error::NotConnected`] when called before the client starts or after it
    /// stops.
    pub async fn wait_until_registered(&self, timeout: Duration) -> Result<(), Error> {
        if !self.inner.started.load(Ordering::SeqCst) {
            return Err(Error::NotConnected);
        }

        let deadline = Instant::now() + timeout;
        loop {
            let notified = self.inner.registration_changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            if let Some(err) = self.fatal_error() {
                return Err(err);
            }
            if self.inner.worker_id.lock_or_recover().is_some() {
                return Ok(());
            }
            if !self.inner.running.load(Ordering::SeqCst) {
                return Err(Error::NotConnected);
            }

            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err(Error::Timeout);
            }
        }
    }

    /// Set custom HTTP headers for the WebSocket handshake (call before connect).
    pub fn set_headers(&self, headers: HashMap<String, String>) {
        *self.inner.headers.lock_or_recover() = Some(headers);
    }

    /// Set OpenTelemetry configuration (call before connect)
    pub fn set_otel_config(&self, config: OtelConfig) {
        *self.inner.otel_config.lock_or_recover() = Some(config);
    }

    /// The telemetry config, with the engine URL and handshake headers of this
    /// connection filled in when the caller set none.
    fn take_otel_config(&self) -> OtelConfig {
        let mut config = self
            .inner
            .otel_config
            .lock_or_recover()
            .take()
            .unwrap_or_default();
        if config.engine_ws_url.is_none() {
            config.engine_ws_url = Some(self.inner.address.clone());
        }
        config
            .engine_ws_headers
            .get_or_insert_with(|| handshake_headers(self.inner.headers.lock_or_recover().clone()));
        config
    }

    pub(crate) fn connect(&self) {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }

        let receiver = self.inner.receiver.lock_or_recover().take();
        let Some(rx) = receiver else { return };

        self.inner.running.store(true, Ordering::SeqCst);

        let iii = self.clone();

        let otel_config = self.take_otel_config();

        // Spawn a dedicated OS thread with its own tokio runtime so
        // the connection loop is independent of the caller's runtime.
        // In Rust, a spawned thread does not keep the process alive on its own;
        // call shutdown() to signal the thread and join connection_thread so
        // run_connection() can exit cleanly before main() returns.
        let handle = std::thread::Builder::new()
            .name("iii-connection".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to create iii connection runtime");

                rt.block_on(async move {
                    let otel_active = telemetry::init_otel(otel_config).await;

                    tokio::spawn(run_reporter(iii.clone()));
                    iii.run_connection(rx).await;

                    if otel_active {
                        telemetry::shutdown_otel().await;
                    }
                });
            })
            .expect("failed to spawn iii connection thread");

        *self.inner.connection_thread.lock_or_recover() = Some(handle);
    }

    /// Shutdown the III client and wait for the connection thread to finish.
    ///
    /// This stops the connection loop, sends a shutdown signal, and joins
    /// the background connection thread. OpenTelemetry is flushed inside the
    /// connection thread before it exits.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.shutdown();
    /// ```
    pub fn shutdown(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        // reports still buffered will never go out: counted as INACTIVE
        self.inner.reporter.close();
        self.inner.registration_changed.notify_waiters();
        let _ = self.inner.outbound.send(Outbound::Shutdown);
        self.set_connection_state(IIIConnectionState::Disconnected);

        if let Some(handle) = self.inner.connection_thread.lock_or_recover().take() {
            let _ = handle.join();
        }
    }

    /// Shutdown the III client.
    ///
    /// This stops the connection loop and sends a shutdown signal, but it
    /// does not join `connection_thread`.
    ///
    /// This method returns without waiting for `run_connection()` to finish,
    /// making it safe to call from an async context without stalling the
    /// executor; [`shutdown`](Self::shutdown) blocks and joins the thread.
    /// The OpenTelemetry flush (`telemetry::shutdown_otel()`) still runs inside the connection thread
    /// after `run_connection()` returns, so it may not complete unless
    /// [`shutdown`](Self::shutdown) is used to join the thread.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # async fn docs() {
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.shutdown_async().await;
    /// # }
    /// ```
    pub async fn shutdown_async(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        // reports still buffered will never go out: counted as INACTIVE
        self.inner.reporter.close();
        self.inner.registration_changed.notify_waiters();
        let _ = self.inner.outbound.send(Outbound::Shutdown);
        self.set_connection_state(IIIConnectionState::Disconnected);
    }

    #[cfg(test)]
    fn register_function_inner(
        &self,
        message: RegisterFunctionMessage,
        handler: Option<RemoteFunctionHandlerWithMetadata>,
    ) -> FunctionRef {
        self.register_function_parts(RegistrationParts {
            message,
            handler,
            observe: None,
            local_dispatch: true,
        })
    }

    fn register_function_parts(&self, parts: RegistrationParts) -> FunctionRef {
        let RegistrationParts {
            message,
            handler,
            observe,
            local_dispatch,
        } = parts;
        let id = message.id.clone();
        if id.trim().is_empty() {
            panic!("id is required");
        }
        if let Some(observe) = &observe {
            if observe.descriptor.args_format.is_some()
                && observe.delivery() != crate::protocol::Delivery::Owner
            {
                panic!(
                    "{}: function '{id}' declares args_format with auto delivery; args are \
                     for an owner that routes by them, use Observe::owner()",
                    observe_codes::INVALID_OBSERVE_DESCRIPTOR
                );
            }
        }
        let state = Arc::new(FunctionEntry::new(
            id.clone(),
            observe.as_ref(),
            handler.is_some(),
            local_dispatch,
        ));
        let envelope = match Self::prepare_envelope(&message.to_message()) {
            Ok(envelope) => envelope,
            Err(error) => {
                tracing::warn!(error = %error, "Function registration rejected locally");
                // A rejected handle must not unregister a previous owner of this ID.
                return FunctionRef {
                    id,
                    unregister_fn: Arc::new(|| {}),
                    state,
                };
            }
        };
        let data = RemoteFunctionData {
            message,
            handler,
            observe: state.clone(),
        };
        let mut funcs = self.inner.functions.lock_or_recover();
        match funcs.entry(id.clone()) {
            std::collections::hash_map::Entry::Occupied(_) => {
                panic!("function id '{}' already registered", id);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(data);
            }
        }
        drop(funcs);
        let _ = self.send_prepared(envelope);
        // Registered after this engine said it has no observe: warned here,
        // as `on_engine_features` did for the functions it found.
        if state.needs_observe
            && self.inner.engine_features.load(Ordering::SeqCst) == ENGINE_WITHOUT_OBSERVE
        {
            self.warn_no_observe(vec![id.clone()]);
        }

        let iii = self.clone();
        let unregister_id = id.clone();
        let unregister_fn = Arc::new(move || {
            let removed = iii.inner.functions.lock_or_recover().remove(&unregister_id);
            if let Some(data) = removed {
                data.observe.release();
                data.observe.enqueue(&iii, HookOp::Reset { last: true });
                iii.reporter().discard(&unregister_id);
            }
            let _ = iii.send_message(Message::UnregisterFunction {
                id: unregister_id.clone(),
            });
        });

        FunctionRef {
            id,
            unregister_fn,
            state,
        }
    }

    /// Register a function with the engine.
    ///
    /// Argument order matches the Node and Python SDKs:
    /// `(id, registration)`.
    ///
    /// # Arguments
    /// * `id` - Unique identifier for the function.
    /// * `registration` - Built via [`RegisterFunction::new`],
    ///   [`RegisterFunction::new_async`], or [`RegisterFunction::http`].
    ///   Chain `.description(...)`, `.metadata(...)`, `.request_format(...)`,
    ///   `.response_format(...)` as needed.
    ///
    /// # Panics
    /// Panics if `id` is empty or already registered, and with
    /// `INVALID_OBSERVE_DESCRIPTOR` when an [`Observe::auto`] descriptor
    /// declares an `args_format`.
    ///
    /// # Examples
    /// ```rust,no_run
    /// use iii_sdk::{register_worker, InitOptions, Error, RegisterFunction};
    /// use serde::{Deserialize, Serialize};
    /// use schemars::JsonSchema;
    ///
    /// #[derive(Deserialize, JsonSchema)]
    /// struct Input { name: String }
    /// #[derive(Serialize, JsonSchema)]
    /// struct Output { message: String }
    ///
    /// async fn greet(input: Input) -> Result<Output, Error> {
    ///     Ok(Output { message: format!("Hello, {}!", input.name) })
    /// }
    ///
    /// let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.register_function(
    ///     "greetings::greet",
    ///     RegisterFunction::new_async(greet).description("Greets a user"),
    /// );
    /// ```
    ///
    /// Registration metadata stays on the builder, so the no-metadata path remains
    /// clean:
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions, RegisterFunction};
    /// # use serde_json::{json, Value};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.register_function(
    ///     "orders::create",
    ///     RegisterFunction::new_async(|input: Value| async move { Ok(input) })
    ///         .metadata(json!({"owner": "billing-team", "priority": "high"})),
    /// );
    /// ```
    ///
    /// Untyped handler taking `serde_json::Value`:
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions, RegisterFunction};
    /// # use serde_json::{json, Value};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.register_function(
    ///     "debug::echo",
    ///     RegisterFunction::new_async(|input: Value| async move { Ok(json!({"echo": input})) }),
    /// );
    /// ```
    ///
    /// HTTP-invoked function:
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions, RegisterFunction};
    /// # use iii_helpers::http::{HttpInvocationConfig, HttpMethod};
    /// # use std::collections::HashMap;
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// let config = HttpInvocationConfig {
    ///     url: "https://example.com/invoke".into(),
    ///     method: HttpMethod::Post,
    ///     timeout_ms: Some(30_000),
    ///     headers: HashMap::new(),
    ///     auth: None,
    /// };
    /// worker.register_function("ext::lambda", RegisterFunction::http(config));
    /// ```
    pub fn register_function(
        &self,
        id: impl Into<String>,
        registration: RegisterFunction,
    ) -> FunctionRef {
        let mut parts = registration.into_parts();
        parts.message.id = id.into();
        self.register_function_parts(parts)
    }

    /// Register a custom trigger type with the engine.
    ///
    /// Returns a [`TriggerTypeRef`] handle that can register triggers and
    /// functions with compile-time validated types.
    ///
    /// It keeps working in 0.25 and logs one warning per process. A new
    /// provider registers a function with an observe descriptor instead
    /// (`RegisterFunction::new(..).observe(Observe::owner().args_format::<Config>().handler(..))`):
    /// [`ObserveHandler`](crate::ObserveHandler) replaces [`TriggerHandler`],
    /// and observers' args replace trigger configs.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # #![allow(deprecated)]
    /// # use iii_sdk::{IIIClient, RegisterTriggerType};
    /// # use iii_sdk::trigger::{TriggerConfig, TriggerHandler};
    /// # struct MyHandler;
    /// # #[async_trait::async_trait]
    /// # impl TriggerHandler for MyHandler {
    /// #     async fn register_trigger(&self, _: TriggerConfig) -> Result<(), iii_sdk::Error> { Ok(()) }
    /// #     async fn unregister_trigger(&self, _: TriggerConfig) -> Result<(), iii_sdk::Error> { Ok(()) }
    /// # }
    /// # #[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)] struct MyConfig { url: String }
    /// # #[derive(serde::Deserialize, schemars::JsonSchema)] struct MyRequest { data: String }
    /// # let worker = IIIClient::new("ws://localhost:49134");
    /// let my_trigger = worker.register_trigger_type(
    ///     RegisterTriggerType::new("my-trigger", "My custom trigger", MyHandler)
    ///         .trigger_request_format::<MyConfig>()
    ///         .call_request_format::<MyRequest>(),
    /// );
    ///
    /// // Compile-time safe: config must be MyConfig, function input must be MyRequest
    /// my_trigger.register_function("my::handler", |req: MyRequest| -> Result<serde_json::Value, iii_sdk::Error> {
    ///     Ok(serde_json::json!({ "data": req.data }))
    /// });
    /// my_trigger.register_trigger("my::handler", MyConfig { url: "/hook".into() });
    /// ```
    #[deprecated(
        since = "0.25.0",
        note = "register a function with RegisterFunction::observe(Observe::owner()..) instead; see https://iii.dev/docs/upgrading/from-0-24-x"
    )]
    pub fn register_trigger_type<H, C, R>(
        &self,
        trigger_type: RegisterTriggerType<H, C, R>,
    ) -> TriggerTypeRef<C, R>
    where
        H: TriggerHandler + 'static,
    {
        warn_register_trigger_type();
        self.register_trigger_type_inner(trigger_type)
    }

    /// [`register_trigger_type`](Self::register_trigger_type) without the
    /// runtime deprecation warning: for iii's own crates that still provide a
    /// trigger type while its subscribers move to observers, so a deprecation
    /// meant for user code is not printed on every run. Not part of the
    /// public API.
    #[doc(hidden)]
    #[deprecated(
        since = "0.25.0",
        note = "register a function with RegisterFunction::observe(Observe::owner()..) instead; see https://iii.dev/docs/upgrading/from-0-24-x"
    )]
    pub fn register_trigger_type_without_warning<H, C, R>(
        &self,
        trigger_type: RegisterTriggerType<H, C, R>,
    ) -> TriggerTypeRef<C, R>
    where
        H: TriggerHandler + 'static,
    {
        self.register_trigger_type_inner(trigger_type)
    }

    fn register_trigger_type_inner<H, C, R>(
        &self,
        trigger_type: RegisterTriggerType<H, C, R>,
    ) -> TriggerTypeRef<C, R>
    where
        H: TriggerHandler + 'static,
    {
        let message = RegisterTriggerTypeMessage {
            id: trigger_type.id,
            description: trigger_type.description,
            trigger_request_format: trigger_type.trigger_request_format,
            call_request_format: trigger_type.call_request_format,
            // Left to the engine, which files it under this connection's
            // namespace. A worker providing a trigger type provides it for the
            // project it belongs to.
            namespace: None,
        };

        let trigger_type_id = message.id.clone();

        // Validate before touching ownership: rollback by ID could remove a
        // previously accepted handler, including a newer concurrent generation.
        match Self::prepare_envelope(&message.to_message()) {
            Ok(envelope) => {
                self.inner.trigger_types.lock_or_recover().insert(
                    message.id.clone(),
                    RemoteTriggerTypeData {
                        message,
                        handler: Arc::new(trigger_type.handler),
                    },
                );

                let _ = self.send_prepared(envelope);
            }
            Err(error) => {
                tracing::warn!(error = %error, "Trigger type registration rejected locally")
            }
        }

        TriggerTypeRef {
            iii: self.clone(),
            trigger_type_id,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Unregister a previously registered trigger type.
    ///
    /// **Deprecated since 0.25.0** with [`register_trigger_type`](Self::register_trigger_type).
    ///
    /// # Examples
    /// ```rust,no_run
    /// # #![allow(deprecated)]
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.unregister_trigger_type("cron");
    /// ```
    #[deprecated(
        since = "0.25.0",
        note = "unregister the function registered with RegisterFunction::observe instead; see https://iii.dev/docs/upgrading/from-0-24-x"
    )]
    pub fn unregister_trigger_type(&self, id: impl Into<String>) {
        let id = id.into();
        self.inner.trigger_types.lock_or_recover().remove(&id);
        let msg = UnregisterTriggerTypeMessage { id };
        let _ = self.send_message(msg.to_message());
    }

    /// Bind a trigger configuration to a registered function.
    /// <!-- docs:expand-params -->
    ///
    /// It keeps working in 0.25 and logs one warning per process. The http,
    /// cron, queue, state, stream, pubsub and database workers still publish
    /// trigger types, so keep using it for them until they ship source
    /// functions; observe any other function with [`observe`](Self::observe).
    ///
    /// # Arguments
    /// * `input` - Trigger registration input with trigger_type, function_id, and config.
    ///
    /// # Examples
    /// ```rust
    /// # #![allow(deprecated)]
    /// # use iii_sdk::IIIClient;
    /// # use iii_sdk::protocol::RegisterTriggerInput;
    /// # use serde_json::json;
    /// # let worker = IIIClient::new("ws://localhost:49134");
    /// let trigger = worker.register_trigger(RegisterTriggerInput::new(
    ///     "http",
    ///     "greet",
    ///     json!({ "api_path": "/greet", "http_method": "GET" }),
    /// ))?;
    /// // Later...
    /// trigger.unregister();
    /// # Ok::<(), iii_sdk::Error>(())
    /// ```
    #[deprecated(
        since = "0.25.0",
        note = "use iii.observe(Source::function(..).args(..), target) instead; see https://iii.dev/docs/upgrading/from-0-24-x"
    )]
    pub fn register_trigger(&self, input: RegisterTriggerInput) -> Result<Trigger, Error> {
        warn_register_trigger();
        let id = Uuid::new_v4().to_string();
        let message = RegisterTriggerMessage {
            id: id.clone(),
            trigger_type: input.trigger_type,
            function_id: input.function_id,
            config: input.config,
            metadata: input.metadata,
            // Unset means this worker's namespace, not the engine's default.
            // A trigger names a function, and the function a worker registers
            // lands in the worker's namespace, so defaulting anywhere else
            // registers a trigger that fires and resolves nothing. Naming
            // another namespace, `default` included, stays a matter of saying
            // so.
            namespace: call_namespace(
                input.namespace,
                self.namespace(),
                "RegisterTriggerInput.namespace",
            )?,
            // Passed through untouched, including when it is `None`. `None` is
            // the engine's two-step resolution — this worker's namespace, then
            // the engine's own — which is what lets a project ship its own
            // provider for a type id the engine also provides while every
            // unmigrated worker keeps reaching the engine's without saying so.
            // The SDK cannot decide this locally: a sibling worker in the same
            // project may be the one providing the type.
            trigger_namespace: input.trigger_namespace,
        };

        self.send_message(message.to_message())?;
        self.inner
            .triggers
            .lock_or_recover()
            .insert(message.id.clone(), message.clone());

        let iii = self.clone();
        let trigger_type = message.trigger_type.clone();
        let unregister_id = message.id.clone();
        let unregister_fn = Arc::new(move || {
            let _ = iii.inner.triggers.lock_or_recover().remove(&unregister_id);
            let msg = UnregisterTriggerMessage {
                id: unregister_id.clone(),
                trigger_type: trigger_type.clone(),
            };
            let _ = iii.send_message(msg.to_message());
        });

        Ok(Trigger::new(unregister_fn))
    }

    /// Invoke a function.
    /// <!-- docs:expand-params: TriggerRequest -->
    ///
    /// The routing behavior depends on the `action` field of the request:
    /// - No action: synchronous, waits for the function to return.
    /// - [`TriggerAction::Enqueue`]: async via named queue.
    /// - [`TriggerAction::Void`][]: fire-and-forget.
    ///
    /// A function this worker registered itself runs in-process once the
    /// engine allowed it ("local dispatch", see [`Dispatch`]): same result,
    /// error shape and timeout, and nothing on the wire unless an observer's
    /// filters match the call. A sync handler then runs inline on the
    /// caller's task, where the timeout cannot interrupt it: a sync handler
    /// that can block should be async, or registered with
    /// [`RegisterFunction::local_dispatch(false)`](RegisterFunction::local_dispatch).
    /// [`TriggerRequest::dispatch`] with [`Dispatch::Remote`]
    /// forces the engine path. Calls made from inside a handler carry that
    /// handler as their `caller`.
    ///
    /// # Examples
    /// ```rust
    /// # use iii_sdk::{IIIClient, TriggerAction};
    /// # use iii_sdk::protocol::TriggerRequest;
    /// # use serde_json::json;
    /// # async fn example(worker: &IIIClient) -> Result<(), iii_sdk::Error> {
    /// // Synchronous
    /// let result = worker.trigger(TriggerRequest {
    ///     function_id: "greet".to_string(),
    ///     payload: json!({"name": "World"}),
    ///     action: None,
    ///     timeout_ms: None,
    /// }).await?;
    ///
    /// // Fire-and-forget
    /// worker.trigger(TriggerRequest {
    ///     function_id: "notify".to_string(),
    ///     payload: json!({}),
    ///     action: Some(TriggerAction::Void),
    ///     timeout_ms: None,
    /// }).await?;
    ///
    /// // Enqueue (the queue must be declared in the queue worker's
    /// // queue_configs)
    /// let receipt = worker.trigger(TriggerRequest {
    ///     function_id: "iii::durable::publish".to_string(),
    ///     payload: json!({"topic": "test"}),
    ///     action: Some(TriggerAction::Enqueue { queue: "test".to_string() }),
    ///     timeout_ms: None,
    /// }).await?;
    ///
    /// // Metadata
    /// worker.trigger(
    ///     TriggerRequest {
    ///         function_id: "audit::write".to_string(),
    ///         payload: json!({"event": "checkout"}),
    ///         action: Some(TriggerAction::Void),
    ///         timeout_ms: None,
    ///     }
    ///     .metadata(json!({"tenant": "acme"})),
    /// ).await?;
    ///
    /// # Ok(())
    /// # }
    /// ```
    pub async fn trigger(
        &self,
        request: impl Into<TriggerRequestWithMetadata>,
    ) -> Result<Value, Error> {
        let TriggerRequestWithMetadata {
            request: req,
            metadata,
            namespace,
            dispatch,
        } = request.into();
        // Engine-owned builtins always live in `default`. Other implicit calls
        // stay in the caller's namespace. An explicit request namespace wins.
        let namespace = invocation_namespace(
            namespace,
            self.namespace(),
            &req.function_id,
            "TriggerRequest.namespace",
        )?;

        // Local dispatch: a function this worker hosts, confirmed by the
        // engine, runs in-process. `enqueue` keeps its queue receipt, and a
        // call nested too deep in local calls (a function recursing into
        // itself) goes through the engine rather than deeper into the stack.
        if dispatch == Dispatch::Auto
            && !matches!(req.action, Some(TriggerAction::Enqueue { .. }))
            && crate::context::local_depth() < crate::dispatch::MAX_LOCAL_DEPTH
        {
            if let Some((handler, entry)) =
                self.local_target(&req.function_id, namespace.as_deref())
            {
                return self.invoke_local(handler, entry, req, metadata).await;
            }
        }

        let (caller, observe_depth) = self.outgoing_caller();
        self.invoke_remote(RemoteCall {
            function_id: req.function_id,
            payload: req.payload,
            action: req.action,
            timeout: req.timeout_ms.map(Duration::from_millis),
            metadata,
            namespace,
            caller,
            observe_depth,
            activation: None,
        })
        .await
    }

    /// Sends an invocation through the engine and waits for its result (or
    /// its queue receipt), or returns at once for a `void` one.
    pub(crate) async fn invoke_remote(&self, call: RemoteCall) -> Result<Value, Error> {
        let RemoteCall {
            function_id,
            payload,
            action,
            timeout,
            metadata,
            namespace,
            caller,
            observe_depth,
            activation,
        } = call;
        let (tp, bg) = inject_trace_headers();

        // Void is fire-and-forget, no invocation_id, no response
        if matches!(action, Some(TriggerAction::Void)) {
            self.send_message(Message::InvokeFunction {
                invocation_id: None,
                function_id,
                data: payload,
                traceparent: tp,
                baggage: bg,
                action,
                metadata,
                namespace,
                caller,
                observe_depth,
                activation,
            })?;
            return Ok(Value::Null);
        }

        // Enqueue and default: use invocation_id to receive acknowledgement/result
        let timeout = timeout.unwrap_or(Duration::from_millis(DEFAULT_TIMEOUT_MS));
        let invocation_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();

        self.inner
            .pending
            .lock_or_recover()
            .insert(invocation_id, tx);

        if let Err(error) = self.send_message(Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id,
            data: payload,
            traceparent: tp,
            baggage: bg,
            action,
            metadata,
            namespace,
            caller,
            observe_depth,
            activation,
        }) {
            self.inner.pending.lock_or_recover().remove(&invocation_id);
            return Err(error);
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::NotConnected),
            Err(_) => {
                self.inner.pending.lock_or_recover().remove(&invocation_id);
                Err(Error::Timeout)
            }
        }
    }

    /// Get the current connection state.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # use iii_sdk::runtime::IIIConnectionState;
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// if worker.get_connection_state() != IIIConnectionState::Connected {
    ///     eprintln!("engine not reachable yet");
    /// }
    /// ```
    pub fn get_connection_state(&self) -> IIIConnectionState {
        *self.inner.connection_state.lock_or_recover()
    }

    /// Whether the engine on the current connection supports observe: it
    /// advertised `observe.v1` in its `WorkerRegistered`. `None` until that
    /// arrives, before the first connection and again while reconnecting.
    ///
    /// Without it, `observe()` fails with `UNSUPPORTED` and observable
    /// functions register as plain functions whose hooks never run; a
    /// worker can keep a fallback for older engines (its former trigger
    /// type, say) when this is `Some(false)`. Pair it with
    /// [`on_engine_supports_observe`](Self::on_engine_supports_observe) to
    /// follow changes across reconnects.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// if worker.engine_supports_observe() == Some(false) {
    ///     eprintln!("engine without observe: keeping the fallback");
    /// }
    /// ```
    pub fn engine_supports_observe(&self) -> Option<bool> {
        match self.inner.engine_features.load(Ordering::SeqCst) {
            ENGINE_WITH_OBSERVE => Some(true),
            ENGINE_WITHOUT_OBSERVE => Some(false),
            _ => None,
        }
    }

    /// Calls `listener` with the engine's observe support (see
    /// [`engine_supports_observe`](Self::engine_supports_observe)) when it
    /// becomes known and whenever a later connection's `WorkerRegistered`
    /// changes it (an engine upgraded or rolled back between reconnects).
    /// Not called for a reconnect to an engine that says the same, nor while
    /// reconnecting (the value is unknown then, not changed).
    ///
    /// A listener is called with the current value right away when it is
    /// known; one added while it is not (before the first connection,
    /// while reconnecting) hears it from the next `WorkerRegistered`, the
    /// same value or not. Either way it sees every value from the current
    /// one on, in order. Listeners run on the same thread as
    /// [`ObserverHandle::on_state_change`](crate::ObserverHandle::on_state_change)
    /// listeners, never on the connection's read loop: one may call the
    /// client.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// let listener = worker.on_engine_supports_observe(|observe| {
    ///     if !observe {
    ///         eprintln!("engine without observe: registering the fallback");
    ///     }
    /// });
    /// // later
    /// listener.unsubscribe();
    /// ```
    pub fn on_engine_supports_observe<F>(&self, listener: F) -> ObserveSupportListener
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        let call: ObserveSupportFn = Arc::new(listener);
        let active = Arc::new(AtomicBool::new(true));
        let mut support = self.inner.observe_support.lock_or_recover();
        // read under the lock: `on_engine_features` stores the value before
        // taking it, so a change either shows here or finds `heard` stale
        let current = self.engine_supports_observe();
        let id = support.next_id;
        support.next_id += 1;
        support.listeners.push(SupportListener {
            id,
            active: active.clone(),
            heard: current,
            call: call.clone(),
        });
        if let Some(observe) = current {
            ObserveSupport::notify(vec![(active.clone(), call)], observe);
        }
        ObserveSupportListener {
            support: Arc::downgrade(&self.inner.observe_support),
            id,
            active,
        }
    }

    pub(crate) fn set_connection_state(&self, state: IIIConnectionState) {
        let mut current = self.inner.connection_state.lock_or_recover();
        if *current == state {
            return;
        }
        *current = state;
    }

    /// Register this worker's metadata with the engine (called automatically on connect)
    fn register_worker_metadata(&self) {
        if let Some(mut metadata) = self.inner.worker_metadata.lock_or_recover().clone() {
            let fw = metadata
                .telemetry
                .as_ref()
                .and_then(|t| t.framework.as_deref())
                .unwrap_or("");
            if fw.is_empty() {
                let telem = metadata.telemetry.get_or_insert_with(Default::default);
                telem.framework = Some("iii-rust".to_string());
            }
            if let Ok(mut value) = serde_json::to_value(metadata) {
                // The protocol features this SDK speaks: the engine pushes
                // observe messages only to a connection that declared them.
                if let Value::Object(map) = &mut value {
                    map.insert("features".to_string(), serde_json::json!(SDK_FEATURES));
                }
                let _ = self.send_message(Message::InvokeFunction {
                    invocation_id: None,
                    function_id: "engine::workers::register".to_string(),
                    data: value,
                    traceparent: None,
                    baggage: None,
                    action: Some(TriggerAction::Void),
                    metadata: None,
                    namespace: None,
                    caller: None,
                    observe_depth: None,
                    activation: None,
                });
            }
        }
    }

    // A single-frame JSON envelope must fit Axum's inclusive 16 MiB frame limit.
    fn receive_config() -> WebSocketConfig {
        WebSocketConfig::default()
            .max_frame_size(Some(MAX_JSON_MESSAGE_BYTES))
            .max_message_size(Some(MAX_JSON_MESSAGE_BYTES))
    }

    fn prepare_outbound(&self, message: &Message) -> Result<PreparedEnvelope, Error> {
        let budget = if matches!(
            message,
            Message::InvokeFunction { .. } | Message::InvocationResult { .. }
        ) && self.get_connection_state() == IIIConnectionState::Connected
        {
            Some(&self.inner.spare_capacity_budget)
        } else {
            None
        };
        Self::prepare_envelope_with_budget(message, budget)
    }

    fn prepare_envelope(message: &Message) -> Result<PreparedEnvelope, Error> {
        Self::prepare_envelope_with_budget(message, None)
    }

    fn prepare_envelope_with_budget(
        message: &Message,
        budget: Option<&Arc<AtomicUsize>>,
    ) -> Result<PreparedEnvelope, Error> {
        let payload = serde_json::to_string(message)?;
        let size = payload.len();
        if size <= MAX_JSON_FRAME_BYTES {
            let envelope =
                PreparedEnvelope::from_json(payload, Self::registration_key(message), budget);
            return Ok(if is_observe_family(message) {
                envelope.observe_family()
            } else {
                envelope
            });
        }
        let error_message = format!(
            "Serialized JSON envelope is {size} bytes; limit is {MAX_JSON_FRAME_BYTES} bytes. Use channels for large data."
        );
        if let Message::InvocationResult {
            invocation_id,
            function_id,
            ..
        } = message
        {
            let fallback = Message::InvocationResult {
                invocation_id: *invocation_id,
                function_id: function_id.clone(),
                result: None,
                error: Some(ErrorBody {
                    code: "payload_too_large".to_string(),
                    message: error_message.clone(),
                    stacktrace: None,
                }),
                traceparent: None,
                baggage: None,
            };
            let payload = serde_json::to_string(&fallback)?;
            if payload.len() <= MAX_JSON_FRAME_BYTES {
                return Ok(PreparedEnvelope::from_json(payload, None, None));
            }
        }
        Err(Error::Remote {
            code: "payload_too_large".to_string(),
            message: error_message,
            stacktrace: None,
        })
    }

    pub(crate) fn send_message(&self, message: Message) -> Result<(), Error> {
        let envelope = match self.prepare_outbound(&message) {
            Ok(envelope) => envelope,
            Err(error) => {
                tracing::warn!(error = %error, "JSON envelope rejected locally");
                return Err(error);
            }
        };
        self.send_prepared(envelope)
    }

    pub(crate) fn send_prepared(&self, envelope: PreparedEnvelope) -> Result<(), Error> {
        if !self.inner.running.load(Ordering::SeqCst) {
            if envelope.report_lease.is_some() {
                // a report frame that will never be sent is counted, not lost
                self.inner
                    .reporter
                    .count_frame(envelope.payload.as_str(), observe_codes::INACTIVE);
            }
            return Ok(());
        }

        self.inner
            .outbound
            .send(Outbound::Message(envelope))
            .map_err(|_| Error::NotConnected)
    }

    /// Opens the worker connection; `TCP_NODELAY` is negotiated with the
    /// engine (see `transport::connect`).
    async fn connect_socket(&self) -> WsConnectResult {
        let headers = handshake_headers(self.inner.headers.lock_or_recover().clone());
        let request = handshake_request(&self.inner.address, &headers)?;
        transport::connect(request, Some(Self::receive_config())).await
    }

    async fn run_connection(&self, mut rx: mpsc::UnboundedReceiver<Outbound>) {
        let mut queue: Vec<PreparedEnvelope> = Vec::new();
        // Report frames of a dropped connection, in order, until they are
        // back in the report buffer.
        let mut reports: Vec<PreparedEnvelope> = Vec::new();
        let mut has_connected_before = false;

        while self.inner.running.load(Ordering::SeqCst) {
            let t = *self.inner.timings.lock_or_recover();
            self.set_connection_state(if has_connected_before {
                IIIConnectionState::Reconnecting
            } else {
                IIIConnectionState::Connecting
            });

            // Cap the whole connect (TCP + TLS + WS upgrade): a stalled
            // socket otherwise wedges this loop forever and the worker sits
            // in Reconnecting with zero functions registered engine-side
            // (MOT-3857).
            let connect_result =
                tokio::time::timeout(t.connect_timeout, self.connect_socket()).await;

            match connect_result {
                Ok(Ok((stream, _))) => {
                    tracing::info!(address = %self.inner.address, "iii connected");
                    has_connected_before = true;
                    // A new connection: the engine's features are unknown until
                    // its `WorkerRegistered`, and every function waits for its
                    // `functionregistered` here before its reports go out.
                    self.inner
                        .engine_features
                        .store(ENGINE_FEATURES_UNKNOWN, Ordering::SeqCst);
                    self.inner.connection_epoch.fetch_add(1, Ordering::AcqRel);
                    self.set_connection_state(IIIConnectionState::Connected);
                    let (mut ws_tx, mut ws_rx) = stream.split();

                    // Reconnect: present the previous engine-assigned identity
                    // BEFORE the registration replay so the engine retires the
                    // old connection and the replay lands on a clean slate
                    // instead of racing its cleanup. The token proves we ARE
                    // that worker (ids alone are publicly listable). Sent
                    // directly (not via `queue`) so it never accumulates
                    // across retries.
                    let previous_worker_id = self.inner.worker_id.lock_or_recover().clone();
                    if let Some(previous_worker_id) = previous_worker_id {
                        let reattach_token = self.inner.reattach_token.lock_or_recover().clone();
                        let reattach = Self::prepare_envelope(&Message::Reattach {
                            previous_worker_id,
                            reattach_token,
                        });
                        let send_result = match reattach {
                            Ok(envelope) => self.send_ws(&mut ws_tx, &envelope).await,
                            Err(error) => Err(error),
                        };
                        if let Err(err) = send_result {
                            tracing::warn!(error = %err, "failed to send reattach; reconnecting");
                            sleep(t.retry_delay).await;
                            continue;
                        }
                    }

                    queue.extend(self.collect_registrations());
                    Self::dedupe_registrations(&mut queue);
                    // Observers are replayed once this engine said it supports
                    // them (`WorkerRegistered`), not blindly; report frames
                    // go back in the buffer below, oldest first.
                    self.hold_observe_frames(&mut queue, &mut reports);

                    // Snapshot the registration keys we're about to send so
                    // we can drop duplicate copies still pending in `rx`.
                    // These are leftover from `register_*` calls made by user
                    // threads before the WS handshake completed: each call
                    // both inserts into the in-memory map (replayed via
                    // `collect_registrations`) AND queues into `outbound`.
                    let snapshot_ids: HashSet<String> = queue
                        .iter()
                        .filter_map(|message| message.registration_key.clone())
                        .collect();

                    if let Err(err) = self.flush_queue(&mut ws_tx, &mut queue).await {
                        tracing::warn!(error = %err, "failed to flush queue");
                        sleep(t.retry_delay).await;
                        continue;
                    }

                    // Drain pre-connect leftovers from `rx`, dropping
                    // register duplicates and preserving everything else
                    // (invocations, results, channel ops, and any
                    // registrations added after the snapshot was taken).
                    let shutdown =
                        Self::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);
                    if shutdown {
                        self.inner.running.store(false, Ordering::SeqCst);
                        self.abandon_reports(&mut queue, &mut reports, &mut rx);
                        return;
                    }
                    self.hold_observe_frames(&mut queue, &mut reports);
                    self.requeue_reports(&mut reports);

                    if !queue.is_empty() {
                        if let Err(err) = self.flush_queue(&mut ws_tx, &mut queue).await {
                            tracing::warn!(
                                error = %err,
                                "failed to flush post-drain queue"
                            );
                            sleep(t.retry_delay).await;
                            continue;
                        }
                    }

                    // Auto-register worker metadata on connect (like Node SDK)
                    self.register_worker_metadata();

                    let mut should_reconnect = false;

                    // Keepalive: pings make an idle link produce traffic, and
                    // a frameless idle window means the link is dead even if
                    // our sends still "succeed" (half-open socket: the engine
                    // has already dropped us and unregistered our functions
                    // while we still look Connected — MOT-3857).
                    let mut ping = interval(t.ping_interval);
                    let mut last_rx = Instant::now();

                    while self.inner.running.load(Ordering::SeqCst) && !should_reconnect {
                        tokio::select! {
                            outgoing = rx.recv() => {
                                match outgoing {
                                    Some(Outbound::Message(mut message)) => {
                                        if let Err(err) = self.send_ws(&mut ws_tx, &message).await {
                                            tracing::warn!(error = %err, "send failed; reconnecting");
                                            message.write_failed = true;
                                            queue.push(message);
                                            should_reconnect = true;
                                        }
                                    }
                                    Some(Outbound::Shutdown) => {
                                        self.inner.running.store(false, Ordering::SeqCst);
                                        self.abandon_reports(&mut queue, &mut reports, &mut rx);
                                        return;
                                    }
                                    None => {
                                        self.inner.running.store(false, Ordering::SeqCst);
                                        self.abandon_reports(&mut queue, &mut reports, &mut rx);
                                        return;
                                    }
                                }
                            }
                            incoming = ws_rx.next() => {
                                match incoming {
                                    Some(Ok(frame)) => {
                                        last_rx = Instant::now();
                                        if let Err(err) = self.handle_frame(frame) {
                                            tracing::warn!(error = %err, "failed to handle frame");
                                        }
                                    }
                                    Some(Err(err)) => {
                                        tracing::warn!(error = %err, "websocket receive error");
                                        should_reconnect = true;
                                    }
                                    None => {
                                        should_reconnect = true;
                                    }
                                }
                            }
                            _ = sleep_until(last_rx + t.idle_timeout) => {
                                tracing::warn!(
                                    idle_timeout = ?t.idle_timeout,
                                    "no frames from engine within idle window; forcing reconnect"
                                );
                                should_reconnect = true;
                            }
                            _ = ping.tick() => {
                                // Bounded like every send: an unbounded await
                                // here wedges the whole select loop on a full
                                // TCP window (see send_ws).
                                let ping_send = ws_tx.send(WsMessage::Ping(Default::default()));
                                match tokio::time::timeout(t.idle_timeout, ping_send).await {
                                    Ok(Ok(())) => {}
                                    Ok(Err(err)) => {
                                        tracing::warn!(error = %err, "keepalive ping failed; reconnecting");
                                        should_reconnect = true;
                                    }
                                    Err(_) => {
                                        tracing::warn!("keepalive ping timed out; reconnecting");
                                        should_reconnect = true;
                                    }
                                }
                            }
                        }
                    }

                    if should_reconnect {
                        // The link is gone now, not after the retry delay:
                        // reports wait in their buffer instead of queueing
                        // behind a dead socket, and every function waits for
                        // its confirmation on the next connection.
                        self.set_connection_state(IIIConnectionState::Reconnecting);
                        self.inner.connection_epoch.fetch_add(1, Ordering::AcqRel);
                        self.inner
                            .engine_features
                            .store(ENGINE_FEATURES_UNKNOWN, Ordering::SeqCst);
                    }
                }
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, "failed to connect; retrying");
                }
                Err(_) => {
                    tracing::warn!(
                        connect_timeout = ?t.connect_timeout,
                        "connect attempt timed out; retrying"
                    );
                }
            }

            if self.inner.running.load(Ordering::SeqCst) {
                sleep(t.retry_delay).await;
            }
        }
        // stopped (shutdown or a fatal rejection)
        self.abandon_reports(&mut queue, &mut reports, &mut rx);
    }

    /// The worker stopped: the report frames it can no longer send, and the
    /// items still buffered, are counted as `INACTIVE`, never dropped
    /// silently.
    fn abandon_reports(
        &self,
        queue: &mut Vec<PreparedEnvelope>,
        reports: &mut Vec<PreparedEnvelope>,
        rx: &mut mpsc::UnboundedReceiver<Outbound>,
    ) {
        while let Ok(outbound) = rx.try_recv() {
            if let Outbound::Message(envelope) = outbound {
                queue.push(envelope);
            }
        }
        for envelope in queue.drain(..).chain(reports.drain(..)) {
            if envelope.report_lease.is_some() {
                self.inner
                    .reporter
                    .count_frame(envelope.payload.as_str(), observe_codes::INACTIVE);
            }
        }
        self.inner.reporter.close();
    }

    fn collect_registrations(&self) -> Vec<PreparedEnvelope> {
        let mut messages = Vec::new();

        for trigger_type in self.inner.trigger_types.lock_or_recover().values() {
            messages.push(trigger_type.message.to_message());
        }

        for function in self.inner.functions.lock_or_recover().values() {
            messages.push(function.message.to_message());
        }

        for trigger in self.inner.triggers.lock_or_recover().values() {
            messages.push(trigger.to_message());
        }

        messages
            .into_iter()
            .filter_map(|message| match Self::prepare_envelope(&message) {
                Ok(envelope) => Some(envelope),
                Err(error) => {
                    tracing::warn!(error = %error, "registration replay rejected locally");
                    None
                }
            })
            .collect()
    }

    /// Returns a stable identity key for a registration message, or `None`
    /// for non-registration messages (invocations, ping/pong, etc.).
    ///
    /// Used both to deduplicate within `queue` and to detect leftover
    /// pre-connect register messages in `rx` whose state has already been
    /// re-sent via `collect_registrations()`.
    fn registration_key(message: &Message) -> Option<String> {
        match message {
            Message::RegisterTriggerType { id, .. } => Some(format!("trigger_type:{id}")),
            Message::RegisterTrigger { id, .. } => Some(format!("trigger:{id}")),
            Message::RegisterFunction { id, .. } => Some(format!("function:{id}")),
            Message::Observe { id, .. } => Some(format!("{OBSERVER_KEY_PREFIX}{id}")),
            _ => None,
        }
    }

    /// Observe messages left over from a previous connection wait for this
    /// engine's `WorkerRegistered`: sent when it advertises observe, dropped
    /// otherwise. Leftover `observe` frames are dropped now: every live
    /// observer is replayed from the map once the engine said it can take
    /// them.
    fn hold_observe_frames(
        &self,
        queue: &mut Vec<PreparedEnvelope>,
        reports: &mut Vec<PreparedEnvelope>,
    ) {
        let mut held = self.inner.held_frames.lock_or_recover();
        let mut held_bytes: usize = held.iter().map(|e| e.payload.len()).sum();
        let mut kept = Vec::with_capacity(queue.len());
        for envelope in queue.drain(..) {
            let replayed = envelope
                .registration_key
                .as_deref()
                .is_some_and(|key| key.starts_with(OBSERVER_KEY_PREFIX));
            if replayed {
                continue;
            }
            if envelope.report_lease.is_some() {
                reports.push(envelope);
            } else if envelope.observe_family {
                if held.len() < MAX_HELD_FRAMES
                    && held_bytes + envelope.payload.len() <= MAX_HELD_BYTES
                {
                    held_bytes += envelope.payload.len();
                    held.push(envelope);
                } else {
                    tracing::debug!(
                        "too many observe messages held across reconnects; dropping one"
                    );
                }
            } else {
                kept.push(envelope);
            }
        }
        *queue = kept;
    }

    /// Puts the report frames a dropped connection left behind back in the
    /// report buffer, oldest first, to wait for their functions' confirmation
    /// on the new connection. A frame whose write failed is not resent: it
    /// may have reached the engine, and reports are at-most-once. Its items
    /// count as `INACTIVE`.
    fn requeue_reports(&self, reports: &mut Vec<PreparedEnvelope>) {
        let mut frames = Vec::with_capacity(reports.len());
        for mut envelope in reports.drain(..) {
            if envelope.write_failed {
                self.inner
                    .reporter
                    .count_frame(envelope.payload.as_str(), observe_codes::INACTIVE);
            } else {
                // the items' push order, to put them back in their place
                let seqs = envelope
                    .report_lease
                    .as_mut()
                    .map(|lease| std::mem::take(&mut lease.seqs))
                    .unwrap_or_default();
                frames.push((envelope.payload.as_str().to_string(), seqs));
            }
            // the frame's lease is released here, before its items count again
        }
        self.inner.reporter.requeue_frames(
            frames
                .iter()
                .map(|(frame, seqs)| (frame.as_str(), seqs.as_slice())),
        );
    }

    /// Drain everything currently pending in the outbound `rx` channel,
    /// dropping register messages whose keys are already covered by
    /// `snapshot_ids` (already sent via `collect_registrations()`),
    /// and pushing every other message onto `queue` for re-flushing.
    ///
    /// Returns `true` if a `Shutdown` signal was observed during the
    /// drain, the caller should then stop the connection loop.
    fn drain_pre_connect_duplicates(
        rx: &mut mpsc::UnboundedReceiver<Outbound>,
        queue: &mut Vec<PreparedEnvelope>,
        snapshot_ids: &HashSet<String>,
    ) -> bool {
        loop {
            match rx.try_recv() {
                Ok(Outbound::Message(msg)) => {
                    let is_dup = msg
                        .registration_key
                        .as_ref()
                        .map(|k| snapshot_ids.contains(k))
                        .unwrap_or(false);
                    if is_dup {
                        continue;
                    }
                    queue.push(msg);
                }
                Ok(Outbound::Shutdown) => return true,
                Err(_) => return false,
            }
        }
    }

    fn dedupe_registrations(queue: &mut Vec<PreparedEnvelope>) {
        let mut seen = HashSet::new();
        let mut deduped_rev = Vec::with_capacity(queue.len());

        for message in std::mem::take(queue).into_iter().rev() {
            match &message.registration_key {
                Some(key) => {
                    if seen.insert(key.clone()) {
                        deduped_rev.push(message);
                    }
                }
                None => {
                    deduped_rev.push(message);
                }
            }
        }

        deduped_rev.reverse();
        *queue = deduped_rev;
    }

    async fn flush_queue(
        &self,
        ws_tx: &mut WsTx,
        queue: &mut Vec<PreparedEnvelope>,
    ) -> Result<(), Error> {
        let mut drained = Vec::new();
        std::mem::swap(queue, &mut drained);

        let mut iter = drained.into_iter();
        while let Some(mut message) = iter.next() {
            if let Err(err) = self.send_ws(ws_tx, &message).await {
                message.write_failed = true;
                queue.push(message);
                queue.extend(iter);
                return Err(err);
            }
        }

        Ok(())
    }

    async fn send_ws(&self, ws_tx: &mut WsTx, message: &PreparedEnvelope) -> Result<(), Error> {
        // Bound the send: on a blackholed peer with a full TCP send window,
        // send().await can block indefinitely — and since callers await this
        // inside select! handlers, an unbounded send wedges the entire
        // connection loop (idle detection included). A send that can't
        // complete within the idle window is a dead link; reconnect.
        let t = *self.inner.timings.lock_or_recover();
        match tokio::time::timeout(
            t.idle_timeout,
            ws_tx.send(WsMessage::Text(message.payload.clone())),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err.into()),
            Err(_) => {
                tracing::warn!("websocket send timed out; treating link as dead");
                Err(Error::Timeout)
            }
        }
    }

    fn handle_frame(&self, frame: WsMessage) -> Result<(), Error> {
        match frame {
            WsMessage::Text(text) => self.handle_message(&text),
            WsMessage::Binary(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                self.handle_message(&text)
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn handle_message(&self, payload: &str) -> Result<(), Error> {
        let message: Message = serde_json::from_str(payload)?;

        match message {
            Message::InvocationResult {
                invocation_id,
                result,
                error,
                ..
            } => {
                self.handle_invocation_result(invocation_id, result, error);
            }
            Message::InvokeFunction {
                invocation_id,
                function_id,
                data,
                traceparent,
                baggage,
                action: _,
                metadata,
                namespace: _,
                caller,
                observe_depth,
                activation,
            } => {
                self.handle_invoke_function(InboundInvocation {
                    invocation_id,
                    function_id,
                    data,
                    traceparent,
                    baggage,
                    metadata,
                    caller,
                    observe_depth,
                    activation,
                });
            }
            Message::RegisterTrigger {
                id,
                trigger_type,
                function_id,
                config,
                metadata,
                // Surfaced to the handler via `TriggerConfig.namespace`: a custom
                // provider that stores the config and later calls `trigger()`
                // needs it to fire the target in the right namespace.
                namespace,
                // Which of this provider's namespaces the bind belongs to. Only
                // meaningful to a provider serving more than one, and the
                // engine has already routed the message here by it.
                trigger_namespace: _,
            } => {
                self.handle_register_trigger(
                    id,
                    trigger_type,
                    function_id,
                    config,
                    metadata,
                    namespace,
                );
            }
            Message::UnregisterTrigger { id, trigger_type } => {
                self.handle_unregister_trigger(id, trigger_type);
            }
            Message::Ping => {
                let _ = self.send_message(Message::Pong);
            }
            Message::WorkerRegistered {
                worker_id,
                reattach_token,
                features,
            } => {
                tracing::debug!(worker_id = %worker_id, features = ?features, "Worker registered");
                self.update_identity(Some(worker_id.clone()), None);
                *self.inner.worker_id.lock_or_recover() = Some(worker_id);
                *self.inner.reattach_token.lock_or_recover() = reattach_token;
                self.on_engine_features(features.iter().any(|f| f == FEATURE_OBSERVE_V1));
                self.inner.registration_changed.notify_waiters();
            }
            Message::FunctionRegistered {
                id,
                namespace,
                revision,
                local_dispatch,
                observers,
            } => match self.function_entry(&id) {
                Some(entry) => entry.enqueue(
                    self,
                    HookOp::Registered {
                        revocation: entry.revocation(),
                        epoch: self.connection_epoch(),
                        namespace,
                        revision,
                        local_dispatch,
                        observers,
                    },
                ),
                None => {
                    tracing::debug!(function_id = %id, "functionregistered for a function no longer registered")
                }
            },
            Message::ObserverAdded {
                function_id,
                namespace,
                revision,
                observer,
            } => match self.function_entry(&function_id) {
                Some(entry) => entry.enqueue(
                    self,
                    HookOp::Added {
                        namespace,
                        revision,
                        observer,
                    },
                ),
                None => {
                    tracing::debug!(function_id = %function_id, "observeradded for a function no longer registered")
                }
            },
            Message::ObserverRemoved {
                function_id,
                revision,
                id,
                ..
            } => {
                if let Some(entry) = self.function_entry(&function_id) {
                    entry.enqueue(self, HookOp::Removed { revision, id });
                }
            }
            Message::ObserveResult {
                id,
                state,
                error,
                warnings,
            } => {
                if let Some(entry) = self.observer_entry(&id) {
                    entry.set_warnings(warnings);
                    self.apply_observer_state(&entry, state, error, None);
                }
            }
            Message::ObserverStateChanged {
                id,
                state,
                reason,
                fires,
            } => {
                if let Some(entry) = self.observer_entry(&id) {
                    self.apply_observer_state(&entry, state, reason, Some(fires));
                }
            }
            Message::Unknown => {
                tracing::debug!("ignoring a message type this SDK does not know");
            }
            Message::RegistrationRejected {
                code,
                namespace,
                worker_name,
                function_id,
                owner_worker_id,
            } => {
                self.handle_registration_rejected(
                    code,
                    namespace,
                    worker_name,
                    function_id,
                    owner_worker_id,
                );
            }
            Message::TriggerRegistrationResult {
                id,
                trigger_type,
                function_id: _,
                error: Some(err),
            } => {
                tracing::error!(
                    trigger_id = %id,
                    trigger_type = %trigger_type,
                    code = %err.code,
                    "[iii] Trigger registration failed for {:?}: {}",
                    id,
                    err.message
                );
            }
            _ => {}
        }

        Ok(())
    }

    /// Dispatch a `RegistrationRejected` message by its `code`.
    ///
    /// The two rejection codes carry different semantics. A worker-name
    /// conflict is fatal — the engine has closed the connection and the SDK
    /// must not reconnect into the same collision. A function-id conflict costs
    /// the worker only that one function: the connection stays open and every
    /// other export keeps serving, so it must not be treated as fatal. Any
    /// unrecognised code is treated as fatal, the safe default.
    fn handle_registration_rejected(
        &self,
        code: String,
        namespace: String,
        worker_name: Option<String>,
        function_id: Option<String>,
        owner_worker_id: String,
    ) {
        match code.as_str() {
            FUNCTION_NAMESPACE_CONFLICT => {
                tracing::warn!(
                    code = %code,
                    namespace = %namespace,
                    function_id = %function_id.as_deref().unwrap_or("<unknown>"),
                    owner_worker_id = %owner_worker_id,
                    "function registration rejected: another worker in this namespace already \
                     owns this function id; the worker keeps serving its other functions"
                );
                // Another worker owns it now: calls go through the engine to
                // that owner, and this worker's observer set is void.
                if let Some(entry) = function_id
                    .as_deref()
                    .and_then(|id| self.function_entry(id))
                {
                    entry.release();
                    self.inner.reporter.discard(&entry.id);
                    entry.enqueue(self, HookOp::Reset { last: false });
                }
            }
            WORKER_NAMESPACE_CONFLICT => {
                self.fail_registration_fatal(
                    code,
                    namespace,
                    worker_name,
                    function_id,
                    owner_worker_id,
                );
            }
            _ => {
                tracing::error!(
                    code = %code,
                    "registration rejected with an unknown code; treating as fatal"
                );
                self.fail_registration_fatal(
                    code,
                    namespace,
                    worker_name,
                    function_id,
                    owner_worker_id,
                );
            }
        }
    }

    /// Record a fatal registration rejection: surface the error, mark the
    /// connection failed, and clear the running flag so the connection loop
    /// exits instead of reconnecting into the same collision.
    fn fail_registration_fatal(
        &self,
        code: String,
        namespace: String,
        worker_name: Option<String>,
        function_id: Option<String>,
        owner_worker_id: String,
    ) {
        let err = Error::RegistrationRejected {
            code,
            namespace,
            worker_name,
            function_id,
            owner_worker_id,
        };
        tracing::error!(error = %err, "worker registration rejected; not reconnecting");
        // Fail every in-flight invocation now with the fatal error, so a
        // `trigger()` awaiting a response returns `RegistrationRejected`
        // immediately instead of sitting on its oneshot until the invocation
        // timeout elapses. Mirrors Go's `handleRegistrationRejected`.
        let drained: Vec<PendingInvocation> = self
            .inner
            .pending
            .lock_or_recover()
            .drain()
            .map(|(_, sender)| sender)
            .collect();
        for sender in drained {
            let _ = sender.send(Err(err.clone()));
        }
        *self.inner.fatal_error.lock_or_recover() = Some(err);
        self.set_connection_state(IIIConnectionState::Failed);
        self.inner.running.store(false, Ordering::SeqCst);
        self.inner.registration_changed.notify_waiters();
    }

    fn handle_invocation_result(
        &self,
        invocation_id: Uuid,
        result: Option<Value>,
        error: Option<ErrorBody>,
    ) {
        let sender = self.inner.pending.lock_or_recover().remove(&invocation_id);
        if let Some(sender) = sender {
            let result = match error {
                Some(error) => Err(Error::Remote {
                    code: error.code,
                    message: error.message,
                    stacktrace: error.stacktrace,
                }),
                None => Ok(result.unwrap_or(Value::Null)),
            };
            let _ = sender.send(result);
        }
    }

    fn handle_invoke_function(&self, inbound: InboundInvocation) {
        let InboundInvocation {
            invocation_id,
            function_id,
            data,
            traceparent,
            baggage,
            metadata,
            caller,
            observe_depth,
            activation,
        } = inbound;
        tracing::debug!(function_id = %function_id, traceparent = ?traceparent, baggage = ?baggage, "Invoking function");

        // An observer activation is answered like any call: the engine
        // discards the result of an auto or emit activation, and the answer
        // frees the observer's in-flight slot.
        let reply_to = invocation_id;

        // Preserve HTTP-only registrations without copying their descriptors.
        let function_handler = self
            .inner
            .functions
            .lock_or_recover()
            .get(&function_id)
            .map(|function| function.handler.clone());

        let Some(Some(handler)) = function_handler else {
            let (code, message) = match function_handler {
                Some(_) => (
                    "function_not_invokable".to_string(),
                    "Function is HTTP-invoked and cannot be invoked locally".to_string(),
                ),
                None => (
                    "function_not_found".to_string(),
                    "Function not found".to_string(),
                ),
            };
            tracing::warn!(function_id = %function_id, "Invocation: {}", message);

            if let Some(invocation_id) = reply_to {
                let (resp_tp, resp_bg) = inject_trace_headers();

                let error = ErrorBody {
                    code,
                    message,
                    stacktrace: None,
                };
                let result = self.send_message(Message::InvocationResult {
                    invocation_id,
                    function_id,
                    result: None,
                    error: Some(error),
                    traceparent: resp_tp,
                    baggage: resp_bg,
                });

                if let Err(err) = result {
                    tracing::warn!(error = %err, "error sending invocation result");
                }
            }
            return;
        };

        // The current invocation, for calls and emits made by the handler.
        let ctx = InvocationCtx {
            function_id: function_id.clone(),
            invocation_id: invocation_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            caller: caller.map(|caller| *caller),
            activation,
            observe_depth: observe_depth.unwrap_or(0),
            metadata: metadata.clone(),
            traceparent: None,
            baggage: None,
        };

        let iii = self.clone();

        tokio::spawn(async move {
            // Extract incoming trace context and create a span for this
            // invocation, so the handler and any outbound calls it makes are
            // linked as children of the caller's trace.
            let parent_cx = iii_helpers::observability::extract_context(
                traceparent.as_deref(),
                baggage.as_deref(),
            );
            let executed = execute(parent_cx, data, metadata, handler, ctx, 0).await;

            if let Some(invocation_id) = reply_to {
                // Inject trace context from our span into the response.
                // We briefly attach the otel context (no .await crossing)
                // so inject_traceparent/inject_baggage can read it.
                let (resp_tp, resp_bg) = {
                    let _guard = executed.otel_cx.attach();
                    inject_trace_headers()
                };

                let message = match executed.result {
                    Ok(value) => Message::InvocationResult {
                        invocation_id,
                        function_id,
                        result: Some(value),
                        error: None,
                        traceparent: resp_tp,
                        baggage: resp_bg,
                    },
                    Err(err) => Message::InvocationResult {
                        invocation_id,
                        function_id,
                        result: None,
                        error: Some(error_body(err, executed.stacktrace)),
                        traceparent: resp_tp,
                        baggage: resp_bg,
                    },
                };

                let _ = iii.send_message(message);
            } else if let Err(err) = executed.result {
                tracing::warn!(error = %err, "error handling async invocation");
            }
        });
    }

    /// The engine's features on this connection are known: replay the
    /// observers (or fail them with `UNSUPPORTED`), and say once when the
    /// engine leaves functions' observe hooks inactive.
    fn on_engine_features(&self, observe: bool) {
        self.inner.engine_features.store(
            if observe {
                ENGINE_WITH_OBSERVE
            } else {
                ENGINE_WITHOUT_OBSERVE
            },
            Ordering::SeqCst,
        );
        self.inner
            .engine_observe_last
            .store(observe, Ordering::Relaxed);
        let held = std::mem::take(&mut *self.inner.held_frames.lock_or_recover());
        let observers: Vec<Arc<ObserverEntry>> = self
            .inner
            .observers
            .lock_or_recover()
            .values()
            .cloned()
            .collect();
        if observe {
            for envelope in held {
                let _ = self.send_prepared(envelope);
            }
            for entry in observers {
                self.send_observe(&entry);
            }
        } else {
            if !held.is_empty() {
                tracing::debug!(
                    frames = held.len(),
                    "dropping observe messages queued for an engine without observe"
                );
            }
            for entry in observers {
                Self::fail_unsupported(&entry);
            }
            let inactive: Vec<String> = self
                .functions()
                .values()
                .filter(|data| data.observe.needs_observe)
                .map(|data| data.message.id.clone())
                .collect();
            self.warn_no_observe(inactive);
        }
        if observe {
            // a later engine without observe is worth saying again
            self.inner.warned_no_observe.lock_or_recover().clear();
        }
        {
            // each listener that has not heard this value yet: after a
            // change, or added while reconnecting
            let mut support = self.inner.observe_support.lock_or_recover();
            let tell: Vec<(Arc<AtomicBool>, ObserveSupportFn)> = support
                .listeners
                .iter_mut()
                .filter(|listener| listener.heard != Some(observe))
                .map(|listener| {
                    listener.heard = Some(observe);
                    (listener.active.clone(), listener.call.clone())
                })
                .collect();
            if !tell.is_empty() {
                ObserveSupport::notify(tell, observe);
            }
        }
        // Reports buffered while disconnected wait for their functions'
        // confirmation; stats and anything else due go out now.
        self.inner.reporter.wake.notify_one();
    }

    /// Warns, once per function, that the engine leaves these functions'
    /// observe hooks inactive.
    fn warn_no_observe(&self, functions: Vec<String>) {
        let fresh: Vec<String> = {
            let mut warned = self.inner.warned_no_observe.lock_or_recover();
            functions
                .into_iter()
                .filter(|id| warned.insert(id.clone()))
                .collect()
        };
        if !fresh.is_empty() {
            tracing::warn!(
                functions = ?fresh,
                "the engine does not support observe (no `observe.v1`): these functions are \
                 registered as plain functions and their observe hooks will not run"
            );
        }
    }

    fn fail_unsupported(entry: &ObserverEntry) {
        entry.set(
            ObserverState::Error,
            Some(ObserverReason {
                code: observe_codes::UNSUPPORTED.to_string(),
                message: "the engine does not support observe (no `observe.v1`)".to_string(),
            }),
            None,
        );
    }

    fn send_observe(&self, entry: &ObserverEntry) {
        let _order = entry.send_lock.lock_or_recover();
        if entry.disposing.load(Ordering::Acquire) {
            return;
        }
        entry.sent.store(true, Ordering::Release);
        if let Err(error) = self.send_message(entry.message.clone()) {
            tracing::warn!(observer_id = %entry.id, error = %error, "observe not sent");
        }
    }

    /// Starts tracking a new observer: sent now when this engine supports
    /// observe, failed when it does not, and sent on `WorkerRegistered`
    /// before that.
    pub(crate) fn track_observer(&self, entry: Arc<ObserverEntry>) {
        self.inner
            .observers
            .lock_or_recover()
            .insert(entry.id.clone(), entry.clone());
        match self.inner.engine_features.load(Ordering::SeqCst) {
            ENGINE_WITH_OBSERVE => self.send_observe(&entry),
            ENGINE_WITHOUT_OBSERVE => Self::fail_unsupported(&entry),
            _ => {}
        }
    }

    /// Stops replaying an observer.
    pub(crate) fn forget_observer(&self, id: &str) {
        self.inner.observers.lock_or_recover().remove(id);
    }

    fn observer_entry(&self, id: &str) -> Option<Arc<ObserverEntry>> {
        self.inner.observers.lock_or_recover().get(id).cloned()
    }

    fn apply_observer_state(
        &self,
        entry: &ObserverEntry,
        state: ObserverState,
        reason: Option<ErrorBody>,
        fires: Option<u64>,
    ) {
        if state == ObserverState::Disposed {
            // Disposed by the engine (policy, expiry, grace): not replayed.
            self.forget_observer(&entry.id);
        }
        entry.set(state, reason.map(ObserverReason::from), fires);
    }

    fn handle_register_trigger(
        &self,
        id: String,
        trigger_type: String,
        function_id: String,
        config: Value,
        metadata: Option<Value>,
        namespace: Option<String>,
    ) {
        let handler = self
            .inner
            .trigger_types
            .lock_or_recover()
            .get(&trigger_type)
            .map(|data| data.handler.clone());

        let iii = self.clone();

        tokio::spawn(async move {
            let message = if let Some(handler) = handler {
                let config = TriggerConfig {
                    id: id.clone(),
                    function_id: function_id.clone(),
                    config,
                    metadata,
                    namespace,
                };

                match handler.register_trigger(config).await {
                    Ok(()) => Message::TriggerRegistrationResult {
                        id,
                        trigger_type,
                        function_id,
                        error: None,
                    },
                    Err(err) => Message::TriggerRegistrationResult {
                        id,
                        trigger_type,
                        function_id,
                        error: Some(ErrorBody {
                            code: "trigger_registration_failed".to_string(),
                            message: err.to_string(),
                            stacktrace: None,
                        }),
                    },
                }
            } else {
                Message::TriggerRegistrationResult {
                    id,
                    trigger_type,
                    function_id,
                    error: Some(ErrorBody {
                        code: "trigger_type_not_found".to_string(),
                        message: "Trigger type not found".to_string(),
                        stacktrace: None,
                    }),
                }
            };

            let _ = iii.send_message(message);
        });
    }

    fn handle_unregister_trigger(&self, id: String, trigger_type: String) {
        let handler = self
            .inner
            .trigger_types
            .lock_or_recover()
            .get(&trigger_type)
            .map(|data| data.handler.clone());

        let Some(handler) = handler else {
            return;
        };

        tokio::spawn(async move {
            let config = TriggerConfig {
                id: id.clone(),
                function_id: String::new(),
                config: Value::Null,
                metadata: None,
                // Unregister carries only the trigger id; no namespace to surface.
                namespace: None,
            };

            if let Err(err) = handler.unregister_trigger(config).await {
                tracing::warn!(trigger_id = %id, error = %err, "Error unregistering trigger");
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Internal implementations for items relocated to the `helpers` submodule.
// Exposed at `pub(crate)` so the thin wrappers in `crate::helpers` can call
// them; user code must continue to go through `crate::helpers::*`.
// ---------------------------------------------------------------------------

pub(crate) async fn internal_create_channel(
    iii: &IIIClient,
    buffer_size: Option<usize>,
) -> Result<Channel, Error> {
    let result = iii
        .trigger(TriggerRequest {
            function_id: "engine::channels::create".to_string(),
            payload: serde_json::json!({ "buffer_size": buffer_size }),
            action: None,
            timeout_ms: None,
        })
        .await?;

    let writer_ref: StreamChannelRef = serde_json::from_value(
        result
            .get("writer")
            .cloned()
            .ok_or_else(|| Error::Serde("missing 'writer' in channel response".into()))?,
    )
    .map_err(|e| Error::Serde(e.to_string()))?;

    let reader_ref: StreamChannelRef = serde_json::from_value(
        result
            .get("reader")
            .cloned()
            .ok_or_else(|| Error::Serde("missing 'reader' in channel response".into()))?,
    )
    .map_err(|e| Error::Serde(e.to_string()))?;

    let headers = iii.inner.headers.lock_or_recover().clone();
    Ok(Channel {
        writer: ChannelWriter::new(&iii.inner.address, &writer_ref).with_headers(headers.clone()),
        reader: ChannelReader::new(&iii.inner.address, &reader_ref).with_headers(headers),
        writer_ref,
        reader_ref,
    })
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;

    use serde_json::json;

    use iii_helpers::http::{HttpInvocationConfig, HttpMethod};

    use super::*;
    use crate::trigger::{TriggerConfig, TriggerHandler};
    use crate::{InitOptions, protocol::RegisterTriggerInput, register_worker};

    use std::sync::atomic::AtomicUsize;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    struct ScopedEnvVar {
        key: &'static str,
        previous: Option<OsString>,
        _lock: MutexGuard<'static, ()>,
    }

    impl ScopedEnvVar {
        fn new(key: &'static str) -> Self {
            let lock = ENV_MUTEX.lock_or_recover();
            let previous = std::env::var_os(key);
            Self {
                key,
                previous,
                _lock: lock,
            }
        }

        fn set(&self, value: &str) {
            // SAFETY: this guard holds the shared test mutex until the
            // original environment value is restored.
            unsafe {
                std::env::set_var(self.key, value);
            }
        }

        fn remove(&self) {
            // SAFETY: this guard holds the shared test mutex until the
            // original environment value is restored.
            unsafe {
                std::env::remove_var(self.key);
            }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            // SAFETY: restoration happens while this guard still holds the
            // shared test mutex, including when a test unwinds after a panic.
            unsafe {
                match self.previous.take() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    /// Raw TCP listener that accepts and then holds sockets open without
    /// ever answering (or even reading) the WS handshake — the stalled /
    /// half-open link from MOT-3857.
    async fn spawn_stalled_listener() -> (std::net::SocketAddr, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((sock, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                held.push(sock);
            }
        });
        (addr, accepted)
    }

    /// WS server that completes handshakes and counts them. A `silent`
    /// server then holds the socket without polling it (no auto-pong, no
    /// frames — half-open link); a polling server reads frames, which lets
    /// tungstenite's auto-pong answer client pings.
    async fn spawn_ws_server(silent: bool) -> (std::net::SocketAddr, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handshakes = Arc::new(AtomicUsize::new(0));
        let counter = handshakes.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let counter = counter.clone();
                tokio::spawn(async move {
                    let Ok(mut ws) = tokio_tungstenite::accept_async(sock).await else {
                        return;
                    };
                    counter.fetch_add(1, Ordering::SeqCst);
                    if silent {
                        std::future::pending::<()>().await;
                    }
                    while let Some(frame) = ws.next().await {
                        if frame.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        (addr, handshakes)
    }

    fn client_with_timings(addr: std::net::SocketAddr, timings: ConnTimings) -> IIIClient {
        let iii = IIIClient::new(&format!("ws://{addr}"));
        // Keep OTel out of these tests: its exporter would dial the same
        // server and pollute the connection counters.
        *iii.inner.otel_config.lock_or_recover() = Some(OtelConfig {
            enabled: Some(false),
            ..Default::default()
        });
        *iii.inner.timings.lock_or_recover() = timings;
        iii.connect();
        iii
    }

    async fn wait_for_count(counter: &AtomicUsize, want: usize, within: Duration) -> usize {
        let deadline = Instant::now() + within;
        loop {
            let n = counter.load(Ordering::SeqCst);
            if n >= want || Instant::now() >= deadline {
                return n;
            }
            sleep(Duration::from_millis(25)).await;
        }
    }

    #[tokio::test]
    async fn connect_timeout_abandons_stalled_connect_and_retries() {
        let (addr, accepted) = spawn_stalled_listener().await;
        let iii = client_with_timings(
            addr,
            ConnTimings {
                connect_timeout: Duration::from_millis(150),
                retry_delay: Duration::from_millis(100),
                ..ConnTimings::default()
            },
        );

        // Without the connect timeout the first attempt wedges forever and
        // no second TCP connect ever happens.
        let n = wait_for_count(&accepted, 2, Duration::from_secs(5)).await;
        iii.shutdown_async().await;
        assert!(
            n >= 2,
            "stalled connect must be abandoned and retried, saw {n} attempts"
        );
    }

    #[tokio::test]
    async fn idle_timeout_reconnects_when_engine_goes_silent() {
        let (addr, handshakes) = spawn_ws_server(true).await;
        let iii = client_with_timings(
            addr,
            ConnTimings {
                connect_timeout: Duration::from_secs(5),
                // Ping never fires: idle detection must not depend on the
                // ping cadence (and a polled test server would auto-pong).
                ping_interval: Duration::from_secs(30),
                idle_timeout: Duration::from_millis(250),
                retry_delay: Duration::from_millis(100),
            },
        );

        // A half-open link produces no frames; the idle deadline must tear
        // the connection down and reconnect (replaying registrations).
        let n = wait_for_count(&handshakes, 2, Duration::from_secs(5)).await;
        iii.shutdown_async().await;
        assert!(
            n >= 2,
            "silent link must force a reconnect, saw {n} handshakes"
        );
    }

    #[tokio::test]
    async fn keepalive_pings_hold_healthy_connection_open() {
        let (addr, handshakes) = spawn_ws_server(false).await;
        let iii = client_with_timings(
            addr,
            ConnTimings {
                connect_timeout: Duration::from_secs(5),
                ping_interval: Duration::from_millis(100),
                idle_timeout: Duration::from_secs(1),
                retry_delay: Duration::from_millis(100),
            },
        );

        // Pongs elicited by our pings count as liveness: well past the idle
        // window, the original connection must still be the only one.
        sleep(Duration::from_millis(1500)).await;
        let n = handshakes.load(Ordering::SeqCst);
        iii.shutdown_async().await;
        assert_eq!(n, 1, "healthy pinged connection must not be torn down");
    }

    type Handshake = (String, tokio_tungstenite::tungstenite::http::HeaderMap);

    /// WS server that reports each handshake's request URI and headers.
    // tungstenite's handshake callback is forced to return its large error type.
    #[allow(clippy::result_large_err)]
    async fn spawn_handshake_recorder() -> (std::net::SocketAddr, mpsc::UnboundedReceiver<Handshake>)
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let record =
                        |req: &tokio_tungstenite::tungstenite::handshake::server::Request, resp| {
                            let _ = tx.send((req.uri().to_string(), req.headers().clone()));
                            Ok(resp)
                        };
                    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(sock, record).await else {
                        return;
                    };
                    while let Some(Ok(_)) = ws.next().await {}
                });
            }
        });
        (addr, rx)
    }

    async fn next_handshake(rx: &mut mpsc::UnboundedReceiver<Handshake>) -> Handshake {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("no handshake within 5s")
            .expect("recorder stopped")
    }

    fn connect_recording(
        addr: std::net::SocketAddr,
        headers: Option<HashMap<String, String>>,
    ) -> IIIClient {
        let iii = IIIClient::new(&format!("ws://{addr}"));
        // Keep OTel's exporter from dialing the recorder too.
        *iii.inner.otel_config.lock_or_recover() = Some(OtelConfig {
            enabled: Some(false),
            ..Default::default()
        });
        if let Some(headers) = headers {
            iii.set_headers(headers);
        }
        iii.connect();
        iii
    }

    fn channel_ref(direction: crate::channels::ChannelDirection) -> StreamChannelRef {
        StreamChannelRef {
            channel_id: "chan".into(),
            access_key: "key".into(),
            direction,
        }
    }

    /// `III_TOKEN` alone authenticates the main connection and both channel
    /// ends, and an explicit `Authorization` wins on each of them. Sync because
    /// the env guard is a std mutex that must not be held across an await.
    #[test]
    fn iii_token_authenticates_the_handshake_of_the_connection_and_channels() {
        use crate::channels::ChannelDirection::{Read, Write};

        let env = ScopedEnvVar::new("III_TOKEN");
        env.set("secret");
        let explicit =
            HashMap::from([("authorization".to_string(), "Bearer explicit".to_string())]);

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (addr, mut rx) = spawn_handshake_recorder().await;
            let auth = |h: &Handshake| {
                h.1.get("authorization")
                    .map(|v| v.to_str().unwrap().to_string())
            };

            let iii = connect_recording(addr, None);
            assert_eq!(
                auth(&next_handshake(&mut rx).await).as_deref(),
                Some("Bearer secret")
            );
            iii.shutdown_async().await;

            let iii = connect_recording(addr, Some(explicit.clone()));
            assert_eq!(
                auth(&next_handshake(&mut rx).await).as_deref(),
                Some("Bearer explicit")
            );
            iii.shutdown_async().await;

            // The base query survives, and the channel's own params follow it.
            let base = format!("ws://{addr}/?token=abc");
            let writer = ChannelWriter::new(&base, &channel_ref(Write));
            writer.send_message("hi").await.unwrap();
            let handshake = next_handshake(&mut rx).await;
            assert_eq!(handshake.0, "/ws/channels/chan?token=abc&key=key&dir=write");
            assert_eq!(auth(&handshake).as_deref(), Some("Bearer secret"));

            let reader = ChannelReader::new(&base, &channel_ref(Read));
            let _ = tokio::time::timeout(Duration::from_millis(500), reader.next_binary()).await;
            let handshake = next_handshake(&mut rx).await;
            assert_eq!(handshake.0, "/ws/channels/chan?token=abc&key=key&dir=read");
            assert_eq!(auth(&handshake).as_deref(), Some("Bearer secret"));

            let writer =
                ChannelWriter::new(&base, &channel_ref(Write)).with_headers(Some(explicit));
            writer.send_message("hi").await.unwrap();
            assert_eq!(
                auth(&next_handshake(&mut rx).await).as_deref(),
                Some("Bearer explicit")
            );
        });
    }

    /// Telemetry dials the engine too, so it gets the connection's handshake
    /// headers unless the caller set its own.
    #[test]
    fn telemetry_config_gets_the_handshake_headers() {
        let env = ScopedEnvVar::new("III_TOKEN");
        env.set("secret");
        let iii = IIIClient::new("ws://engine.test");
        iii.set_headers(HashMap::from([("x-other".into(), "kept".into())]));

        let headers = iii.take_otel_config().engine_ws_headers.unwrap();
        assert_eq!(headers["x-other"], "kept");
        assert_eq!(headers["Authorization"], "Bearer secret");

        let own = HashMap::from([("x-own".to_string(), "1".to_string())]);
        iii.set_otel_config(OtelConfig {
            engine_ws_headers: Some(own.clone()),
            ..Default::default()
        });
        assert_eq!(iii.take_otel_config().engine_ws_headers, Some(own));
    }

    #[test]
    fn handshake_headers_add_a_bearer_only_for_a_non_blank_token() {
        let env = ScopedEnvVar::new("III_TOKEN");
        let kept = HashMap::from([("x-other".to_string(), "kept".to_string())]);

        env.remove();
        assert_eq!(handshake_headers(None), HashMap::new());
        for blank in ["", "  \t "] {
            env.set(blank);
            assert_eq!(handshake_headers(Some(kept.clone())), kept);
        }

        env.set(" secret\n");
        let mut with_bearer = kept.clone();
        with_bearer.insert("Authorization".to_string(), "Bearer secret".to_string());
        assert_eq!(handshake_headers(Some(kept)), with_bearer);

        let explicit =
            HashMap::from([("AUTHORIZATION".to_string(), "Bearer explicit".to_string())]);
        assert_eq!(handshake_headers(Some(explicit.clone())), explicit);
    }

    struct NoopTriggerHandler;

    #[async_trait::async_trait]
    impl TriggerHandler for NoopTriggerHandler {
        async fn register_trigger(&self, _: TriggerConfig) -> Result<(), Error> {
            Ok(())
        }

        async fn unregister_trigger(&self, _: TriggerConfig) -> Result<(), Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn register_trigger_unregister_removes_entry() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let trigger = iii
            .register_trigger(RegisterTriggerInput::new(
                "demo",
                "functions.echo",
                json!({ "foo": "bar" }),
            ))
            .unwrap();

        assert_eq!(iii.inner.triggers.lock().unwrap().len(), 1);

        trigger.unregister();

        assert_eq!(iii.inner.triggers.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn typed_trigger_ref_register_trigger_keeps_legacy_two_arg_shape() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let trigger_type = iii.register_trigger_type(
            RegisterTriggerType::new("typed-trigger", "typed trigger", NoopTriggerHandler)
                .trigger_request_format::<Value>(),
        );

        let trigger = trigger_type
            .register_trigger("functions.echo", json!({ "foo": "bar" }))
            .expect("register trigger");

        assert_eq!(iii.inner.triggers.lock().unwrap().len(), 1);
        trigger.unregister();
    }

    #[tokio::test]
    async fn typed_trigger_ref_register_trigger_with_metadata_stores_metadata() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let trigger_type = iii.register_trigger_type(
            RegisterTriggerType::new("typed-trigger-meta", "typed trigger", NoopTriggerHandler)
                .trigger_request_format::<Value>(),
        );

        let metadata = json!({ "owner": "billing-team" });
        let trigger = trigger_type
            .register_trigger_with_metadata(
                "functions.echo",
                json!({ "foo": "bar" }),
                Some(metadata.clone()),
            )
            .expect("register trigger");

        {
            let triggers = iii.inner.triggers.lock().unwrap();
            let stored = triggers.values().next().expect("stored trigger");
            assert_eq!(stored.metadata, Some(metadata));
        }
        trigger.unregister();
    }

    #[test]
    fn remote_function_handler_alias_remains_single_arg() {
        // Compile-time back-compat guard: the public RemoteFunctionHandler alias keeps
        // its pre-metadata single-argument shape; the sidecar-aware form is the separate
        // RemoteFunctionHandlerWithMetadata alias.
        let legacy: crate::types::RemoteFunctionHandler =
            std::sync::Arc::new(|input| Box::pin(async move { Ok(input) }));
        let with_meta: crate::types::RemoteFunctionHandlerWithMetadata =
            std::sync::Arc::new(|input, _metadata| Box::pin(async move { Ok(input) }));
        drop((legacy, with_meta));
    }

    #[tokio::test]
    async fn register_function_with_http_config_stores_and_unregister_removes() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let config = HttpInvocationConfig {
            url: "https://example.com/invoke".to_string(),
            method: HttpMethod::Post,
            timeout_ms: Some(30000),
            headers: HashMap::new(),
            auth: None,
        };

        let func_ref = iii.register_function("external::my_lambda", RegisterFunction::http(config));

        assert_eq!(func_ref.id, "external::my_lambda");
        assert_eq!(iii.inner.functions.lock().unwrap().len(), 1);

        func_ref.unregister();

        assert_eq!(iii.inner.functions.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    #[should_panic(expected = "id is required")]
    async fn register_function_rejects_empty_id() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let config = HttpInvocationConfig {
            url: "https://example.com/invoke".to_string(),
            method: HttpMethod::Post,
            timeout_ms: None,
            headers: HashMap::new(),
            auth: None,
        };

        iii.register_function("", RegisterFunction::http(config));
    }

    #[tokio::test]
    async fn register_function_takes_id_then_builder() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let func_ref = iii.register_function(
            "test::reshaped::ordering",
            RegisterFunction::new_async(|input: Value| async move { Ok(input) })
                .description("reshaped")
                .metadata(json!({"owner": "sdk"})),
        );
        assert_eq!(func_ref.id, "test::reshaped::ordering");

        let funcs = iii.inner.functions.lock().unwrap();
        let stored = funcs.get("test::reshaped::ordering").expect("stored");
        assert_eq!(stored.message.id, "test::reshaped::ordering");
        assert_eq!(stored.message.description.as_deref(), Some("reshaped"));
        assert_eq!(stored.message.metadata, Some(json!({"owner": "sdk"})));
        assert!(stored.handler.is_some());
    }

    #[tokio::test]
    async fn register_function_metadata_builder_is_optional() {
        let clean = RegisterFunction::new_async(|input: Value| async move { Ok(input) });
        assert!(
            clean.message.metadata.is_none(),
            "metadata should be absent unless the builder method is used"
        );

        let metadata = json!({"owner": "billing-team", "priority": "high"});
        let with_metadata = RegisterFunction::new_async(|input: Value| async move { Ok(input) })
            .metadata(metadata.clone());
        assert_eq!(with_metadata.message.metadata, Some(metadata));
    }

    #[tokio::test]
    async fn register_function_http_variant_has_no_handler() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let config = HttpInvocationConfig {
            url: "https://example.com/invoke".to_string(),
            method: HttpMethod::Post,
            timeout_ms: Some(30_000),
            headers: HashMap::new(),
            auth: None,
        };

        let func_ref = iii.register_function("external::reshaped", RegisterFunction::http(config));

        assert_eq!(func_ref.id, "external::reshaped");
        let funcs = iii.inner.functions.lock().unwrap();
        let stored = funcs.get("external::reshaped").expect("stored");
        assert!(
            stored.handler.is_none(),
            "handler should be None for HTTP invocation"
        );
        assert!(
            stored.message.invocation.is_some(),
            "invocation should be set"
        );
    }

    #[tokio::test]
    async fn register_function_new_async_extracts_schemas() {
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct In {
            name: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct Out {
            message: String,
        }
        async fn greet(input: In) -> Result<Out, Error> {
            Ok(Out {
                message: format!("Hello, {}!", input.name),
            })
        }

        let reg = RegisterFunction::new_async(greet);
        assert!(reg.message.request_format.is_some());
        assert!(reg.message.response_format.is_some());
        assert_eq!(reg.message.request_format.as_ref().unwrap()["title"], "In");
        assert_eq!(
            reg.message.response_format.as_ref().unwrap()["title"],
            "Out"
        );
    }

    #[tokio::test]
    async fn register_function_request_format_setter_overrides_auto_extraction() {
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct In {
            name: String,
        }
        async fn handler(input: In) -> Result<String, Error> {
            Ok(input.name)
        }

        let custom = json!({"custom": true});
        let reg = RegisterFunction::new_async(handler).request_format(custom.clone());
        assert_eq!(reg.message.request_format.as_ref().unwrap(), &custom);
    }

    #[tokio::test]
    async fn register_function_untyped_runs_handler() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let _func_ref = iii.register_function(
            "test::untyped",
            RegisterFunction::new_async(|input: Value| async move { Ok(json!({ "echo": input })) }),
        );
        let handler = {
            let funcs = iii.inner.functions.lock().unwrap();
            let stored = funcs.get("test::untyped").expect("stored");
            stored.handler.as_ref().expect("has handler").clone()
        };
        let out = handler(json!({"name": "world"}), None).await.unwrap();
        assert_eq!(out, json!({"echo": {"name": "world"}}));
    }

    #[tokio::test]
    async fn new_async_delivers_metadata_as_second_arg() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        iii.register_function(
            "test::with_meta",
            RegisterFunction::new_async(|input: Value, metadata: Option<Value>| async move {
                // Echo back both the payload and the metadata sidecar so the
                // test can prove they arrive as distinct arguments.
                Ok(json!({ "input": input, "metadata": metadata }))
            }),
        );
        let handler = {
            let funcs = iii.inner.functions.lock().unwrap();
            funcs
                .get("test::with_meta")
                .expect("stored")
                .handler
                .as_ref()
                .expect("has handler")
                .clone()
        };

        // Metadata present: delivered as the second argument, payload untouched.
        let out = handler(json!({ "x": 1 }), Some(json!({ "session_id": "s" })))
            .await
            .unwrap();
        assert_eq!(out["input"], json!({ "x": 1 }));
        assert_eq!(out["metadata"], json!({ "session_id": "s" }));

        // Metadata absent: handler still runs, sees null.
        let out_none = handler(json!({ "x": 2 }), None).await.unwrap();
        assert_eq!(out_none["metadata"], Value::Null);
    }

    #[tokio::test]
    async fn trigger_request_metadata_is_sent_by_trigger() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let _ = iii
            .trigger(
                TriggerRequest {
                    function_id: "svc::work".to_string(),
                    payload: json!({ "x": 1 }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                }
                .metadata(json!({ "tenant": "acme" })),
            )
            .await
            .expect("void trigger should enqueue");

        let mut rx = iii.inner.receiver.lock().unwrap().take().expect("receiver");
        let sent = rx.try_recv().expect("sent invoke");
        match sent {
            Outbound::Message(envelope) => {
                let wire: Value = serde_json::from_str(envelope.payload.as_str()).unwrap();
                assert_eq!(wire["type"], "invokefunction");
                assert_eq!(wire["function_id"], "svc::work");
                assert_eq!(wire["data"], json!({ "x": 1 }));
                assert_eq!(wire["metadata"], json!({ "tenant": "acme" }));
                assert_eq!(wire["action"]["type"], "void");
            }
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[tokio::test]
    async fn trigger_request_without_metadata_keeps_legacy_struct_literal_shape() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let _ = iii
            .trigger(TriggerRequest {
                function_id: "svc::work".to_string(),
                payload: json!({ "x": 1 }),
                action: Some(TriggerAction::Void),
                timeout_ms: None,
            })
            .await
            .expect("void trigger should enqueue");

        let mut rx = iii.inner.receiver.lock().unwrap().take().expect("receiver");
        let sent = rx.try_recv().expect("sent invoke");
        match sent {
            Outbound::Message(envelope) => {
                let wire: Value = serde_json::from_str(envelope.payload.as_str()).unwrap();
                assert_eq!(wire["type"], "invokefunction");
                assert_eq!(wire["function_id"], "svc::work");
                assert!(wire.get("metadata").is_none());
            }
            _ => panic!("expected invoke"),
        }
    }

    #[tokio::test]
    async fn invoke_function_times_out_and_clears_pending() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let result = iii
            .trigger(TriggerRequest {
                function_id: "functions.echo".to_string(),
                payload: json!({ "a": 1 }),
                action: None,
                timeout_ms: Some(10),
            })
            .await;

        assert!(matches!(result, Err(Error::Timeout)));
        assert!(iii.inner.pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn fatal_registration_rejection_fails_pending_invocations() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let id = uuid::Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        iii.inner.pending.lock().unwrap().insert(id, tx);

        iii.fail_registration_fatal(
            "WORKER_NAMESPACE_CONFLICT".to_string(),
            "orders".to_string(),
            Some("state".to_string()),
            None,
            "owner-1".to_string(),
        );

        // The in-flight invocation is failed fast with the typed error instead
        // of sitting on its receiver until the invocation timeout.
        match rx.await {
            Ok(Err(Error::RegistrationRejected { code, .. })) => {
                assert_eq!(code, "WORKER_NAMESPACE_CONFLICT");
            }
            other => panic!("expected RegistrationRejected, got {other:?}"),
        }
        assert!(iii.inner.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn worker_metadata_default_reads_iii_isolation_env_var() {
        let env = ScopedEnvVar::new("III_ISOLATION");

        env.remove();
        assert!(WorkerMetadata::default().isolation.is_none());

        env.set("docker");
        assert_eq!(
            WorkerMetadata::default().isolation.as_deref(),
            Some("docker")
        );
    }

    #[test]
    fn worker_name_resolution_prefers_iii_worker_name_env_var() {
        let env = ScopedEnvVar::new("III_WORKER_NAME");

        env.remove();
        let fallback = WorkerMetadata::default().name;
        assert!(
            fallback.ends_with(&format!(":{}", std::process::id())),
            "expected hostname:pid fallback, got {fallback}"
        );

        env.set("");
        let metadata = WorkerMetadata {
            name: "explicit-worker".to_string(),
            ..WorkerMetadata::default()
        };
        let iii = IIIClient::with_metadata("ws://127.0.0.1:0", metadata);
        let explicit_name = iii
            .inner
            .worker_metadata
            .lock_or_recover()
            .as_ref()
            .unwrap()
            .name
            .clone();
        assert_eq!(explicit_name, "explicit-worker");

        env.set("managed-worker");
        assert_ne!(WorkerMetadata::default().name, "managed-worker");

        let metadata = WorkerMetadata {
            name: "explicit-worker".to_string(),
            ..WorkerMetadata::default()
        };
        let iii = IIIClient::with_metadata("ws://127.0.0.1:0", metadata);
        let resolved_name = iii
            .inner
            .worker_metadata
            .lock_or_recover()
            .as_ref()
            .unwrap()
            .name
            .clone();
        assert_eq!(resolved_name, "managed-worker");

        let replacement = WorkerMetadata {
            name: "replacement-worker".to_string(),
            ..WorkerMetadata::default()
        };
        iii.set_metadata(replacement);
        let replaced_name = iii
            .inner
            .worker_metadata
            .lock_or_recover()
            .as_ref()
            .unwrap()
            .name
            .clone();
        assert_eq!(replaced_name, "managed-worker");
    }

    #[test]
    fn explicit_identity_preserves_metadata_name() {
        let env = ScopedEnvVar::new("III_WORKER_NAME");
        env.set("managed-worker");

        let metadata = WorkerMetadata {
            name: "remote-bridge".to_string(),
            ..WorkerMetadata::default()
        };
        let iii =
            IIIClient::with_identity("ws://127.0.0.1:0", metadata, WorkerIdentityMode::Explicit);
        let resolved_name = iii
            .inner
            .worker_metadata
            .lock_or_recover()
            .as_ref()
            .unwrap()
            .name
            .clone();

        assert_eq!(resolved_name, "remote-bridge");
    }

    #[test]
    fn explicit_identity_preserves_metadata_namespace() {
        let env = ScopedEnvVar::new("III_NAMESPACE");
        env.set("managed-namespace");

        let metadata = WorkerMetadata {
            namespace: Some("remote-namespace".to_string()),
            ..WorkerMetadata::default()
        };
        let iii =
            IIIClient::with_identity("ws://127.0.0.1:0", metadata, WorkerIdentityMode::Explicit);

        assert_eq!(iii.namespace().as_deref(), Some("remote-namespace"));
    }

    #[test]
    fn managed_identity_ignores_whitespace_only_namespace() {
        let env = ScopedEnvVar::new("III_NAMESPACE");
        env.set(" \t ");

        let iii = IIIClient::with_metadata("ws://127.0.0.1:0", WorkerMetadata::default());

        assert!(iii.namespace().is_none());
    }

    #[test]
    fn parse_cargo_package_name_extracts_name_field() {
        let toml = "[package]\nname = \"my-crate\"\nversion = \"1.0.0\"\n";
        assert_eq!(parse_cargo_package_name(toml), Some("my-crate".to_string()));
    }

    #[test]
    fn parse_cargo_package_name_ignores_other_tables() {
        let toml = "[dependencies]\nname = \"not-the-package\"\n[package]\nname = \"the-pkg\"\n";
        assert_eq!(parse_cargo_package_name(toml), Some("the-pkg".to_string()));
    }

    #[test]
    fn parse_cargo_package_name_returns_none_when_missing() {
        let toml = "[package]\nversion = \"1.0.0\"\n";
        assert_eq!(parse_cargo_package_name(toml), None);
    }

    #[test]
    fn parse_cargo_package_name_returns_none_when_blank() {
        let toml = "[package]\nname = \"\"\n";
        assert_eq!(parse_cargo_package_name(toml), None);
    }

    #[test]
    fn detect_project_name_reads_cargo_toml_in_cwd() {
        let tmp = std::env::temp_dir().join(format!("iii-rust-detect-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"detected-crate\"\n",
        )
        .unwrap();

        assert_eq!(
            detect_project_name(Some(tmp.clone())),
            Some("detected-crate".to_string())
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn detect_project_name_falls_back_to_dir_basename_without_cargo_toml() {
        let tmp = std::env::temp_dir().join(format!("iii-rust-fallback-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        let basename = tmp.file_name().unwrap().to_str().unwrap().to_string();
        assert_eq!(detect_project_name(Some(tmp.clone())), Some(basename));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn detect_project_name_falls_back_to_dir_basename_when_cargo_toml_lacks_name() {
        let tmp =
            std::env::temp_dir().join(format!("iii-rust-fallback-noname-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("Cargo.toml"), "[package]\nversion = \"1.0.0\"\n").unwrap();

        let basename = tmp.file_name().unwrap().to_str().unwrap().to_string();
        assert_eq!(detect_project_name(Some(tmp.clone())), Some(basename));

        std::fs::remove_dir_all(&tmp).ok();
    }

    fn make_register_function(id: &str) -> Message {
        Message::RegisterFunction {
            id: id.to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
            observe: None,
        }
    }

    fn make_register_trigger(id: &str) -> Message {
        Message::RegisterTrigger {
            id: id.to_string(),
            trigger_type: "demo".to_string(),
            function_id: "fn".to_string(),
            config: json!({}),
            metadata: None,
            namespace: None,
            trigger_namespace: None,
        }
    }

    fn make_register_trigger_type(id: &str) -> Message {
        Message::RegisterTriggerType {
            id: id.to_string(),
            description: "tt".to_string(),
            trigger_request_format: None,
            call_request_format: None,
            namespace: None,
        }
    }

    fn make_invoke(function_id: &str) -> Message {
        Message::InvokeFunction {
            invocation_id: None,
            function_id: function_id.to_string(),
            data: json!({}),
            traceparent: None,
            baggage: None,
            action: None,
            metadata: None,
            namespace: None,
            caller: None,
            observe_depth: None,
            activation: None,
        }
    }

    #[test]
    fn registration_key_returns_typed_keys_for_register_messages() {
        assert_eq!(
            IIIClient::registration_key(&make_register_function("greet")),
            Some("function:greet".to_string())
        );
        assert_eq!(
            IIIClient::registration_key(&make_register_trigger("t1")),
            Some("trigger:t1".to_string())
        );
        assert_eq!(
            IIIClient::registration_key(&make_register_trigger_type("tt1")),
            Some("trigger_type:tt1".to_string())
        );
    }

    #[test]
    fn registration_key_returns_none_for_non_register_messages() {
        assert_eq!(IIIClient::registration_key(&make_invoke("f")), None);
        assert_eq!(IIIClient::registration_key(&Message::Ping), None);
        assert_eq!(IIIClient::registration_key(&Message::Pong), None);
        assert_eq!(
            IIIClient::registration_key(&Message::WorkerRegistered {
                worker_id: "w".to_string(),
                reattach_token: None,
                features: Vec::new(),
            }),
            None
        );
    }

    #[tokio::test]
    async fn drain_pre_connect_duplicates_drops_only_known_register_ids() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Outbound>();

        tx.send(Outbound::Message(
            IIIClient::prepare_envelope(&make_register_function("dup-fn")).unwrap(),
        ))
        .unwrap();
        tx.send(Outbound::Message(
            IIIClient::prepare_envelope(&make_invoke("some::fn")).unwrap(),
        ))
        .unwrap();
        tx.send(Outbound::Message(
            IIIClient::prepare_envelope(&make_register_function("new-fn")).unwrap(),
        ))
        .unwrap();
        tx.send(Outbound::Message(
            IIIClient::prepare_envelope(&Message::Pong).unwrap(),
        ))
        .unwrap();
        tx.send(Outbound::Message(
            IIIClient::prepare_envelope(&make_register_trigger("dup-trig")).unwrap(),
        ))
        .unwrap();
        tx.send(Outbound::Message(
            IIIClient::prepare_envelope(&make_register_trigger("new-trig")).unwrap(),
        ))
        .unwrap();

        let snapshot_ids: HashSet<String> = [
            "function:dup-fn".to_string(),
            "trigger:dup-trig".to_string(),
        ]
        .into_iter()
        .collect();

        let mut queue: Vec<PreparedEnvelope> = Vec::new();
        let shutdown = IIIClient::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);

        assert!(!shutdown);
        let kept_keys: Vec<Option<String>> = queue
            .iter()
            .map(|message| message.registration_key.clone())
            .collect();
        assert_eq!(
            kept_keys,
            vec![
                None,
                Some("function:new-fn".to_string()),
                None,
                Some("trigger:new-trig".to_string()),
            ],
            "kept queue mismatch: {queue:#?}"
        );
    }

    #[tokio::test]
    async fn drain_pre_connect_duplicates_signals_shutdown() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Outbound>();

        tx.send(Outbound::Message(
            IIIClient::prepare_envelope(&make_register_function("a")).unwrap(),
        ))
        .unwrap();
        tx.send(Outbound::Shutdown).unwrap();
        tx.send(Outbound::Message(
            IIIClient::prepare_envelope(&make_register_function("b")).unwrap(),
        ))
        .unwrap();

        let snapshot_ids: HashSet<String> = ["function:a".to_string()].into_iter().collect();
        let mut queue: Vec<PreparedEnvelope> = Vec::new();
        let shutdown = IIIClient::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);

        assert!(shutdown, "expected shutdown signal to be reported");
        assert!(
            queue.is_empty(),
            "queue must be empty when shutdown short-circuits the drain: {queue:#?}"
        );
    }

    #[tokio::test]
    async fn drain_pre_connect_duplicates_returns_false_on_empty_channel() {
        let (_tx, mut rx) = mpsc::unbounded_channel::<Outbound>();
        let snapshot_ids: HashSet<String> = HashSet::new();
        let mut queue: Vec<PreparedEnvelope> = Vec::new();

        let shutdown = IIIClient::drain_pre_connect_duplicates(&mut rx, &mut queue, &snapshot_ids);

        assert!(!shutdown);
        assert!(queue.is_empty());
    }

    fn hello(iii: &IIIClient, observe: bool) {
        let features: Vec<&str> = if observe { vec!["observe.v1"] } else { vec![] };
        let message = serde_json::json!({ "type": "workerregistered", "worker_id": "w",
            "features": features });
        iii.handle_message(&message.to_string()).unwrap();
    }

    /// What `run_connection` does on a reconnect.
    fn reconnecting(iii: &IIIClient) {
        iii.inner
            .engine_features
            .store(ENGINE_FEATURES_UNKNOWN, Ordering::SeqCst);
    }

    type Heard = Arc<Mutex<Vec<bool>>>;

    fn listen(iii: &IIIClient) -> (Heard, ObserveSupportListener) {
        let heard: Heard = Arc::default();
        let sink = heard.clone();
        let listener =
            iii.on_engine_supports_observe(move |observe| sink.lock().unwrap().push(observe));
        (heard, listener)
    }

    fn wait_for(heard: &Heard, expected: Vec<bool>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while heard.lock().unwrap().len() < expected.len() {
            assert!(
                Instant::now() < deadline,
                "heard {:?}",
                heard.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(*heard.lock().unwrap(), expected);
    }

    /// The engine's observe support: unknown until its `WorkerRegistered`,
    /// and listeners hear each change, from the current value on.
    #[test]
    fn the_engines_observe_support_is_known_and_followed() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        assert_eq!(iii.engine_supports_observe(), None);
        let _boom = iii.on_engine_supports_observe(|_| panic!("listener bug"));
        let (early, early_listener) = listen(&iii);
        hello(&iii, true);
        assert_eq!(iii.engine_supports_observe(), Some(true));
        wait_for(&early, vec![true]);

        // added while reconnecting: told nothing until the engine says
        reconnecting(&iii);
        assert_eq!(iii.engine_supports_observe(), None);
        let (during, _during) = listen(&iii);
        hello(&iii, false);
        assert_eq!(iii.engine_supports_observe(), Some(false));
        wait_for(&during, vec![false]);
        wait_for(&early, vec![true, false]);

        // and told even when the engine says the same as before
        reconnecting(&iii);
        let (same, _same) = listen(&iii);
        hello(&iii, false);
        wait_for(&same, vec![false]);

        // a listener added now hears the current value first
        let (late, _late) = listen(&iii);
        wait_for(&late, vec![false]);
        early_listener.unsubscribe();
        hello(&iii, true);
        wait_for(&late, vec![false, true]);
        wait_for(&during, vec![false, true]);
        assert_eq!(*early.lock().unwrap(), vec![true, false]);
        assert_eq!(*same.lock().unwrap(), vec![false, true]);
    }

    /// A change already queued when the listener unsubscribes does not call it.
    #[test]
    fn an_unsubscribed_listener_misses_a_queued_change() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (started_tx, release_rx) = (Mutex::new(started_tx), Mutex::new(release_rx));
        // holds the listener thread while the next listener's call waits
        let _blocker = iii.on_engine_supports_observe(move |_| {
            let _ = started_tx.lock().unwrap().send(());
            let _ = release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5));
        });
        let (heard, listener) = listen(&iii);
        hello(&iii, true);
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        listener.unsubscribe();
        release_tx.send(()).unwrap();
        // runs after the queued change
        let (after, _after) = listen(&iii);
        wait_for(&after, vec![true]);
        assert!(
            heard.lock().unwrap().is_empty(),
            "heard {:?}",
            heard.lock().unwrap()
        );
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn trigger_registration_result_error_is_logged() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let payload = serde_json::json!({
            "type": "triggerregistrationresult",
            "id": "trig-1",
            "trigger_type": "http",
            "function_id": "fn-1",
            "error": {
                "code": "trigger_type_not_found",
                "message": "Trigger type \"http\" not found — worker http is missing. Run: iii trigger -n <compose-daemon-namespace> compose::add worker=http",
            },
        })
        .to_string();

        iii.handle_message(&payload).unwrap();

        assert!(logs_contain("<compose-daemon-namespace>"));
        assert!(logs_contain("compose::add worker=http"));
        assert!(logs_contain("trig-1"));
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn trigger_registration_result_success_does_not_log_error() {
        let iii = register_worker("ws://localhost:1234", InitOptions::default());
        let payload = serde_json::json!({
            "type": "triggerregistrationresult",
            "id": "trig-2",
            "trigger_type": "http",
            "function_id": "fn-2",
        })
        .to_string();

        iii.handle_message(&payload).unwrap();

        assert!(!logs_contain("Trigger registration failed"));
    }

    #[test]
    fn namespace_resolution_reads_env_and_prefers_explicit_option() {
        let env = ScopedEnvVar::new("III_NAMESPACE");

        env.remove();
        // Absent everywhere -> None (engine applies its default namespace).
        assert!(WorkerMetadata::default().namespace.is_none());
        assert!(resolve_namespace(None).is_none());
        // Explicit option still wins with no env set.
        assert_eq!(
            resolve_namespace(Some("payments".into())).as_deref(),
            Some("payments")
        );

        env.set("orders");
        // Raw metadata has no process-wide identity until a managed client is
        // created from it.
        assert!(WorkerMetadata::default().namespace.is_none());
        // Env is the fallback when no explicit option is given.
        assert_eq!(resolve_namespace(None).as_deref(), Some("orders"));
        // options.namespace beats the env var.
        assert_eq!(
            resolve_namespace(Some("payments".into())).as_deref(),
            Some("payments")
        );
    }

    #[tokio::test]
    async fn trigger_request_namespace_is_sent_by_trigger() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let _ = iii
            .trigger(
                TriggerRequest {
                    function_id: "svc::work".to_string(),
                    payload: json!({ "x": 1 }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                }
                .namespace("orders"),
            )
            .await
            .expect("void trigger should enqueue");

        let mut rx = iii.inner.receiver.lock().unwrap().take().expect("receiver");
        let sent = rx.try_recv().expect("sent invoke");
        match sent {
            Outbound::Message(envelope) => {
                let wire = envelope.payload.as_str();
                let parsed: Value = serde_json::from_str(wire).unwrap();
                assert_eq!(parsed["type"], "invokefunction");
                assert_eq!(parsed["namespace"], "orders");
                assert!(wire.contains(r#""namespace":"orders""#), "wire: {wire}");
            }
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[tokio::test]
    async fn trigger_request_without_namespace_omits_it_on_the_wire() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let _ = iii
            .trigger(TriggerRequest {
                function_id: "svc::work".to_string(),
                payload: json!({ "x": 1 }),
                action: Some(TriggerAction::Void),
                timeout_ms: None,
            })
            .await
            .expect("void trigger should enqueue");

        let mut rx = iii.inner.receiver.lock().unwrap().take().expect("receiver");
        let sent = rx.try_recv().expect("sent invoke");
        match sent {
            Outbound::Message(envelope) => {
                let wire = envelope.payload.as_str();
                let parsed: Value = serde_json::from_str(wire).unwrap();
                assert_eq!(parsed["type"], "invokefunction");
                assert!(parsed.get("namespace").is_none());
                assert!(!wire.contains("namespace"), "wire: {wire}");
            }
            _ => panic!("expected InvokeFunction"),
        }
    }

    #[test]
    fn worker_name_conflict_is_fatal_and_stops_worker() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let payload = json!({
            "type": "registrationrejected",
            "code": "WORKER_NAMESPACE_CONFLICT",
            "namespace": "orders",
            "worker_name": "checkout",
            "owner_worker_id": "worker-abc",
        })
        .to_string();

        iii.handle_message(&payload).unwrap();

        // Fatal: the connection loop must not reconnect.
        assert!(
            !iii.inner.running.load(std::sync::atomic::Ordering::SeqCst),
            "worker must stop; running flag should be cleared"
        );
        assert_eq!(iii.get_connection_state(), IIIConnectionState::Failed);

        let err = iii.fatal_error().expect("fatal error must surface");
        match err {
            Error::RegistrationRejected {
                code,
                namespace,
                worker_name,
                function_id,
                owner_worker_id,
            } => {
                assert_eq!(code, "WORKER_NAMESPACE_CONFLICT");
                assert_eq!(namespace, "orders");
                assert_eq!(worker_name.as_deref(), Some("checkout"));
                assert!(function_id.is_none());
                assert_eq!(owner_worker_id, "worker-abc");
            }
            other => panic!("expected RegistrationRejected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn wait_until_registered_completes_after_engine_accepts_worker() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner.started.store(true, Ordering::SeqCst);
        iii.inner.running.store(true, Ordering::SeqCst);
        let waiter = {
            let iii = iii.clone();
            tokio::spawn(async move { iii.wait_until_registered(Duration::from_secs(1)).await })
        };

        tokio::task::yield_now().await;
        let payload = json!({
            "type": "workerregistered",
            "worker_id": "worker-accepted",
            "reattach_token": "secret",
        })
        .to_string();
        iii.handle_message(&payload).unwrap();

        assert!(waiter.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn wait_until_registered_returns_registration_rejection() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner.started.store(true, Ordering::SeqCst);
        iii.inner.running.store(true, Ordering::SeqCst);
        let payload = json!({
            "type": "registrationrejected",
            "code": "WORKER_NAMESPACE_CONFLICT",
            "namespace": "orders",
            "worker_name": "checkout",
            "owner_worker_id": "worker-abc",
        })
        .to_string();
        iii.handle_message(&payload).unwrap();

        let result = iii.wait_until_registered(Duration::from_secs(1)).await;
        assert!(matches!(result, Err(Error::RegistrationRejected { .. })));
    }

    #[test]
    fn function_conflict_is_not_fatal_and_keeps_serving() {
        let iii = IIIClient::new("ws://localhost:1234");
        iii.inner
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        // Simulate a live, connected worker.
        iii.set_connection_state(IIIConnectionState::Connected);

        // The engine refused a single duplicate function id but deliberately
        // kept the connection open.
        let payload = json!({
            "type": "registrationrejected",
            "code": "FUNCTION_NAMESPACE_CONFLICT",
            "namespace": "orders",
            "function_id": "orders::charge",
            "owner_worker_id": "worker-abc",
        })
        .to_string();

        iii.handle_message(&payload).unwrap();

        // Non-fatal: the worker keeps running, stays connected, and no fatal
        // error is recorded — its other functions are still served.
        assert!(
            iii.inner.running.load(std::sync::atomic::Ordering::SeqCst),
            "worker must keep running after a function-id conflict"
        );
        assert_eq!(iii.get_connection_state(), IIIConnectionState::Connected);
        assert!(
            iii.fatal_error().is_none(),
            "a function-id conflict must not be fatal"
        );
    }

    #[test]
    fn function_info_captures_namespace_and_tolerates_absence() {
        // Engine listings now carry `namespace`; the typed struct exposes it.
        let with_ns: FunctionInfo = serde_json::from_value(json!({
            "function_id": "svc::work",
            "description": null,
            "request_format": null,
            "response_format": null,
            "metadata": null,
            "namespace": "orders",
        }))
        .unwrap();
        assert_eq!(with_ns.namespace.as_deref(), Some("orders"));

        // Legacy payloads without the field still deserialize.
        let without_ns: FunctionInfo = serde_json::from_value(json!({
            "function_id": "svc::work",
            "description": null,
            "request_format": null,
            "response_format": null,
            "metadata": null,
        }))
        .unwrap();
        assert!(without_ns.namespace.is_none());
    }
}

#[cfg(test)]
mod nodelay_tests {
    use super::*;
    use crate::transport::test_engine;

    /// Connects through `connect_socket` to a stand-in engine and reports
    /// whether the client socket ended up with `TCP_NODELAY`.
    async fn client_nodelay(headers: Option<HashMap<String, String>>, advertise: bool) -> bool {
        let (address, server) = test_engine::spawn(advertise).await;
        let client = IIIClient::new(&format!("ws://{address}"));
        if let Some(headers) = headers {
            client.set_headers(headers);
        }
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), client.connect_socket())
            .await
            .unwrap()
            .unwrap();
        let nodelay = test_engine::nodelay(&stream);
        drop(server.await.unwrap());
        nodelay
    }

    #[tokio::test]
    async fn worker_socket_has_tcp_nodelay_when_engine_advertises_it() {
        assert!(client_nodelay(None, true).await);
    }

    #[tokio::test]
    async fn worker_socket_with_headers_has_tcp_nodelay_when_engine_advertises_it() {
        let headers = HashMap::from([("x-test".into(), "nodelay".into())]);
        assert!(client_nodelay(Some(headers), true).await);
    }

    #[tokio::test]
    async fn worker_socket_keeps_nagle_when_engine_does_not_advertise_it() {
        assert!(!client_nodelay(None, false).await);
    }
}

#[cfg(test)]
mod held_frames_tests {
    use super::*;
    use crate::reporter::{Gate, PendingItem, ReportFrame};

    /// A flushed frame on its way to the socket, as `flush_reports` sends it.
    fn envelope(frame: ReportFrame) -> PreparedEnvelope {
        PreparedEnvelope::from_json(frame.json, None, None)
            .observe_family()
            .with_report_lease(frame.lease)
    }

    /// `items` items of `function_id` pushed and flushed into one frame.
    fn report_frame(iii: &IIIClient, function_id: &str, items: usize) -> PreparedEnvelope {
        for _ in 0..items {
            assert!(iii.reporter().push(PendingItem::test(function_id)));
        }
        let mut frames = iii.reporter().take_report_frames(|_| Gate::Send);
        assert_eq!(frames.len(), 1);
        envelope(frames.pop().unwrap())
    }

    /// The steps of a reconnect in `run_connection` for what the dropped
    /// connection left in its queue.
    fn reconnect(iii: &IIIClient, mut queue: Vec<PreparedEnvelope>) {
        let mut reports = Vec::new();
        iii.hold_observe_frames(&mut queue, &mut reports);
        iii.requeue_reports(&mut reports);
    }

    /// The function of each item flushed now, in order.
    fn flushed_functions(iii: &IIIClient) -> Vec<String> {
        iii.reporter()
            .take_frames(|_| Gate::Send)
            .iter()
            .flat_map(|frame| {
                serde_json::from_str::<Value>(frame).unwrap()["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| i["function_id"].as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn observe_messages_held_across_a_reconnect_are_capped() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        let unobserve = |i: usize| {
            IIIClient::prepare_envelope(&Message::Unobserve {
                id: format!("o{i}"),
            })
            .unwrap()
        };
        let mut queue: Vec<PreparedEnvelope> = (0..=MAX_HELD_FRAMES).map(unobserve).collect();
        queue.push(report_frame(&iii, "f", 3));
        queue.push(IIIClient::prepare_envelope(&Message::Pong).unwrap());
        let mut reports = Vec::new();
        iii.hold_observe_frames(&mut queue, &mut reports);
        iii.requeue_reports(&mut reports);
        assert_eq!(queue.len(), 1, "other messages stay queued");
        assert_eq!(
            iii.inner.held_frames.lock_or_recover().len(),
            MAX_HELD_FRAMES
        );
        assert_eq!(iii.reporter().len(), 3, "reports are buffered items again");
        assert_eq!(iii.reporter().in_flight(), 0, "their frames released");
    }

    #[test]
    fn observe_messages_held_across_a_reconnect_are_capped_by_bytes() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        let big = |i: usize| {
            IIIClient::prepare_envelope(&Message::ObserverAck {
                function_id: "f".into(),
                namespace: None,
                id: format!("o{i}"),
                revision: 1,
                error: Some(ErrorBody::new("X", "y".repeat(1024 * 1024))),
            })
            .unwrap()
        };
        let mut queue: Vec<PreparedEnvelope> = (0..6).map(big).collect();
        let mut reports = Vec::new();
        iii.hold_observe_frames(&mut queue, &mut reports);
        let held = iii.inner.held_frames.lock_or_recover();
        assert!(held.len() < 6, "{} held", held.len());
        let bytes: usize = held.iter().map(|e| e.payload.len()).sum();
        assert!(bytes <= MAX_HELD_BYTES);
    }

    /// A worker that stops counts the report frames it can no longer send,
    /// and the items still buffered or parked.
    #[test]
    fn report_frames_abandoned_at_shutdown_are_counted() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(Outbound::Message(report_frame(&iii, "in-channel", 2)))
            .unwrap();
        let mut queue = vec![report_frame(&iii, "queued", 1)];
        let mut reports = vec![report_frame(&iii, "requeue", 1)];
        let late = report_frame(&iii, "late", 1);
        iii.reporter().push(PendingItem::test("parked"));
        assert!(iii.reporter().take_frames(|_| Gate::Hold).is_empty());
        iii.reporter().push(PendingItem::test("buffered"));
        iii.abandon_reports(&mut queue, &mut reports, &mut rx);
        // not running: a frame handed to the send path is counted too
        iii.send_prepared(late).unwrap();
        let mut drops = iii.reporter().take_drops();
        drops.sort();
        assert_eq!(
            drops,
            vec![
                ("buffered".to_string(), "INACTIVE", 1),
                ("in-channel".to_string(), "INACTIVE", 2),
                ("late".to_string(), "INACTIVE", 1),
                ("parked".to_string(), "INACTIVE", 1),
                ("queued".to_string(), "INACTIVE", 1),
                ("requeue".to_string(), "INACTIVE", 1),
            ]
        );
        assert_eq!(iii.reporter().in_flight(), 0);
        assert_eq!(iii.reporter().len(), 0);
    }

    /// A drop leaves one frame whose write failed in the queue and two
    /// unsent ones in the channel: the unsent ones come back in order, the
    /// failed one is not resent (it may have arrived) and is counted.
    #[test]
    fn frames_left_by_a_drop_come_back_oldest_first() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut failed = report_frame(&iii, "f0", 1);
        failed.write_failed = true;
        let mut queue = vec![failed, report_frame(&iii, "f1", 1)];
        tx.send(Outbound::Message(report_frame(&iii, "f2", 1)))
            .unwrap();
        tx.send(Outbound::Message(report_frame(&iii, "f3", 1)))
            .unwrap();
        // pushed while the connection was down
        iii.reporter().push(PendingItem::test("newest"));

        // the same steps as a reconnect in `run_connection`
        let mut reports = Vec::new();
        iii.hold_observe_frames(&mut queue, &mut reports);
        IIIClient::drain_pre_connect_duplicates(&mut rx, &mut queue, &HashSet::new());
        iii.hold_observe_frames(&mut queue, &mut reports);
        iii.requeue_reports(&mut reports);

        assert_eq!(flushed_functions(&iii), vec!["f1", "f2", "f3", "newest"]);
        assert_eq!(
            iii.reporter().take_drops(),
            vec![("f0".to_string(), "INACTIVE", 1)]
        );
    }

    /// An item of `f` read back after a first drop waits for `f`'s
    /// confirmation while a newer item of `g` goes out on the next
    /// connection, which drops too: once `f` is confirmed, the two still go
    /// out in push order.
    #[test]
    fn reports_keep_push_order_across_two_reconnects() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        // A (of f) flushed on the first connection, which drops
        let a = report_frame(&iii, "f", 1);
        reconnect(&iii, vec![a]);
        // B (of g) flushed on the second, f not confirmed there yet
        iii.reporter().push(PendingItem::test("g"));
        let mut frames = iii.reporter().take_report_frames(|function_id| {
            if function_id == "f" {
                Gate::Hold
            } else {
                Gate::Send
            }
        });
        assert_eq!(frames.len(), 1, "B goes out, A is parked");
        reconnect(&iii, vec![envelope(frames.pop().unwrap())]);
        // f confirmed on the third
        iii.reporter().unpark("f");
        assert_eq!(flushed_functions(&iii), vec!["f", "g"]);
        assert_eq!(iii.reporter().len(), 0);
        assert!(iii.reporter().take_drops().is_empty());
    }
}
