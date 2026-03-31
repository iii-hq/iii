/// Pattern: Realtime Streams
/// Comparable to: Socket.io, Pusher, Firebase Realtime
///
/// Push live data to connected WebSocket clients.
/// Clients connect at: ws://host:3112/stream/{stream_name}/{group_id}
///
/// Built-in stream operations: stream::set, stream::get, stream::list,
/// stream::delete, stream::send.

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest, TriggerAction,
    builtin_triggers::*, IIITrigger, Logger,
};
use serde_json::json;
use std::time::Duration;

use serde;
use schemars;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PostMessageInput {
    room: String,
    sender: String,
    text: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct GetMessageInput {
    room: String,
    #[serde(rename = "messageId")]
    message_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ListMessagesInput {
    room: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct DeleteMessageInput {
    room: String,
    #[serde(rename = "messageId")]
    message_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BroadcastInput {
    room: String,
    sender: String,
    text: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PresenceInput {
    room: String,
    #[serde(rename = "userId")]
    user_id: String,
    name: Option<String>,
}

fn main() {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // ---
    // stream::set - Persist an item in a stream group
    // Payload: { stream_name, group_id, item_id, data }
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("chat::post-message", move |data: PostMessageInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let message_id = format!("msg-{}", chrono::Utc::now().timestamp_millis());

                iii.trigger(TriggerRequest {
                    function_id: "stream::set".into(),
                    payload: json!({
                        "stream_name": "chat",
                        "group_id": data.room,
                        "item_id": message_id,
                        "data": {
                            "sender": data.sender,
                            "text": data.text,
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                logger.info("Message stored in stream", &json!({ "room": data.room, "messageId": message_id }));
                Ok(json!({ "messageId": message_id }))
            }
        })
        .description("Post a chat message to a room stream"),
    );

    // ---
    // stream::get - Retrieve a single item from a stream group
    // Payload: { stream_name, group_id, item_id }
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("chat::get-message", move |data: GetMessageInput| {
            let iii = iii_clone.clone();
            async move {
                let message = iii
                    .trigger(TriggerRequest {
                        function_id: "stream::get".into(),
                        payload: json!({
                            "stream_name": "chat",
                            "group_id": data.room,
                            "item_id": data.message_id,
                        }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                if message.is_null() {
                    return Ok(json!({ "error": "Message not found" }));
                }

                Ok(message)
            }
        })
        .description("Get a single chat message"),
    );

    // ---
    // stream::list - List all items in a stream group
    // Payload: { stream_name, group_id }
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("chat::list-messages", move |data: ListMessagesInput| {
            let iii = iii_clone.clone();
            async move {
                let messages = iii
                    .trigger(TriggerRequest {
                        function_id: "stream::list".into(),
                        payload: json!({
                            "stream_name": "chat",
                            "group_id": data.room,
                        }),
                        action: None,
                        timeout_ms: None,
                    })
                    .await
                    .map_err(|e| e.to_string())?;

                let arr = messages.as_array().cloned().unwrap_or_default();
                Ok(json!({ "room": data.room, "messages": arr }))
            }
        })
        .description("List all messages in a chat room"),
    );

    // ---
    // stream::delete - Remove an item from a stream group
    // Payload: { stream_name, group_id, item_id }
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("chat::delete-message", move |data: DeleteMessageInput| {
            let iii = iii_clone.clone();
            async move {
                iii.trigger(TriggerRequest {
                    function_id: "stream::delete".into(),
                    payload: json!({
                        "stream_name": "chat",
                        "group_id": data.room,
                        "item_id": data.message_id,
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                Ok(json!({ "deleted": data.message_id }))
            }
        })
        .description("Delete a chat message"),
    );

    // ---
    // stream::send - Push a live event to all connected clients
    // Clients on ws://host:3112/stream/chat/{room} receive this instantly.
    // Use TriggerAction::Void for fire-and-forget delivery.
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("chat::broadcast", move |data: BroadcastInput| {
            let iii = iii_clone.clone();
            async move {
                let logger = Logger::new();
                let event_id = format!("evt-{}", chrono::Utc::now().timestamp_millis());
                let timestamp = chrono::Utc::now().to_rfc3339();

                iii.trigger(TriggerRequest {
                    function_id: "stream::set".into(),
                    payload: json!({
                        "stream_name": "chat",
                        "group_id": data.room,
                        "item_id": event_id,
                        "data": {
                            "sender": data.sender,
                            "text": data.text,
                            "timestamp": timestamp,
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                iii.trigger(TriggerRequest {
                    function_id: "stream::send".into(),
                    payload: json!({
                        "stream_name": "chat",
                        "group_id": data.room,
                        "id": event_id,
                        "event_type": "new_message",
                        "data": {
                            "sender": data.sender,
                            "text": data.text,
                            "timestamp": timestamp,
                        },
                    }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .ok();

                logger.info("Message broadcast", &json!({ "room": data.room, "eventId": event_id }));
                Ok(json!({ "eventId": event_id }))
            }
        })
        .description("Broadcast a message to all connected clients"),
    );

    // ---
    // Presence tracking - user joins/leaves
    // Clients connect at: ws://host:3112/stream/presence/{room}
    // ---
    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("presence::join", move |data: PresenceInput| {
            let iii = iii_clone.clone();
            async move {
                iii.trigger(TriggerRequest {
                    function_id: "stream::set".into(),
                    payload: json!({
                        "stream_name": "presence",
                        "group_id": data.room,
                        "item_id": data.user_id,
                        "data": {
                            "userId": data.user_id,
                            "name": data.name,
                            "status": "online",
                        },
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                iii.trigger(TriggerRequest {
                    function_id: "stream::send".into(),
                    payload: json!({
                        "stream_name": "presence",
                        "group_id": data.room,
                        "id": format!("join-{}", chrono::Utc::now().timestamp_millis()),
                        "event_type": "user_joined",
                        "data": { "userId": data.user_id, "name": data.name },
                    }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .ok();

                Ok(json!({ "joined": data.room }))
            }
        })
        .description("User joins a presence room"),
    );

    let iii_clone = iii.clone();
    iii.register_function(
        RegisterFunction::new_async("presence::leave", move |data: PresenceInput| {
            let iii = iii_clone.clone();
            async move {
                iii.trigger(TriggerRequest {
                    function_id: "stream::delete".into(),
                    payload: json!({
                        "stream_name": "presence",
                        "group_id": data.room,
                        "item_id": data.user_id,
                    }),
                    action: None,
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                iii.trigger(TriggerRequest {
                    function_id: "stream::send".into(),
                    payload: json!({
                        "stream_name": "presence",
                        "group_id": data.room,
                        "id": format!("leave-{}", chrono::Utc::now().timestamp_millis()),
                        "event_type": "user_left",
                        "data": { "userId": data.user_id },
                    }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .ok();

                Ok(json!({ "left": data.room }))
            }
        })
        .description("User leaves a presence room"),
    );

    // ---
    // HTTP triggers
    // ---
    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/chat/send").method(HttpMethod::Post))
            .for_function("chat::broadcast"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/chat/:room/messages").method(HttpMethod::Get))
            .for_function("chat::list-messages"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/presence/join").method(HttpMethod::Post))
            .for_function("presence::join"),
    )
    .expect("failed");

    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/presence/leave").method(HttpMethod::Post))
            .for_function("presence::leave"),
    )
    .expect("failed");

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::signal::ctrl_c().await.ok();
    });
    iii.shutdown();
}
