//! iii queue helpers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Result returned when a function is called with `CallAction::Enqueue`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EnqueueResult {
    /// Unique receipt ID for the enqueued message.
    #[serde(rename = "messageReceiptId")]
    pub message_receipt_id: String,
}
