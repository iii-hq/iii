//! Stream update builder.
//!
//! # Example
//!
//! ```rust,ignore
//! use iii_sdk::{UpdateBuilder, UpdateOp};
//!
//! let ops = UpdateBuilder::new()
//!     .increment("counter", 1)
//!     .set("status", serde_json::json!("active"))
//!     .build();
//! ```

use crate::types::UpdateOp;

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
