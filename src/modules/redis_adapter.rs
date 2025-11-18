use serde_json::Value;

use crate::modules::event::EventAdapter;

pub struct RedisAdapter {}

impl EventAdapter for RedisAdapter {
    fn emit(&self, topic: &str, event_data: Value) {
        tracing::info!("Emitting event to topic: {}", topic);
        tracing::info!("Event data: {:?}", event_data);
    }
    fn subscribe(&self, topic: &str, id: &str, function_path: &str) {
        tracing::info!("Subscribing to topic: {}", topic);
        tracing::info!("ID: {}", id);
        tracing::info!("Function path: {}", function_path);
    }
    fn unsubscribe(&self, topic: &str, id: &str) {
        tracing::info!("Unsubscribing from topic: {}", topic);
        tracing::info!("ID: {}", id);
    }
}

impl RedisAdapter {
    pub fn new() -> Self {
        Self {}
    }
}

// use redis::Client as RedisClient;
// use serde_json::Value;
// use std::sync::Arc;
// use tokio::sync::Mutex;

// use crate::modules::event::EventAdapter;

// pub struct RedisAdapter {
//     client: Arc<Mutex<RedisClient>>,
// }

// impl EventAdapter for RedisAdapter {
//     fn emit(&self, _topic: &str, _event_data: Value) {
//         tracing::info!("Emitting event to Redis");

//         // let topic = topic.to_string();
//         // let payload = event_data.to_string();
//         // let client = self.client.clone();
//         // // Fire and forget (spawned) for demo: for production, you may want to propagate errors/provide ack
//         // tokio::spawn(async move {
//         //     let mut conn = match client.lock().await.get_async_connection().await {
//         //         Ok(conn) => conn,
//         //         Err(err) => {
//         //             tracing::error!("Redis emit connect error: {:?}", err);
//         //             return;
//         //         }
//         //     };
//         //     let publish_res: redis::RedisResult<i64> = conn.publish(topic.as_str(), &payload).await;
//         //     match publish_res {
//         //         Ok(_) => tracing::info!("Published event to topic: {}", topic),
//         //         Err(e) => tracing::error!("Failed to publish event to topic '{}': {:?}", topic, e),
//         //     }
//         // });
//     }

//     fn subscribe(&self, topic: &str, id: &str, function_path: &str) {
//         tracing::info!(
//             topic = topic,
//             id = id,
//             function_path = function_path,
//             "Subscribing to Redis"
//         );
//         // // Here, for every subscription, we'd typically spawn a listener task.
//         // // For now, only log; a full implementation would hand the message to a function handler when redis PubSub receives a msg.
//         // tracing::info!(
//         //     "Request to subscribe to topic '{}' (id: {}, function: {})",
//         //     topic,
//         //     id,
//         //     function_path
//         // );

//         // let topic = topic.to_string();
//         // let function_path = function_path.to_string();
//         // let client = self.client.clone();

//         // // For illustration; a real solution would need a subscription coordination (per process) and handler invocation with an engine reference.
//         // tokio::spawn(async move {
//         //     let mut pubsub_conn = match client.lock().await.get_async_connection().await {
//         //         Ok(conn) => redis::aio::PubSub::new(conn),
//         //         Err(err) => {
//         //             tracing::error!("Redis subscribe connect error: {:?}", err);
//         //             return;
//         //         }
//         //     };
//         //     if let Err(e) = pubsub_conn.subscribe(topic.as_str()).await {
//         //         tracing::error!("Failed to subscribe to '{}': {:?}", topic, e);
//         //         return;
//         //     }
//         //     // Demo: just log messages (no invocation)
//         //     loop {
//         //         match pubsub_conn.on_message().await {
//         //             Ok(msg) => {
//         //                 let payload: redis::RedisResult<String> = msg.get_payload();
//         //                 match payload {
//         //                     Ok(p) => tracing::info!(
//         //                         "Received on {}: payload = {} (function: {})",
//         //                         topic,
//         //                         p,
//         //                         function_path
//         //                     ),
//         //                     Err(e) => tracing::error!("Error getting payload: {:?}", e),
//         //                 }
//         //             }
//         //             Err(err) => {
//         //                 tracing::error!("Error receiving pubsub msg: {:?}", err);
//         //                 break;
//         //             }
//         //         }
//         //     }
//         // });
//     }

//     fn unsubscribe(&self, topic: &str, id: &str) {
//         // Unsubscribe is non-trivial, since you must track/abort background tasks
//         // For now, log; actual unsub needs coordinated task handling.
//         tracing::info!(
//             "Request to unsubscribe from topic '{}' with id '{}'",
//             topic,
//             id
//         );
//     }
// }

// impl RedisAdapter {
//     pub fn new() -> Self {
//         // Change the URL as needed; you can make this configurable if required.
//         let client =
//             RedisClient::open("redis://127.0.0.1/").expect("Failed to create Redis client");
//         RedisAdapter {
//             client: Arc::new(Mutex::new(client)),
//         }
//     }
// }
