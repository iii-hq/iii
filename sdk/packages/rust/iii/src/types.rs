use std::{collections::HashMap, sync::Arc};

use futures_util::future::BoxFuture;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    channels::{ChannelReader, ChannelWriter, StreamChannelRef},
    error::IIIError,
    protocol::{RegisterFunctionMessage, RegisterTriggerTypeMessage},
    triggers::TriggerHandler,
};

pub type RemoteFunctionHandler =
    Arc<dyn Fn(Value) -> BoxFuture<'static, Result<Value, IIIError>> + Send + Sync>;

// ============================================================================
// Stream Update Types
// ============================================================================

/// Represents a path to a field in a JSON object
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct FieldPath(pub String);

impl FieldPath {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    pub fn root() -> Self {
        Self(String::new())
    }
}

impl From<&str> for FieldPath {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for FieldPath {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Path target for a [`UpdateOp::Merge`] operation. Accepts either a
/// single string (legacy / first-level field) or an array of literal
/// segments (nested path).
///
/// Path normalization rules applied by the engine:
/// - absent / `Single("")` / `Segments(vec![])` → root merge
/// - `Single("foo")` is equivalent to `Segments(vec!["foo".into()])`
/// - `Segments(["a", "b", "c"])` walks three literal keys, never
///   interpreting dots specially. `Segments(vec!["a.b".into()])` is a
///   single literal key named `"a.b"`.
///
/// **Variant ordering is load-bearing.** `#[serde(untagged)]` tries
/// variants in declaration order — `Single` MUST come before
/// `Segments` so a JSON string deserializes into `Single` rather than
/// failing the array match first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MergePath {
    Single(String),
    Segments(Vec<String>),
}

impl From<&str> for MergePath {
    fn from(value: &str) -> Self {
        Self::Single(value.to_string())
    }
}

impl From<String> for MergePath {
    fn from(value: String) -> Self {
        Self::Single(value)
    }
}

impl From<Vec<String>> for MergePath {
    fn from(value: Vec<String>) -> Self {
        Self::Segments(value)
    }
}

impl From<Vec<&str>> for MergePath {
    fn from(value: Vec<&str>) -> Self {
        Self::Segments(value.into_iter().map(String::from).collect())
    }
}

/// Operations that can be performed atomically on a stream value
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UpdateOp {
    /// Set a value at path (overwrite)
    Set {
        path: FieldPath,
        value: Option<Value>,
    },

    /// Merge object into existing value (object-only). Path may be
    /// omitted (root merge), a single first-level key, or an array of
    /// literal segments for nested merge. See [`MergePath`].
    Merge {
        path: Option<MergePath>,
        value: Value,
    },

    /// Increment numeric value
    Increment { path: FieldPath, by: i64 },

    /// Decrement numeric value
    Decrement { path: FieldPath, by: i64 },

    /// Append an element to an array or concatenate a string
    Append { path: FieldPath, value: Value },

    /// Remove a field
    Remove { path: FieldPath },
}

impl UpdateOp {
    /// Create a Set operation
    pub fn set(path: impl Into<FieldPath>, value: impl Into<Option<Value>>) -> Self {
        Self::Set {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create an Increment operation
    pub fn increment(path: impl Into<FieldPath>, by: i64) -> Self {
        Self::Increment {
            path: path.into(),
            by,
        }
    }

    /// Create a Decrement operation
    pub fn decrement(path: impl Into<FieldPath>, by: i64) -> Self {
        Self::Decrement {
            path: path.into(),
            by,
        }
    }

    /// Create an Append operation
    pub fn append(path: impl Into<FieldPath>, value: impl Into<Value>) -> Self {
        Self::Append {
            path: path.into(),
            value: value.into(),
        }
    }

    /// Create a Remove operation
    pub fn remove(path: impl Into<FieldPath>) -> Self {
        Self::Remove { path: path.into() }
    }

    /// Create a Merge operation at root level
    pub fn merge(value: impl Into<Value>) -> Self {
        Self::Merge {
            path: None,
            value: value.into(),
        }
    }

    /// Create a Merge operation at a specific path. Accepts a single
    /// first-level key (`"foo"`) or any type that converts into
    /// [`MergePath`] (e.g. `Vec<String>` for nested paths).
    pub fn merge_at(path: impl Into<MergePath>, value: impl Into<Value>) -> Self {
        Self::Merge {
            path: Some(path.into()),
            value: value.into(),
        }
    }

    /// Create a Merge operation at a nested path of literal segments.
    /// Convenience wrapper for `merge_at(vec!["a", "b"], v)`.
    pub fn merge_at_path<I, S>(segments: I, value: impl Into<Value>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Merge {
            path: Some(MergePath::Segments(
                segments.into_iter().map(Into::into).collect(),
            )),
            value: value.into(),
        }
    }
}

/// Per-op error reported by an atomic update operation. Currently
/// emitted only for the `merge` op when input violates the new
/// validation bounds (depth/size/proto-pollution); the other ops
/// retain warn-and-skip semantics for backward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateOpError {
    /// Index of the offending op within the original `ops` array.
    pub op_index: usize,
    /// Stable error code, e.g. `"merge.path.too_deep"`.
    pub code: String,
    /// Human-readable description with concrete numbers when applicable.
    pub message: String,
    /// Optional documentation URL for this error class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_url: Option<String>,
}

/// Result of an atomic update operation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateResult {
    /// The value before the update (None if key didn't exist)
    pub old_value: Option<Value>,
    /// The value after the update
    pub new_value: Value,
    /// Errors encountered while applying ops. Successfully applied ops
    /// are still reflected in `new_value`. Field is omitted from JSON
    /// when empty for backward compatibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<UpdateOpError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetResult {
    /// The value before the update (None if key didn't exist)
    pub old_value: Option<Value>,
    /// The value after the update
    pub new_value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteResult {
    /// The value before the update (None if key didn't exist)
    pub old_value: Option<Value>,
}

// ============================================================================
// Stream Input Types
// ============================================================================

/// Input for retrieving a single stream item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamGetInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
}

/// Input for setting a stream item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSetInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
    pub data: Value,
}

