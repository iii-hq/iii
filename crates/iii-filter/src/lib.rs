//! The iii observer filter language (`iii-filter` v1).
//!
//! A filter is the `filters` object of an observer's source. It decides
//! whether one invocation is delivered to one observer, and runs in the
//! engine and in every SDK, which must all agree on every input. The
//! language is small, total and side-effect free:
//!
//! ```
//! use iii_filter::{Doc, Filter};
//! use serde_json::json;
//!
//! let filter = Filter::parse(&json!({
//!     "input.op": { "$in": ["insert", "update"] },
//!     "caller.worker_name": { "$ne": "indexer" },
//! }))
//! .unwrap();
//!
//! let input = json!({ "op": "insert" });
//! assert!(filter.matches(&Doc { input: Some(&input), ..Doc::default() }));
//! assert!(!filter.matches_value(&json!({ "input": { "op": "delete" } })));
//! ```
//!
//! The language, its semantics and the shared conformance vectors are
//! specified in `tech-specs/2026-10-13-observe/filters.md` and
//! `sdk/fixtures/filters/README.md` in the iii repository.

mod error;
mod eval;
mod index;
mod parse;
mod path;
mod pmap;
mod schema;

use std::fmt;
use std::sync::{Arc, OnceLock};

use serde_json::Value;

pub use error::{ErrorCode, FilterError};
pub use index::{FilterIndex, Matches};

use crate::parse::SizeLimit;

/// Validation limits (filters.md §4). `sdk/fixtures/filters/README.md`
/// defines exactly what each one counts.
pub mod limits {
    /// Filter nesting: `$or` / `$not` / `$any` operands open a level, `$and` does not.
    pub const MAX_DEPTH: usize = 8;
    /// Operators (`$eq` ... `$any`) plus `$or` / `$not` combinators.
    pub const MAX_NODES: usize = 64;
    /// Items of one `$in` / `$nin` array.
    pub const MAX_SET_ITEMS: usize = 256;
    /// Segments after the root of one path.
    pub const MAX_PATH_SEGMENTS: usize = 16;
    /// Compact JSON encoding (UTF-8 bytes) of the filter as written.
    pub const MAX_ENCODED_BYTES: usize = 8 * 1024;
    /// Arrays / objects nested in one `$eq` / `$ne` operand or `$in` / `$nin` item.
    pub const MAX_LITERAL_DEPTH: usize = 32;
    /// Arrays / objects nested in the whole filter value, `$and` wrappers and
    /// literals included (`{}` is 1). The engine's JSON decoder stops at 128
    /// levels per message, and a filter sits a few levels inside it.
    pub const MAX_JSON_DEPTH: usize = 120;
    /// Numeric literals must lie within `±MAX_SAFE_INTEGER` (2^53).
    pub const MAX_SAFE_INTEGER: u64 = 1 << 53;
}

/// The document roots a filter references.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Roots {
    pub input: bool,
    pub output: bool,
    pub error: bool,
    pub caller: bool,
    pub metadata: bool,
}

impl Roots {
    /// No root referenced: the filter is a constant (`{}` and its variants).
    pub fn is_empty(&self) -> bool {
        !(self.input || self.output || self.error || self.caller || self.metadata)
    }
}

/// The document a filter is evaluated against, borrowed so that the engine
/// and the SDKs never clone a payload to evaluate it. `None` is *absent*;
/// `Some(&Value::Null)` is the value `null`.
#[derive(Default, Clone, Copy, Debug)]
pub struct Doc<'a> {
    pub input: Option<&'a Value>,
    pub output: Option<&'a Value>,
    pub error: Option<&'a Value>,
    pub caller: Option<&'a Value>,
    pub metadata: Option<&'a Value>,
}

