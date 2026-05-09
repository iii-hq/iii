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
// VARIANT-ORDER-LOAD-BEARING: `Single` MUST precede `Segments` for serde
// untagged deserialization to route bare strings to `MergePath::Single`.
// Reordering breaks wire compatibility — string payloads would deserialize
// as one-element `Segments`. Locked by `merge_path_single_variant_deserializes_string_first`.
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

// Compatibility shim for callers that constructed paths via `FieldPath`
// before `Append.path` widened to `Option<MergePath>` in PR #1552-fix.
// `impl From<FieldPath> for Option<MergePath>` would violate Rust orphan
// rules (both `From` and `Option` are foreign); call sites needing the
// `Option` wrapping use `.map(Into::into)` or `Some(fp.into())`.
impl From<FieldPath> for MergePath {
    fn from(value: FieldPath) -> Self {
        Self::Single(value.0)
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<MergePath>,
        value: Value,
    },

    /// Increment numeric value
    Increment { path: FieldPath, by: i64 },

    /// Decrement numeric value
    Decrement { path: FieldPath, by: i64 },

    /// Append an element to an array or concatenate a string at the
    /// optional path. Path may be omitted (root append), a single
    /// first-level key, or an array of literal segments for nested
    /// append. See [`MergePath`] for the variant shape.
    Append {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<MergePath>,
        value: Value,
    },

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

    /// Create an Append operation at a specific path. Accepts a single
    /// first-level key (`"foo"`) or any type that converts into
    /// [`MergePath`] (e.g. `Vec<String>` for nested paths).
    pub fn append(path: impl Into<MergePath>, value: impl Into<Value>) -> Self {
        Self::Append {
            path: Some(path.into()),
            value: value.into(),
        }
    }

    /// Create an Append operation at the root level (no path).
    pub fn append_root(value: impl Into<Value>) -> Self {
        Self::Append {
            path: None,
            value: value.into(),
        }
    }

    /// Create an Append operation at a nested path of literal segments.
    /// Convenience wrapper for `append(vec!["a", "b"], v)`.
    pub fn append_at_path<I, S>(segments: I, value: impl Into<Value>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Append {
            path: Some(MergePath::Segments(
                segments.into_iter().map(Into::into).collect(),
            )),
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

/// Per-op error reported by an atomic update operation.
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
            UpdateOp::Append {
                path: Some(MergePath::Single(s)),
                value,
            } => {
                assert_eq!(s, "chunks");
                assert_eq!(value, serde_json::json!({"text": "hello"}));
            }
            other => panic!("expected single-string append, got {other:?}"),
        }
    }

    #[test]
    fn append_with_segments_path_round_trips_as_array() {
        let op = UpdateOp::append_at_path(["entityId", "buffer"], serde_json::json!("chunk"));
        let encoded = serde_json::to_value(&op).unwrap();

        assert_eq!(
            encoded,
            serde_json::json!({
                "type": "append",
                "path": ["entityId", "buffer"],
                "value": "chunk",
            })
        );

        let decoded: UpdateOp = serde_json::from_value(encoded).unwrap();
        match decoded {
            UpdateOp::Append {
                path: Some(MergePath::Segments(segs)),
                value,
            } => {
                assert_eq!(segs, vec!["entityId", "buffer"]);
                assert_eq!(value, serde_json::json!("chunk"));
            }
            other => panic!("expected segments append, got {other:?}"),
        }
    }

    #[test]
    fn append_with_root_path_round_trips() {
        let op = UpdateOp::append_root(serde_json::json!("first"));
        let encoded = serde_json::to_value(&op).unwrap();

        // `path` is None, so it is omitted entirely from the JSON
        // wire format (no explicit `null`). Cross-SDK consumers (Node
        // / Python / browser) decode the `path?` field as absent.
        assert_eq!(
            encoded,
            serde_json::json!({
                "type": "append",
                "value": "first",
            })
        );

        let decoded: UpdateOp = serde_json::from_value(encoded).unwrap();
        match decoded {
            UpdateOp::Append { path: None, value } => {
                assert_eq!(value, serde_json::json!("first"));
            }
            other => panic!("expected root append, got {other:?}"),
        }
    }

    #[test]
    fn append_path_omitted_deserializes_as_none() {
        // Wire payloads that pre-date this change may omit `path` entirely
        // (effectively root append). Guard against that breaking. Also
        // covers explicit `null` and missing-field deserialization paths.
        for raw in [
            r#"{"type":"append","value":"x"}"#,
            r#"{"type":"append","path":null,"value":"x"}"#,
        ] {
            let op: UpdateOp = serde_json::from_str(raw).unwrap_or_else(|e| {
                panic!("expected to parse {raw:?} as UpdateOp::Append, got {e}")
            });
            match op {
                UpdateOp::Append { path: None, value } => {
                    assert_eq!(value, serde_json::json!("x"));
                }
                other => panic!("expected root append for {raw:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn append_field_path_into_merge_path_compat() {
        // Legacy callers constructed paths via `FieldPath`; the
        // `From<FieldPath> for MergePath` shim keeps them compiling.
        let fp = FieldPath::new("legacy");
        let mp: MergePath = fp.into();
        assert_eq!(mp, MergePath::Single("legacy".to_string()));
    }

    #[test]
    fn merge_path_single_variant_deserializes_string_first() {
        // Regression for VARIANT-ORDER-LOAD-BEARING: bare JSON strings
        // must deserialize to `MergePath::Single`, not a one-element
        // `Segments`. If the variant order in `MergePath` is ever
        // reordered (alphabetized, auto-formatted) this test fails.
        let single: MergePath = serde_json::from_str(r#""foo""#).unwrap();
        assert_eq!(single, MergePath::Single("foo".to_string()));

        let segments: MergePath = serde_json::from_str(r#"["a","b"]"#).unwrap();
        assert_eq!(
            segments,
            MergePath::Segments(vec!["a".to_string(), "b".to_string()])
        );
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

        // `path` is None, so it is omitted from the JSON wire format
        // (no explicit `null`). Cross-SDK consumers decode `path?` as
        // absent. `path: null` payloads still deserialize via the
        // `#[serde(default)]` attribute.
        assert_eq!(
            encoded,
            serde_json::json!({
                "type": "merge",
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
                doc_url: Some("https://iii.dev/docs/workers/iii-state#merge-bounds".to_string()),
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
