use redis::{AsyncCommands, aio::ConnectionManager};
use tokio::sync::MutexGuard;

use crate::modules::streams::StreamWrapperMessage;

pub const STREAM_TOPIC: &str = "stream::events";

pub async fn emit_event<'a>(
    mut conn: MutexGuard<'a, ConnectionManager>,
    message: StreamWrapperMessage,
) {
    tracing::debug!(msg = ?message, "Emitting event to Redis");

    let event_json = match serde_json::to_string(&message) {
        Ok(json) => json,
        Err(e) => {
            tracing::error!(error = %e, "Failed to serialize event data");
            return;
        }
    };

    if let Err(e) = conn.publish::<_, _, ()>(&STREAM_TOPIC, &event_json).await {
        tracing::error!(error = %e, "Failed to publish event to Redis");
    } else {
        tracing::debug!("Event published to Redis");
    }
}