impl<'a> Doc<'a> {
    /// A view of `{ "input"?, "output"?, "error"?, "caller"?, "metadata"? }`.
    /// Other keys are ignored; a non-object has every root absent.
    pub fn from_value(value: &'a Value) -> Doc<'a> {
        let Value::Object(map) = value else {
            return Doc::default();
        };
        Doc {
            input: map.get("input"),
            output: map.get("output"),
            error: map.get("error"),
            caller: map.get("caller"),
            metadata: map.get("metadata"),
        }
    }
}

/// A validated, normalized and compiled filter. Cheap to clone.
#[derive(Clone)]
pub struct Filter {
    inner: Arc<Inner>,
}

struct Inner {
    normalized: Value,
    node: eval::Node,
    roots: Roots,
}

impl Filter {
    /// Validates (limits, roots, operators, no literal shorthand), normalizes
    /// and compiles `src`. Errors are always [`ErrorCode::InvalidFilter`].
    pub fn parse(src: &Value) -> Result<Filter, FilterError> {
        parse::parse(src, SizeLimit::Enforce).map(Filter::from_parsed)
    }

    /// Compiles a filter in the normal form the engine sends to workers
    /// (`functionregistered`, `observeradded`). The engine validated it when
    /// the observer was created. Every rule of [`Filter::parse`] applies
    /// except the encoded-size limit, which is defined on the filter as the
    /// observer wrote it: its normal form can be a few bytes longer.
    ///
    /// For a normal form, `normalized()` returns the input unchanged.
    pub fn from_normalized(normalized: &Value) -> Result<Filter, FilterError> {
        parse::parse(normalized, SizeLimit::Skip).map(Filter::from_parsed)
    }

    fn from_parsed(parsed: parse::Parsed) -> Filter {
        let mut roots = Roots::default();
        parsed.node.collect_roots(&mut roots);
        Filter {
            inner: Arc::new(Inner {
                normalized: parsed.normalized,
                node: parsed.node,
                roots,
            }),
        }
    }

    /// `{}`: matches every document.
    pub fn match_all() -> Filter {
        static MATCH_ALL: OnceLock<Filter> = OnceLock::new();
        MATCH_ALL
            .get_or_init(|| Filter::parse(&Value::Object(Default::default())).expect("{} is valid"))
            .clone()
    }

    /// True when the normal form is `{}`.
    pub fn is_match_all(&self) -> bool {
        self.inner.node.is_match_all()
    }

    /// The normal form (filters.md §5), as carried by `functionregistered`
    /// and `observeradded`.
    pub fn normalized(&self) -> &Value {
        &self.inner.normalized
    }

    /// The roots any clause references.
    pub fn roots(&self) -> Roots {
        self.inner.roots
    }

    /// Evaluates the filter. Total: never fails, never panics.
    #[inline]
    pub fn matches(&self, doc: &Doc<'_>) -> bool {
        self.inner.node.matches(doc, None)
    }

    /// Evaluates against `{ "input"?, "output"?, "error"?, "caller"?, "metadata"? }`.
    pub fn matches_value(&self, doc: &Value) -> bool {
        self.matches(&Doc::from_value(doc))
    }

    /// The `$eq` / `$in` conditions of the top-level `$and` (see
    /// [`FilterIndex`]).
    pub(crate) fn equality_terms(&self) -> impl Iterator<Item = (&path::Path, &[Value])> {
        self.inner.node.equality_terms()
    }

    /// Checks `input.*` paths against `request_format` and `output.*` paths
    /// against `response_format` (filters.md §4). Returns the warnings for
    /// paths that could not be verified, or [`ErrorCode::InvalidFilterPath`]
    /// for a path that a closed schema (`additionalProperties: false`) rules
    /// out.
    pub fn check_schema_paths(
        &self,
        request_format: Option<&Value>,
        response_format: Option<&Value>,
    ) -> Result<Vec<String>, FilterError> {
        schema::check(&self.inner.node, request_format, response_format)
    }
}

impl Default for Filter {
    fn default() -> Self {
        Filter::match_all()
    }
}

impl fmt::Debug for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Filter")
            .field(&self.inner.normalized)
            .finish()
    }
}

/// Compact JSON of the normal form.
impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner.normalized, f)
    }
}

/// Two filters are equal when their normal forms are.
impl PartialEq for Filter {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner) || self.inner.normalized == other.inner.normalized
    }
}

impl Eq for Filter {}

/// Serializes the normal form.
impl serde::Serialize for Filter {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.inner.normalized.serialize(serializer)
    }
}

