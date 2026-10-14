// Main JSON single-frame threshold: 16 MiB (16,777,216 bytes).
const MAX_JSON_FRAME_BYTES: usize = 16 * 1024 * 1024;
// Main JSON inbound and outbound message limit: 64 MiB (67,108,864 bytes).
const MAX_JSON_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
// Payload bytes in each outbound fragment above the single-frame threshold.
const JSON_FRAGMENT_BYTES: usize = 1024 * 1024;

#[cfg(test)]
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
        let rejected = IIIClient::prepare_envelope(&invocation("x".repeat(MAX_JSON_MESSAGE_BYTES)))
            .unwrap_err();
        assert_eq!(
            rejected.invocation_error().unwrap().code,
            "payload_too_large"
        );
        let mut queue = vec![
            IIIClient::prepare_envelope(&Message::InvocationResult {
                invocation_id: Uuid::nil(),
                function_id: "f".into(),
                result: Some(Value::String("x".repeat(MAX_JSON_MESSAGE_BYTES))),
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
                payload: Value::String("x".repeat(MAX_JSON_MESSAGE_BYTES)),
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

        register_test_type(&sdk, "x".repeat(MAX_JSON_MESSAGE_BYTES));

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
                .with_description("x".repeat(MAX_JSON_MESSAGE_BYTES)),
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
                .with_description("x".repeat(MAX_JSON_MESSAGE_BYTES)),
            None,
        );
        rejected.unregister();
        register_test_type(&sdk, "x".repeat(MAX_JSON_MESSAGE_BYTES));
        let error = sdk
            .register_trigger(RegisterTriggerInput::new(
                "size::custom",
                "size::handler",
                Value::String("x".repeat(MAX_JSON_MESSAGE_BYTES)),
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
            register_test_type(&rejected_sdk, "x".repeat(MAX_JSON_MESSAGE_BYTES));
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
            *description = "x".repeat(MAX_JSON_MESSAGE_BYTES);
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
        }
    }

    fn envelope_with_size(size: usize) -> PreparedEnvelope {
        let overhead = serde_json::to_string(&invocation(String::new()))
            .unwrap()
            .len();
        IIIClient::prepare_envelope(&invocation("x".repeat(size - overhead))).unwrap()
    }

    fn assert_frame_classification(envelope: &PreparedEnvelope, expected_size: usize) {
        assert_eq!(envelope.payload.len(), expected_size);
        let frames = outbound_ws_frames(envelope);
        if expected_size <= MAX_JSON_FRAME_BYTES {
            assert_eq!(frames.len(), 1);
            assert!(matches!(frames[0], WsMessage::Text(_)));
            return;
        }

        let expected_count = expected_size.div_ceil(JSON_FRAGMENT_BYTES);
        assert_eq!(frames.len(), expected_count);
        for (index, frame) in frames.iter().enumerate() {
            let WsMessage::Frame(frame) = frame else {
                panic!("expected a fragmented WebSocket frame");
            };
            let is_first = index == 0;
            let is_final = index + 1 == expected_count;
            let opcode = if is_first { Data::Text } else { Data::Continue };
            assert_eq!(frame.header().opcode, OpCode::Data(opcode));
            assert_eq!(frame.header().is_final, is_final);
            let expected_payload = if is_final {
                expected_size - index * JSON_FRAGMENT_BYTES
            } else {
                JSON_FRAGMENT_BYTES
            };
            assert_eq!(frame.payload().len(), expected_payload);
        }
    }

    #[test]
    fn complete_utf8_envelope_has_inclusive_single_frame_and_message_boundaries() {
        for size in [
            MAX_JSON_FRAME_BYTES - 1,
            MAX_JSON_FRAME_BYTES,
            MAX_JSON_FRAME_BYTES + 1,
            40 * 1024 * 1024,
            MAX_JSON_MESSAGE_BYTES,
        ] {
            let envelope = envelope_with_size(size);
            assert_frame_classification(&envelope, size);
        }

        let overhead = serde_json::to_string(&invocation(String::new()))
            .unwrap()
            .len();
        let message = invocation("x".repeat(MAX_JSON_MESSAGE_BYTES + 1 - overhead));
        let error = IIIClient::prepare_envelope(&message).unwrap_err();
        let size = MAX_JSON_MESSAGE_BYTES + 1;
        assert_eq!(
            error.invocation_error().unwrap().message,
            format!(
                "Serialized JSON envelope is {size} bytes; limit is {MAX_JSON_MESSAGE_BYTES} bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size"
            )
        );
    }

    #[test]
    fn unicode_is_counted_as_serialized_utf8_and_fragments_may_split_codepoints() {
        let input = "😀é\n\"\\\\".repeat(1_500_000);
        assert!(input.chars().count() < MAX_JSON_FRAME_BYTES);
        let envelope = IIIClient::prepare_envelope(&invocation(input)).unwrap();
        assert!(envelope.payload.len() > MAX_JSON_FRAME_BYTES);
        assert!(envelope.payload.len() <= MAX_JSON_MESSAGE_BYTES);

        let mut message = invocation("😀".repeat(5_000_000));
        for _ in 0..4 {
            let serialized = serde_json::to_string(&message).unwrap();
            if !serialized.is_char_boundary(JSON_FRAGMENT_BYTES) {
                break;
            }
            if let Message::InvokeFunction { function_id, .. } = &mut message {
                function_id.push('x');
            }
        }
        let serialized = serde_json::to_string(&message).unwrap();
        assert!(!serialized.is_char_boundary(JSON_FRAGMENT_BYTES));
        let envelope = IIIClient::prepare_envelope(&message).unwrap();
        let frames = outbound_ws_frames(&envelope);
        let payloads: Vec<_> = frames
            .iter()
            .map(|frame| match frame {
                WsMessage::Frame(frame) => frame.payload(),
                _ => panic!("expected a fragmented data frame"),
            })
            .collect();
        assert!(std::str::from_utf8(payloads[0]).is_err());
        let reassembled: Vec<u8> = payloads.into_iter().flatten().copied().collect();
        assert_eq!(reassembled, serialized.as_bytes());
        let decoded: Message = serde_json::from_slice(&reassembled).unwrap();
        assert!(matches!(decoded, Message::InvokeFunction { .. }));
    }

    #[test]
    fn multibyte_envelope_over_message_limit_is_rejected_by_utf8_byte_count() {
        let message = invocation("😀".repeat(17_000_000));
        let serialized = serde_json::to_string(&message).unwrap();
        let character_count = serialized.chars().count();
        let byte_count = serialized.len();

        assert!(character_count < MAX_JSON_MESSAGE_BYTES);
        assert!(byte_count > MAX_JSON_MESSAGE_BYTES);
        assert_eq!(character_count, 17_000_117);
        assert_eq!(byte_count, 68_000_117);

        let error = IIIClient::prepare_envelope(&message).unwrap_err();
        assert_eq!(error.invocation_error().unwrap().code, "payload_too_large");
        assert_eq!(
            error.invocation_error().unwrap().message,
            format!(
                "Serialized JSON envelope is {byte_count} bytes; limit is {MAX_JSON_MESSAGE_BYTES} bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size"
            )
        );
    }

    #[test]
    fn escaped_utf8_below_message_limit_is_rejected_after_json_expansion() {
        let input = String::from_utf8(vec![0; 11_200_000]).unwrap();
        let raw_bytes = input.len();
        let message = invocation(input);
        let serialized = serde_json::to_string(&message).unwrap();
        let envelope_bytes = serialized.len();

        assert!(raw_bytes < MAX_JSON_MESSAGE_BYTES);
        assert!(envelope_bytes > MAX_JSON_MESSAGE_BYTES);
        assert!(
            envelope_bytes >= 6 * raw_bytes,
            "NUL escaping should expand each raw byte to a six-byte JSON escape"
        );

        let error = IIIClient::prepare_envelope(&message).unwrap_err();
        assert_eq!(error.invocation_error().unwrap().code, "payload_too_large");
        assert_eq!(
            error.invocation_error().unwrap().message,
            format!(
                "Serialized JSON envelope is {envelope_bytes} bytes; limit is {MAX_JSON_MESSAGE_BYTES} bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size"
            )
        );
    }

    #[tokio::test]
    async fn fragmented_envelopes_reassemble_under_engine_frame_limits() {
        use tokio_tungstenite::accept_async_with_config;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let config = WebSocketConfig::default()
                .max_frame_size(Some(MAX_JSON_FRAME_BYTES))
                .max_message_size(Some(MAX_JSON_MESSAGE_BYTES));
            let mut ws = accept_async_with_config(socket, Some(config))
                .await
                .unwrap();
            for expected_size in [
                MAX_JSON_FRAME_BYTES,
                MAX_JSON_FRAME_BYTES + 1,
                40 * 1024 * 1024,
                MAX_JSON_MESSAGE_BYTES,
            ] {
                let frame = ws.next().await.unwrap().unwrap();
                let text = frame.into_text().unwrap();
                assert_eq!(text.len(), expected_size);
                assert!(text.starts_with("{\"type\":\"invokefunction\""));
                assert!(text.ends_with('}'));
            }
            let frame = ws.next().await.unwrap().unwrap();
            let text = frame.into_text().unwrap();
            let parsed: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(parsed["data"].as_str().unwrap().chars().count(), 4_200_000);
            ws.send(WsMessage::Ping(Bytes::new())).await.unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                WsMessage::Pong(_)
            ));
        });

        let (stream, _) = connect_async(format!("ws://{address}")).await.unwrap();
        let (mut tx, mut rx) = stream.split();
        let sdk = IIIClient::new("ws://127.0.0.1:0");
        for size in [
            MAX_JSON_FRAME_BYTES,
            MAX_JSON_FRAME_BYTES + 1,
            40 * 1024 * 1024,
            MAX_JSON_MESSAGE_BYTES,
        ] {
            sdk.send_ws(&mut tx, &envelope_with_size(size))
                .await
                .unwrap();
        }
        let mut unicode = invocation("😀".repeat(4_200_000));
        for _ in 0..4 {
            let serialized = serde_json::to_string(&unicode).unwrap();
            if !serialized.is_char_boundary(JSON_FRAGMENT_BYTES) {
                break;
            }
            if let Message::InvokeFunction { function_id, .. } = &mut unicode {
                function_id.push('x');
            }
        }
        sdk.send_ws(&mut tx, &IIIClient::prepare_envelope(&unicode).unwrap())
            .await
            .unwrap();
        assert!(matches!(
            rx.next().await.unwrap().unwrap(),
            WsMessage::Ping(_)
        ));
        tx.send(WsMessage::Pong(Bytes::new())).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_callers_cannot_interleave_fragments_and_connection_stays_open() {
        use tokio_tungstenite::accept_async_with_config;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (ping_tx, ping_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let config = WebSocketConfig::default()
                .max_frame_size(Some(MAX_JSON_FRAME_BYTES))
                .max_message_size(Some(MAX_JSON_MESSAGE_BYTES));
            let mut ws = accept_async_with_config(socket, Some(config))
                .await
                .unwrap();
            let mut received = HashSet::new();
            while received.len() < 3 {
                let frame = ws.next().await.unwrap().unwrap();
                let WsMessage::Text(text) = frame else {
                    continue;
                };
                let parsed: Value = serde_json::from_str(&text).unwrap();
                let Some(function_id) = parsed["function_id"].as_str() else {
                    continue;
                };
                if function_id.starts_with("size::concurrent_") {
                    assert_eq!(
                        parsed["data"].as_str().unwrap().len(),
                        MAX_JSON_FRAME_BYTES + 1
                    );
                    received.insert(function_id.to_owned());
                }
            }
            assert_eq!(received.len(), 3);
            ws.send(WsMessage::Ping(Bytes::new())).await.unwrap();
            let _ = ping_tx.send(());
            loop {
                let frame = ws.next().await.unwrap().unwrap();
                let WsMessage::Text(text) = frame else {
                    continue;
                };
                let parsed: Value = serde_json::from_str(&text).unwrap();
                if parsed["function_id"] == "size::after" {
                    break;
                }
            }
        });

        let sdk = IIIClient::new(&format!("ws://{address}"));
        sdk.connect();
        tokio::time::timeout(Duration::from_secs(10), async {
            while sdk.get_connection_state() != IIIConnectionState::Connected {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("client should connect to the in-process engine");

        let mut callers = Vec::new();
        for index in 0..3 {
            let sdk = sdk.clone();
            callers.push(tokio::task::spawn_blocking(move || {
                let mut message = invocation("x".repeat(MAX_JSON_FRAME_BYTES + 1));
                if let Message::InvokeFunction { function_id, .. } = &mut message {
                    *function_id = format!("size::concurrent_{index}");
                }
                sdk.send_message(message).unwrap();
            }));
        }
        for caller in callers {
            caller.await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(20), ping_rx)
            .await
            .expect("server should receive whole fragmented messages before ping")
            .unwrap();
        let mut after = invocation("after".to_owned());
        if let Message::InvokeFunction { function_id, .. } = &mut after {
            *function_id = "size::after".into();
        }
        sdk.send_message(after).unwrap();
        tokio::time::timeout(Duration::from_secs(10), server)
            .await
            .expect("connection should remain open after fragmented messages")
            .unwrap();
        sdk.shutdown_async().await;
    }

    #[test]
    fn result_fallback_preserves_correlation_and_discards_large_context() {
        let message = Message::InvocationResult {
            invocation_id: Uuid::nil(),
            function_id: "size::echo".into(),
            result: Some(Value::String("x".repeat(MAX_JSON_MESSAGE_BYTES))),
            error: None,
            traceparent: None,
            baggage: Some("secret".repeat(1000)),
        };
        let size = serde_json::to_string(&message).unwrap().len();
        let payload = IIIClient::prepare_envelope(&message).unwrap().payload;
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["invocation_id"], Uuid::nil().to_string());
        assert_eq!(parsed["error"]["code"], "payload_too_large");
        assert_eq!(
            parsed["error"]["message"],
            format!(
                "Serialized JSON envelope is {size} bytes; limit is {MAX_JSON_MESSAGE_BYTES} bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size"
            )
        );
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
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

/// Extension trait for Mutex that recovers from poisoning instead of panicking.
/// This is safe when the protected data is still valid after a panic in another thread.
trait MutexExt<T> {
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
use tokio_tungstenite::tungstenite::{
    Bytes, Message as WsMessage, Utf8Bytes,
    protocol::{
        WebSocketConfig,
        frame::{
            Frame,
            coding::{Data, OpCode},
        },
    },
};
use uuid::Uuid;

const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

use iii_helpers::http::HttpInvocationConfig;

use crate::{
    channels::{
        ChannelReader, ChannelWriter, StreamChannelRef, handshake_headers, handshake_request,
    },
    error::Error,
    protocol::{
        ErrorBody, FUNCTION_NAMESPACE_CONFLICT, Message, RegisterFunctionMessage,
        RegisterTriggerInput, RegisterTriggerMessage, RegisterTriggerTypeMessage, TriggerAction,
        TriggerRequest, TriggerRequestWithMetadata, UnregisterTriggerMessage,
        UnregisterTriggerTypeMessage, WORKER_NAMESPACE_CONFLICT,
    },
    transport::{self, WsConnectResult, WsStream},
    triggers::{Trigger, TriggerConfig, TriggerHandler},
    types::{
        Channel, RemoteFunctionData, RemoteFunctionHandlerWithMetadata, RemoteTriggerTypeData,
    },
};

use iii_helpers::observability as telemetry;
use iii_helpers::observability::OtelConfig;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;

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
    pub fn register_trigger(
        &self,
        function_id: impl Into<String>,
        config: C,
    ) -> Result<Trigger, Error> {
        self.register_trigger_with_metadata(function_id, config, None)
    }

    /// Register a trigger with compile-time validated trigger config and optional metadata.
    ///
    /// This typed helper pairs a function with its trigger, so it defaults the
    /// trigger's namespace to this worker's — otherwise the function lands in the
    /// worker's namespace and the trigger in `default`, never resolving it. The
    /// low-level [`IIIClient::register_trigger`] keeps the engine default.
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
        self.iii.register_trigger(input)
    }
}

impl<C, R> TriggerTypeRef<C, R>
where
    R: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
{
    /// Register a sync function whose input type must match
    /// the call request format `R`.
    pub fn register_function<O, F>(&self, id: impl Into<String>, f: F) -> FunctionRef
    where
        O: Serialize + schemars::JsonSchema + Send + 'static,
        F: Fn(R) -> Result<O, Error> + Send + Sync + 'static,
    {
        self.iii.register_function(id, RegisterFunction::new(f))
    }

    /// Register an async function whose input type must match
    /// the call request format `R`.
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
struct PreparedEnvelope {
    payload: Utf8Bytes,
    registration_key: Option<String>,
    // The payload is dropped before its lease; send_ws borrows the envelope
    // throughout its shared-byte clone and network await.
    _spare_capacity: Option<SpareCapacityLease>,
}

fn outbound_ws_frames(message: &PreparedEnvelope) -> Vec<WsMessage> {
    if message.payload.len() <= MAX_JSON_FRAME_BYTES {
        return vec![WsMessage::Text(message.payload.clone())];
    }

    let payload: Bytes = message.payload.clone().into();
    (0..payload.len())
        .step_by(JSON_FRAGMENT_BYTES)
        .map(|offset| {
            let end = (offset + JSON_FRAGMENT_BYTES).min(payload.len());
            let opcode = if offset == 0 {
                Data::Text
            } else {
                Data::Continue
            };
            let is_final = end == payload.len();
            WsMessage::Frame(Frame::message(
                payload.slice(offset..end),
                OpCode::Data(opcode),
                is_final,
            ))
        })
        .collect()
}

impl PreparedEnvelope {
    fn from_json(
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
            _spare_capacity: lease,
        }
    }
}

enum Outbound {
    Message(PreparedEnvelope),
    Shutdown,
}

type PendingInvocation = oneshot::Sender<Result<Value, Error>>;

// WebSocket transmitter type alias
type WsTx = futures_util::stream::SplitSink<WsStream, WsMessage>;

/// Inject trace context headers for outbound messages.
fn inject_trace_headers() -> (Option<String>, Option<String>) {
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
}

impl FunctionRef {
    pub fn unregister(&self) {
        (self.unregister_fn)();
    }
}

fn json_schema_for<T: schemars::JsonSchema>() -> Option<Value> {
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
pub struct RegisterFunction {
    message: RegisterFunctionMessage,
    handler: Option<RemoteFunctionHandlerWithMetadata>,
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

    pub(crate) fn into_parts(
        self,
    ) -> (
        RegisterFunctionMessage,
        Option<RemoteFunctionHandlerWithMetadata>,
    ) {
        (self.message, self.handler)
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

struct IIIInner {
    address: String,
    outbound: mpsc::UnboundedSender<Outbound>,
    spare_capacity_budget: Arc<AtomicUsize>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<Outbound>>>,
    running: AtomicBool,
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
}

/// WebSocket client for communication with the III Engine.
///
/// Create with [`register_worker`](crate::register_worker).
#[derive(Clone)]
pub struct IIIClient {
    inner: Arc<IIIInner>,
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
        };
        Self {
            inner: Arc::new(inner),
        }
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
        self.inner.registration_changed.notify_waiters();
        let _ = self.inner.outbound.send(Outbound::Shutdown);
        self.set_connection_state(IIIConnectionState::Disconnected);
    }

    fn register_function_inner(
        &self,
        message: RegisterFunctionMessage,
        handler: Option<RemoteFunctionHandlerWithMetadata>,
    ) -> FunctionRef {
        let id = message.id.clone();
        if id.trim().is_empty() {
            panic!("id is required");
        }
        let envelope = match Self::prepare_envelope(&message.to_message()) {
            Ok(envelope) => envelope,
            Err(error) => {
                tracing::warn!(error = %error, "Function registration rejected locally");
                // A rejected handle must not unregister a previous owner of this ID.
                return FunctionRef {
                    id,
                    unregister_fn: Arc::new(|| {}),
                };
            }
        };
        let data = RemoteFunctionData { message, handler };
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

        let iii = self.clone();
        let unregister_id = id.clone();
        let unregister_fn = Arc::new(move || {
            let _ = iii.inner.functions.lock_or_recover().remove(&unregister_id);
            let _ = iii.send_message(Message::UnregisterFunction {
                id: unregister_id.clone(),
            });
        });

        FunctionRef { id, unregister_fn }
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
    /// Panics if `id` is empty or already registered.
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
        let (mut message, handler) = registration.into_parts();
        message.id = id.into();
        self.register_function_inner(message, handler)
    }

    /// Register a custom trigger type with the engine.
    ///
    /// Returns a [`TriggerTypeRef`] handle that can register triggers and
    /// functions with compile-time validated types.
    ///
    /// # Examples
    /// ```rust,no_run
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
    pub fn register_trigger_type<H, C, R>(
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
    /// # Examples
    /// ```rust,no_run
    /// # use iii_sdk::{register_worker, InitOptions};
    /// # let worker = register_worker("ws://localhost:49134", InitOptions::default());
    /// worker.unregister_trigger_type("cron");
    /// ```
    pub fn unregister_trigger_type(&self, id: impl Into<String>) {
        let id = id.into();
        self.inner.trigger_types.lock_or_recover().remove(&id);
        let msg = UnregisterTriggerTypeMessage { id };
        let _ = self.send_message(msg.to_message());
    }

    /// Bind a trigger configuration to a registered function.
    /// <!-- docs:expand-params -->
    ///
    /// # Arguments
    /// * `input` - Trigger registration input with trigger_type, function_id, and config.
    ///
    /// # Examples
    /// ```rust
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
    pub fn register_trigger(&self, input: RegisterTriggerInput) -> Result<Trigger, Error> {
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

    /// Invoke a remote function.
    /// <!-- docs:expand-params: TriggerRequest -->
    ///
    /// The routing behavior depends on the `action` field of the request:
    /// - No action: synchronous, waits for the function to return.
    /// - [`TriggerAction::Enqueue`]: async via named queue.
    /// - [`TriggerAction::Void`][]: fire-and-forget.
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
        let request = request.into();
        let req = request.request;
        let metadata = request.metadata;
        // Engine-owned builtins always live in `default`. Other implicit calls
        // stay in the caller's namespace. An explicit request namespace wins.
        let namespace = invocation_namespace(
            request.namespace,
            self.namespace(),
            &req.function_id,
            "TriggerRequest.namespace",
        )?;
        let (tp, bg) = inject_trace_headers();

        // Void is fire-and-forget, no invocation_id, no response
        if matches!(req.action, Some(TriggerAction::Void)) {
            self.send_message(Message::InvokeFunction {
                invocation_id: None,
                function_id: req.function_id,
                data: req.payload,
                traceparent: tp,
                baggage: bg,
                action: req.action,
                metadata,
                namespace,
            })?;
            return Ok(Value::Null);
        }

        // Enqueue and default: use invocation_id to receive acknowledgement/result
        let timeout = Duration::from_millis(req.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
        let invocation_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();

        self.inner
            .pending
            .lock_or_recover()
            .insert(invocation_id, tx);

        if let Err(error) = self.send_message(Message::InvokeFunction {
            invocation_id: Some(invocation_id),
            function_id: req.function_id,
            data: req.payload,
            traceparent: tp,
            baggage: bg,
            action: req.action,
            metadata,
            namespace,
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

    fn set_connection_state(&self, state: IIIConnectionState) {
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
            if let Ok(value) = serde_json::to_value(metadata) {
                let _ = self.send_message(Message::InvokeFunction {
                    invocation_id: None,
                    function_id: "engine::workers::register".to_string(),
                    data: value,
                    traceparent: None,
                    baggage: None,
                    action: Some(TriggerAction::Void),
                    metadata: None,
                    namespace: None,
                });
            }
        }
    }

    // Receive complete JSON messages up to the shared 64 MiB message limit.
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
        if size <= MAX_JSON_MESSAGE_BYTES {
            return Ok(PreparedEnvelope::from_json(
                payload,
                Self::registration_key(message),
                budget,
            ));
        }
        let error_message = format!(
            "Serialized JSON envelope is {size} bytes; limit is {MAX_JSON_MESSAGE_BYTES} bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size"
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
            if payload.len() <= MAX_JSON_MESSAGE_BYTES {
                return Ok(PreparedEnvelope::from_json(payload, None, None));
            }
        }
        Err(Error::Remote {
            code: "payload_too_large".to_string(),
            message: error_message,
            stacktrace: None,
        })
    }

    fn send_message(&self, message: Message) -> Result<(), Error> {
        let envelope = match self.prepare_outbound(&message) {
            Ok(envelope) => envelope,
            Err(error) => {
                tracing::warn!(error = %error, "JSON envelope rejected locally");
                return Err(error);
            }
        };
        self.send_prepared(envelope)
    }

    fn send_prepared(&self, envelope: PreparedEnvelope) -> Result<(), Error> {
        if !self.inner.running.load(Ordering::SeqCst) {
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
                        return;
                    }

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
                                    Some(Outbound::Message(message)) => {
                                        if let Err(err) = self.send_ws(&mut ws_tx, &message).await {
                                            tracing::warn!(error = %err, "send failed; reconnecting");
                                            queue.push(message);
                                            should_reconnect = true;
                                        }
                                    }
                                    Some(Outbound::Shutdown) => {
                                        self.inner.running.store(false, Ordering::SeqCst);
                                        return;
                                    }
                                    None => {
                                        self.inner.running.store(false, Ordering::SeqCst);
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
            _ => None,
        }
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
        while let Some(message) = iter.next() {
            if let Err(err) = self.send_ws(ws_tx, &message).await {
                queue.push(message);
                queue.extend(iter);
                return Err(err);
            }
        }

        Ok(())
    }

    async fn send_ws(&self, ws_tx: &mut WsTx, message: &PreparedEnvelope) -> Result<(), Error> {
        let timeout = self.inner.timings.lock_or_recover().idle_timeout;
        for frame in outbound_ws_frames(message) {
            self.send_ws_frame(ws_tx, frame, timeout).await?;
        }
        Ok(())
    }

    async fn send_ws_frame(
        &self,
        ws_tx: &mut WsTx,
        frame: WsMessage,
        timeout: Duration,
    ) -> Result<(), Error> {
        match tokio::time::timeout(timeout, ws_tx.send(frame)).await {
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

    fn handle_message(&self, payload: &str) -> Result<(), Error> {
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
            } => {
                self.handle_invoke_function(
                    invocation_id,
                    function_id,
                    data,
                    traceparent,
                    baggage,
                    metadata,
                );
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
            } => {
                tracing::debug!(worker_id = %worker_id, "Worker registered");
                *self.inner.worker_id.lock_or_recover() = Some(worker_id);
                *self.inner.reattach_token.lock_or_recover() = reattach_token;
                self.inner.registration_changed.notify_waiters();
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

    fn handle_invoke_function(
        &self,
        invocation_id: Option<Uuid>,
        function_id: String,
        data: Value,
        traceparent: Option<String>,
        baggage: Option<String>,
        metadata: Option<Value>,
    ) {
        tracing::debug!(function_id = %function_id, traceparent = ?traceparent, baggage = ?baggage, "Invoking function");

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

            if let Some(invocation_id) = invocation_id {
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

        let iii = self.clone();

        tokio::spawn(async move {
            // Extract incoming trace context and create a span for this invocation.
            // This ensures the handler and any outbound calls it makes (e.g.
            // invoke_function_with_timeout) are linked as children of the caller's trace.
            // We use FutureExt::with_context() instead of cx.attach() because
            // ContextGuard is !Send and can't be held across .await in tokio::spawn.
            let otel_cx = {
                use iii_helpers::observability::extract_context;
                use iii_helpers::observability::opentelemetry::trace::{
                    SpanKind, TraceContextExt, Tracer,
                };

                let parent_cx = extract_context(traceparent.as_deref(), baggage.as_deref());
                let tracer =
                    iii_helpers::observability::opentelemetry::global::tracer("iii-rust-sdk");
                // INTERNAL and named `execute` (not `call`/`trigger`): the engine
                // already emits the SERVER `call <fn>` span for this hop AND a
                // `trigger <fn>` span from fire_triggers. Reusing either name would
                // duplicate an engine span under the worker's service. `execute` is
                // unique, so the worker handler span reads as a clean internal child
                // of the engine's call span (and is collapsible by a single rule).
                let span = tracer
                    .span_builder(format!("execute {}", function_id))
                    .with_kind(SpanKind::Internal)
                    .start_with_context(&tracer, &parent_cx);
                parent_cx.with_span(span)
            };

            let trace_payloads = !std::env::var("III_DISABLE_TRACE_PAYLOADS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            let payload_max_bytes = iii_helpers::observability::resolve_max_bytes_from_env();

            if trace_payloads {
                use iii_helpers::observability::opentelemetry::KeyValue;
                use iii_helpers::observability::opentelemetry::trace::TraceContextExt;
                use iii_helpers::observability::redact_and_truncate;
                let span = otel_cx.span();
                if span.is_recording() {
                    let (input_json, truncated) = redact_and_truncate(&data, payload_max_bytes);
                    span.add_event(
                        "iii.invocation.input",
                        vec![
                            KeyValue::new("iii.payload.json", input_json),
                            KeyValue::new("iii.payload.truncated", truncated),
                        ],
                    );
                }
            }

            let result = {
                use iii_helpers::observability::opentelemetry::trace::FutureExt as OtelFutureExt;
                handler(data, metadata).with_context(otel_cx.clone()).await
            };

            if trace_payloads {
                use iii_helpers::observability::opentelemetry::KeyValue;
                use iii_helpers::observability::opentelemetry::trace::TraceContextExt;
                use iii_helpers::observability::redact_and_truncate;
                let span = otel_cx.span();
                if span.is_recording() {
                    let (output_json, truncated, ok) = match &result {
                        Ok(value) => {
                            let (j, t) = redact_and_truncate(value, payload_max_bytes);
                            (j, t, true)
                        }
                        Err(err) => {
                            let payload = serde_json::json!({ "error": err.to_string() });
                            let (j, t) = redact_and_truncate(&payload, payload_max_bytes);
                            (j, t, false)
                        }
                    };
                    span.add_event(
                        "iii.invocation.output",
                        vec![
                            KeyValue::new("iii.payload.json", output_json),
                            KeyValue::new("iii.payload.truncated", truncated),
                            KeyValue::new("iii.payload.ok", ok),
                        ],
                    );
                }
            }

            // Record span status based on result
            let mut error_stacktrace: Option<String> = None;
            {
                use iii_helpers::observability::opentelemetry::KeyValue;
                use iii_helpers::observability::opentelemetry::trace::{Status, TraceContextExt};
                let span = otel_cx.span();
                match &result {
                    Ok(_) => span.set_status(Status::Ok),
                    Err(err) => {
                        let (exc_type, exc_message, stacktrace) = match err {
                            Error::Remote {
                                code,
                                message,
                                stacktrace,
                            } => (
                                code.clone(),
                                message.clone(),
                                stacktrace.clone().unwrap_or_else(|| {
                                    std::backtrace::Backtrace::force_capture().to_string()
                                }),
                            ),
                            other => (
                                "InvocationError".to_string(),
                                other.to_string(),
                                std::backtrace::Backtrace::force_capture().to_string(),
                            ),
                        };
                        span.set_status(Status::error(exc_message.clone()));
                        span.add_event(
                            "exception",
                            vec![
                                KeyValue::new("exception.type", exc_type),
                                KeyValue::new("exception.message", exc_message),
                                KeyValue::new("exception.stacktrace", stacktrace.clone()),
                            ],
                        );
                        // Consumed only by the non-`Remote` fallback arm when
                        // building the wire ErrorBody below; `Remote` passes
                        // its own (possibly absent) stacktrace through.
                        error_stacktrace = Some(stacktrace);
                    }
                }
            }

            if let Some(invocation_id) = invocation_id {
                // Inject trace context from our span into the response.
                // We briefly attach the otel context (no .await crossing)
                // so inject_traceparent/inject_baggage can read it.
                let (resp_tp, resp_bg) = {
                    let _guard = otel_cx.attach();
                    inject_trace_headers()
                };

                let message = match result {
                    Ok(value) => Message::InvocationResult {
                        invocation_id,
                        function_id,
                        result: Some(value),
                        error: None,
                        traceparent: resp_tp,
                        baggage: resp_bg,
                    },
                    Err(err) => {
                        let error_body = match err {
                            Error::Remote {
                                code,
                                message,
                                stacktrace,
                            } => ErrorBody {
                                code,
                                message,
                                // `Remote` is a structured, expected error owned by
                                // the handler, respect `stacktrace: None` instead of
                                // backfilling the dispatch-loop backtrace, which
                                // points at the SDK event loop, not the error site.
                                stacktrace,
                            },
                            other => ErrorBody {
                                code: "invocation_failed".to_string(),
                                message: other.to_string(),
                                stacktrace: error_stacktrace.or_else(|| {
                                    Some(std::backtrace::Backtrace::force_capture().to_string())
                                }),
                            },
                        };
                        Message::InvocationResult {
                            invocation_id,
                            function_id,
                            result: None,
                            error: Some(error_body),
                            traceparent: resp_tp,
                            baggage: resp_bg,
                        }
                    }
                };

                let _ = iii.send_message(message);
            } else if let Err(err) = result {
                tracing::warn!(error = %err, "error handling async invocation");
            }
        });
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
                reattach_token: None
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
