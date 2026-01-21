//! Stream operations for atomic updates
//!
//! This module provides functionality for performing atomic updates on stream data.
//!
//! # Example
//!
//! ```rust,ignore
//! use iii::{Bridge, Streams, UpdateOp};
//!
//! let bridge = Bridge::new("ws://localhost:49134");
//! bridge.connect().await?;
//!
//! let streams = Streams::new(bridge.clone());
//!
//! // Atomic update with multiple operations
//! let result = streams.update(
//!     "my-stream::group-1::item-1",
//!     vec![
//!         UpdateOp::increment("counter", 1),
//!         UpdateOp::set("lastUpdated", serde_json::json!("2024-01-01")),
//!     ],
//! ).await?;
//!
//! println!("Old value: {:?}", result.old_value);
//! println!("New value: {:?}", result.new_value);
//! ```

use crate::{
    bridge::Bridge,
    error::BridgeError,
    types::{StreamUpdateInput, UpdateOp, UpdateResult},
};

/// Provides atomic stream update operations
#[derive(Clone)]
pub struct Streams {
    bridge: Bridge,
}

impl Streams {
    /// Create a new Streams instance with the given bridge
    pub fn new(bridge: Bridge) -> Self {
        Self { bridge }
    }

    /// Perform an atomic update on a stream key
    ///
    /// All operations are applied atomically - either all succeed or none are applied.
    ///
    /// # Arguments
    ///
    /// * `key` - The stream key to update (format: "stream_name::group_id::item_id")
    /// * `ops` - List of operations to apply atomically
    ///
    /// # Returns
    ///
    /// Returns `UpdateResult` containing the old and new values
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = streams.update(
    ///     "orders::user-123::order-456",
    ///     vec![
    ///         UpdateOp::increment("total", 100),
    ///         UpdateOp::set("status", "processing".into()),
    ///     ],
    /// ).await?;
    /// ```
    pub async fn update(
        &self,
        key: impl Into<String>,
        ops: Vec<UpdateOp>,
    ) -> Result<UpdateResult, BridgeError> {
        let input = StreamUpdateInput {
            key: key.into(),
            ops,
        };

        let result = self.bridge.invoke_function("streams.update", input).await?;

        serde_json::from_value(result).map_err(|e| BridgeError::Serde(e.to_string()))
    }

    /// Atomically increment a numeric field
    ///
    /// Convenience method for a single increment operation.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// streams.increment("counters::daily::page-views", "count", 1).await?;
    /// ```
    pub async fn increment(
        &self,
        key: impl Into<String>,
        field: impl Into<String>,
        by: i64,
    ) -> Result<UpdateResult, BridgeError> {
        self.update(key, vec![UpdateOp::increment(field.into(), by)])
            .await
    }

    /// Atomically decrement a numeric field
    ///
    /// Convenience method for a single decrement operation.
    pub async fn decrement(
        &self,
        key: impl Into<String>,
        field: impl Into<String>,
        by: i64,
    ) -> Result<UpdateResult, BridgeError> {
        self.update(key, vec![UpdateOp::decrement(field.into(), by)])
            .await
    }

    /// Atomically set a field value
    ///
    /// Convenience method for a single set operation.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// streams.set_field("users::active::user-1", "status", "online".into()).await?;
    /// ```
    pub async fn set_field(
        &self,
        key: impl Into<String>,
        field: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Result<UpdateResult, BridgeError> {
        self.update(key, vec![UpdateOp::set(field.into(), value.into())])
            .await
    }

    /// Atomically remove a field
    ///
    /// Convenience method for a single remove operation.
    pub async fn remove_field(
        &self,
        key: impl Into<String>,
        field: impl Into<String>,
    ) -> Result<UpdateResult, BridgeError> {
        self.update(key, vec![UpdateOp::remove(field.into())]).await
    }

    /// Atomically merge an object into the existing value
    ///
    /// Convenience method for a single merge operation.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// streams.merge(
    ///     "settings::user-1::preferences",
    ///     serde_json::json!({"theme": "dark", "language": "en"}),
    /// ).await?;
    /// ```
    pub async fn merge(
        &self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Result<UpdateResult, BridgeError> {
        self.update(key, vec![UpdateOp::merge(value.into())]).await
    }
}

/// Builder for creating multiple update operations
#[derive(Debug, Clone, Default)]
pub struct UpdateBuilder {
    ops: Vec<UpdateOp>,
}

impl UpdateBuilder {
    /// Create a new UpdateBuilder
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a set operation
    pub fn set(mut self, path: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.ops.push(UpdateOp::set(path.into(), value.into()));
        self
    }

    /// Add an increment operation
    pub fn increment(mut self, path: impl Into<String>, by: i64) -> Self {
        self.ops.push(UpdateOp::increment(path.into(), by));
        self
    }

    /// Add a decrement operation
    pub fn decrement(mut self, path: impl Into<String>, by: i64) -> Self {
        self.ops.push(UpdateOp::decrement(path.into(), by));
        self
    }

    /// Add a remove operation
    pub fn remove(mut self, path: impl Into<String>) -> Self {
        self.ops.push(UpdateOp::remove(path.into()));
        self
    }

    /// Add a merge operation
    pub fn merge(mut self, value: impl Into<serde_json::Value>) -> Self {
        self.ops.push(UpdateOp::merge(value.into()));
        self
    }

    /// Build the list of operations
    pub fn build(self) -> Vec<UpdateOp> {
        self.ops
    }
}