/// Deserializes a filter the way [`Filter::from_normalized`] compiles one:
/// every rule except the encoded-size limit, so that what `Serialize` writes
/// (the normal form, which can exceed 8 KiB) always reads back. A filter
/// written by an observer must go through [`Filter::parse`] instead.
impl<'de> serde::Deserialize<'de> for Filter {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let src = Value::deserialize(deserializer)?;
        Filter::from_normalized(&src).map_err(serde::de::Error::custom)
    }
}

/// Validates `src` and returns its normal form (filters.md §5).
pub fn normalize(src: &Value) -> Result<Value, FilterError> {
    Filter::parse(src).map(|f| f.normalized().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn filter_is_send_sync() {
        assert_send_sync::<Filter>();
        assert_send_sync::<Doc<'static>>();
    }

    #[test]
    fn match_all() {
        let all = Filter::match_all();
        assert!(all.is_match_all());
        assert_eq!(all.normalized(), &serde_json::json!({}));
        assert!(all.roots().is_empty());
        assert!(all.matches(&Doc::default()));
        assert_eq!(Filter::default(), all);
    }

    #[test]
    fn roots_of_nested_clauses() {
        let f = Filter::parse(&serde_json::json!({
            "$or": [
                { "caller.parent.worker_name": { "$eq": "x" } },
                { "$not": { "metadata.k": { "$exists": true } } }
            ],
            "input.items": { "$any": { "@.sku": { "$eq": "a" } } }
        }))
        .unwrap();
        assert_eq!(
            f.roots(),
            Roots {
                input: true,
                caller: true,
                metadata: true,
                ..Roots::default()
            }
        );
    }

    #[test]
    fn serde_round_trip() {
        let f: Filter = serde_json::from_str(r#"{"input.a":{"$eq":1}}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&f).unwrap(),
            r#"{"$and":[{"input.a":{"$eq":1}}]}"#
        );
        assert!(serde_json::from_str::<Filter>(r#"{"input.a":1}"#).is_err());
        assert_eq!(f.to_string(), r#"{"$and":[{"input.a":{"$eq":1}}]}"#);
    }

    #[test]
    fn serde_round_trip_of_a_normal_form_over_the_size_limit() {
        // 8 KiB as written; the normal form is longer.
        let mut src = serde_json::Map::new();
        for i in 0..32 {
            src.insert(
                format!("input.k{i:02}"),
                serde_json::json!({ "$eq": "v".repeat(232) }),
            );
        }
        let filter = Filter::parse(&Value::Object(src)).unwrap();
        let encoded = serde_json::to_string(&filter).unwrap();
        assert!(encoded.len() > limits::MAX_ENCODED_BYTES);
        let decoded: Filter = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, filter);
        assert_eq!(decoded.normalized(), filter.normalized());
    }

    #[test]
    fn integer_literals_are_judged_on_their_exact_value() {
        // A double-based parser reads these as ±2^53 (valid); the engine sees
        // the JSON text, and the exact values are beyond ±2^53.
        for text in [
            r#"{"input.a":{"$eq":9007199254740993}}"#,
            r#"{"input.a":{"$eq":-9007199254740993}}"#,
            r#"{"input.a":{"$in":[9007199254740993]}}"#,
            r#"{"input.a":{"$gt":-9007199254740993}}"#,
        ] {
            let src: Value = serde_json::from_str(text).unwrap();
            let err = Filter::parse(&src).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidFilter, "{text}");
        }
        for text in [
            r#"{"input.a":{"$eq":9007199254740992}}"#,
            r#"{"input.a":{"$eq":-9007199254740992}}"#,
        ] {
            let src: Value = serde_json::from_str(text).unwrap();
            assert!(Filter::parse(&src).is_ok(), "{text}");
        }
    }

    #[test]
    fn error_display() {
        let err = Filter::parse(&serde_json::json!({ "input.op": "insert" })).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidFilter);
        assert_eq!(err.code.as_str(), "INVALID_FILTER");
        assert!(err.to_string().starts_with("INVALID_FILTER: "));
    }
}