/// Input for deleting a stream item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDeleteInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
}

/// Input for listing all items in a stream group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamListInput {
    pub stream_name: String,
    pub group_id: String,
}

/// Input for listing all groups in a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamListGroupsInput {
    pub stream_name: String,
}

/// Input for atomically updating a stream item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamUpdateInput {
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
    pub ops: Vec<UpdateOp>,
}

// ============================================================================
// Stream Auth Types
// ============================================================================

/// Input for stream authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamAuthInput {
    pub headers: HashMap<String, String>,
    pub path: String,
    pub query_params: HashMap<String, Vec<String>>,
    pub addr: String,
}

/// Result of stream authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamAuthResult {
    pub context: Option<Value>,
}

/// Result of a stream join request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamJoinResult {
    pub unauthorized: bool,
}

#[derive(Clone)]
pub struct RemoteFunctionData {
    pub message: RegisterFunctionMessage,
    pub handler: Option<RemoteFunctionHandler>,
}

#[derive(Clone)]
pub struct RemoteTriggerTypeData {
    pub message: RegisterTriggerTypeMessage,
    pub handler: Arc<dyn TriggerHandler>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRequest<T = Value> {
    #[serde(default)]
    pub query_params: HashMap<String, String>,
    #[serde(default)]
    pub path_params: HashMap<String, String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub body: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T = Value> {
    pub status_code: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body: T,
}

/// A streaming channel pair for worker-to-worker data transfer.
pub struct Channel {
    pub writer: ChannelWriter,
    pub reader: ChannelReader,
    pub writer_ref: StreamChannelRef,
    pub reader_ref: StreamChannelRef,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_request_defaults_when_missing_fields() {
        let request: ApiRequest = serde_json::from_str("{}").unwrap();

        assert!(request.query_params.is_empty());
        assert!(request.path_params.is_empty());
        assert!(request.headers.is_empty());
        assert_eq!(request.path, "");
        assert_eq!(request.method, "");
        assert!(request.body.is_null());
    }

    #[test]
    fn update_append_serializes_as_tagged_operation() {
        let op = UpdateOp::append("chunks", serde_json::json!({"text": "hello"}));
        let encoded = serde_json::to_value(&op).unwrap();

        assert_eq!(
            encoded,
            serde_json::json!({
                "type": "append",
                "path": "chunks",
                "value": {"text": "hello"},
            })
        );

        let decoded: UpdateOp = serde_json::from_value(encoded).unwrap();
        match decoded {
            UpdateOp::Append { path, value } => {
                assert_eq!(path.0, "chunks");
                assert_eq!(value, serde_json::json!({"text": "hello"}));
            }
            other => panic!("expected append op, got {other:?}"),
        }
    }

    #[test]
    fn merge_with_string_path_round_trips_to_single_variant() {
        // Regression: Single must come before Segments in the
        // untagged enum or this test fails.
        let op = UpdateOp::merge_at("session-abc", serde_json::json!({"author": "alice"}));
        let encoded = serde_json::to_value(&op).unwrap();

        assert_eq!(
            encoded,
            serde_json::json!({
                "type": "merge",
                "path": "session-abc",
                "value": {"author": "alice"},
            })
        );

        let decoded: UpdateOp = serde_json::from_value(encoded).unwrap();
        match decoded {
            UpdateOp::Merge {
                path: Some(MergePath::Single(s)),
                value,
            } => {
                assert_eq!(s, "session-abc");
                assert_eq!(value, serde_json::json!({"author": "alice"}));
            }
            other => panic!("expected single-string merge, got {other:?}"),
        }
    }

    #[test]
    fn merge_with_segments_path_round_trips_as_array() {
        let op = UpdateOp::merge_at_path(["sessions", "abc"], serde_json::json!({"ts": "chunk"}));
        let encoded = serde_json::to_value(&op).unwrap();

        assert_eq!(
            encoded,
            serde_json::json!({
                "type": "merge",
                "path": ["sessions", "abc"],
                "value": {"ts": "chunk"},
            })
        );

        let decoded: UpdateOp = serde_json::from_value(encoded).unwrap();
        match decoded {
            UpdateOp::Merge {
                path: Some(MergePath::Segments(segs)),
                value,
            } => {
                assert_eq!(segs, vec!["sessions", "abc"]);
                assert_eq!(value, serde_json::json!({"ts": "chunk"}));
            }
            other => panic!("expected segments merge, got {other:?}"),
        }
    }

    #[test]
    fn merge_without_path_round_trips() {
        let op = UpdateOp::merge(serde_json::json!({"x": 1}));
        let encoded = serde_json::to_value(&op).unwrap();

        // path is None, so it serializes as null.
        assert_eq!(
            encoded,
            serde_json::json!({
                "type": "merge",
                "path": null,
                "value": {"x": 1},
            })
        );

        let decoded: UpdateOp = serde_json::from_value(encoded).unwrap();
        match decoded {
            UpdateOp::Merge { path: None, value } => {
                assert_eq!(value, serde_json::json!({"x": 1}));
            }
            other => panic!("expected root merge, got {other:?}"),
        }
    }

    #[test]
    fn update_result_with_errors_serializes_field() {
        let result = UpdateResult {
            old_value: None,
            new_value: serde_json::json!({"a": 1}),
            errors: vec![UpdateOpError {
                op_index: 0,
                code: "merge.path.too_deep".to_string(),
                message: "Path depth 33 exceeds maximum of 32".to_string(),
                doc_url: Some("https://docs.iii.dev/workers/iii-state#merge-bounds".to_string()),
            }],
        };
        let encoded = serde_json::to_value(&result).unwrap();
        assert_eq!(encoded["errors"][0]["code"], "merge.path.too_deep");
    }

    #[test]
    fn update_result_without_errors_omits_field_from_json() {
        let result = UpdateResult {
            old_value: None,
            new_value: serde_json::json!({"a": 1}),
            errors: vec![],
        };
        let encoded = serde_json::to_value(&result).unwrap();
        assert!(
            encoded.get("errors").is_none(),
            "errors field should be omitted when empty for backward compat"
        );
    }
}
