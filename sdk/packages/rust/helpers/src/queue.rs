//! iii queue helpers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Result returned by the engine when a message is successfully enqueued.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EnqueueResult {
    #[serde(rename = "messageReceiptId")]
    pub message_receipt_id: String,
}
