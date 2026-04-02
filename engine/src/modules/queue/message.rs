// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde_json::Value;

/// Common message abstraction between adapter and consumer loop.
/// Adapters translate their native message format into this struct.
pub struct QueueMessage {
    /// Adapter-specific tag for ack/nack (e.g., RabbitMQ delivery tag)
    pub delivery_id: u64,
    /// The function to invoke
    pub function_id: String,
    /// The payload to pass to the function
    pub data: Value,
    /// Current attempt number (derived from transport-native retry count)
    pub attempt: u32,
    /// Publisher-assigned message ID for tracking and cancellation
    pub message_id: Option<String>,
    /// W3C traceparent header for distributed tracing
    pub traceparent: Option<String>,
    /// W3C baggage header for context propagation
    pub baggage: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn queue_message_fields_accessible() {
        let msg = QueueMessage {
            delivery_id: 42,
            function_id: "functions.process_order".to_string(),
            data: json!({"order_id": "o-1"}),
            attempt: 0,
            message_id: Some("msg-123".to_string()),
            traceparent: Some("00-abc-def-01".to_string()),
            baggage: Some("queue=orders".to_string()),
        };

        assert_eq!(msg.delivery_id, 42);
        assert_eq!(msg.function_id, "functions.process_order");
        assert_eq!(msg.data["order_id"], "o-1");
        assert_eq!(msg.attempt, 0);
        assert!(msg.traceparent.is_some());
        assert!(msg.baggage.is_some());
    }

    #[test]
    fn queue_message_optional_tracing_fields() {
        let msg = QueueMessage {
            delivery_id: 1,
            function_id: "fn".to_string(),
            data: json!(null),
            attempt: 3,
            message_id: None,
            traceparent: None,
            baggage: None,
        };

        assert!(msg.traceparent.is_none());
        assert!(msg.baggage.is_none());
        assert_eq!(msg.attempt, 3);
    }
}
