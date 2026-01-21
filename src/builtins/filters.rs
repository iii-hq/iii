use serde_json::Value;
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldPath(pub String);

impl From<&str> for FieldPath {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone)]
pub enum UpdateOp {
    /// Set a value at path (overwrite)
    Set { path: FieldPath, value: Value },

    /// Merge object into existing value
    /// (object-only; backend should reject non-objects)
    Merge {
        path: Option<FieldPath>, // usually root: ""
        value: Value,
    },

    /// Increment numeric value
    Increment { path: FieldPath, by: i64 },

    /// Decrement numeric value
    Decrement { path: FieldPath, by: i64 },

    /// Remove a field
    Remove { path: FieldPath },
}
