//! Schema path checks (filters.md §4): `input.*` paths against the
//! function's `request_format`, `output.*` paths against its
//! `response_format`. Other roots are not checked.
//!
//! The walk follows `properties` (and `items` for index segments). A path
//! that leaves the schema where `additionalProperties` is `false` is
//! `INVALID_FILTER_PATH`. Where the schema is open, or uses something this
//! walk does not resolve (a remote `$ref`, `anyOf` / `oneOf` / `allOf` with
//! several branches, `patternProperties`, `if`, ...), the path is accepted with
//! a warning. Local `$ref`s (`#/...`), single-branch `allOf` and nullable
//! `anyOf` / `oneOf` (one branch plus `{"type": "null"}`) are followed, since
//! schema generators emit them for plain structs and optional fields.
//! `@` paths inside `$any` are checked against the array's `items`.

use serde_json::{Map, Value};

use crate::error::FilterError;
use crate::eval::{AnyOperand, Node, Op};
use crate::path::{Root, Segment};

/// How many `$ref` / wrapper hops to follow before giving up.
const MAX_HOPS: usize = 32;

/// Keywords that can admit a property this walk cannot see.
const UNRESOLVED: [&str; 7] = [
    "patternProperties",
    "anyOf",
    "oneOf",
    "allOf",
    "if",
    "dependentSchemas",
    "unevaluatedProperties",
];

/// Keywords that shape where a value can be, used to tell a plain `$ref`
/// (or wrapper) from one with sibling constraints.
const STRUCTURAL: [&str; 17] = [
    "properties",
    "additionalProperties",
    "items",
    "prefixItems",
    "additionalItems",
    "patternProperties",
    "anyOf",
    "oneOf",
    "allOf",
    "not",
    "if",
    "then",
    "else",
    "dependentSchemas",
    "unevaluatedProperties",
    "$ref",
    "$dynamicRef",
];

pub(crate) fn check(
    node: &Node,
    request_format: Option<&Value>,
    response_format: Option<&Value>,
) -> Result<Vec<String>, FilterError> {
    let mut checker = Checker {
        request_format,
        response_format,
        warnings: Vec::new(),
    };
    checker.node(node, None)?;
    Ok(checker.warnings)
}

/// A position in a schema: the schema there (`None` once unverifiable), the
/// document its `$ref`s resolve against, and how to name it in messages.
#[derive(Clone)]
struct At<'s> {
    document: &'s Value,
    schema: Option<&'s Value>,
    display: String,
    format: &'static str,
}

enum Step<'s> {
    Next(&'s Value),
    NotInSchema,
    Unverified(Reason),
}

enum Reason {
    Open,
    Keyword(&'static str),
    Type(String),
}

struct Checker<'s> {
    request_format: Option<&'s Value>,
    response_format: Option<&'s Value>,
    warnings: Vec<String>,
}

impl<'s> Checker<'s> {
    /// `element` is the position of `@` inside `$any`; `None` at the top.
    fn node(&mut self, node: &Node, element: Option<&At<'s>>) -> Result<(), FilterError> {
        match node {
            Node::And(nodes) | Node::Or(nodes) => {
                for node in nodes.iter() {
                    self.node(node, element)?;
                }
            }
            Node::Not(node) => self.node(node, element)?,
            Node::Path(clause) => {
                let start = match clause.path.root {
                    Root::Input => self.request_format.map(|s| At {
                        document: s,
                        schema: Some(s),
                        display: "input".to_owned(),
                        format: "request_format",
                    }),
                    Root::Output => self.response_format.map(|s| At {
                        document: s,
                        schema: Some(s),
                        display: "output".to_owned(),
                        format: "response_format",
                    }),
                    Root::Element => element.map(|at| At {
                        display: format!("{}[*]", at.display),
                        ..at.clone()
                    }),
                    Root::Error | Root::Caller | Root::Metadata => None,
                };
                let Some(start) = start else {
                    return Ok(());
                };
                let at = self.walk(start, &clause.path.segments)?;
                self.ops(&clause.ops, &at)?;
            }
        }
        Ok(())
    }

    /// Checks the `$any` operands of an operator object at `at`.
    fn ops(&mut self, ops: &[Op], at: &At<'s>) -> Result<(), FilterError> {
        for op in ops {
            let Op::Any(operand) = op else { continue };
            // The array itself could not be verified: a warning was already
            // given for its path, the elements add nothing.
            let Some(schema) = at.schema else { continue };
            let items = match items_of(at.document, schema) {
                Ok(items) => Some(items),
                Err(reason) => {
                    self.element_warnings(operand, at, &reason);
                    None
                }
            };
            let element = At {
                document: at.document,
                schema: items,
                display: at.display.clone(),
                format: at.format,
            };
            match operand {
                AnyOperand::Ops(inner) => {
                    let element = At {
                        display: format!("{}[*]", at.display),
                        ..element
                    };
                    self.ops(inner, &element)?;
                }
                AnyOperand::Filter(node) => self.node(node, Some(&element))?,
            }
        }
        Ok(())
    }

    /// Follows `segments` from `at`, returning the position at the end.
    fn walk(&mut self, at: At<'s>, segments: &[Segment]) -> Result<At<'s>, FilterError> {
        let mut display = at.display.clone();
        let mut schema = at.schema;
        let mut unverified: Option<(String, Reason)> = None;
        for segment in segments {
            let prefix = display.clone();
            display.push('.');
            push_escaped(&mut display, &segment.key);
            let Some(current) = schema else { continue };
            match step(at.document, current, segment) {
                Step::Next(next) => schema = Some(next),
                Step::NotInSchema => {
                    return Err(FilterError::invalid_path(format!(
                        "{display} is not in {}",
                        at.format
                    )));
                }
                Step::Unverified(reason) => {
                    schema = None;
                    unverified = Some((prefix, reason));
                }
            }
        }
        if let Some((prefix, reason)) = unverified {
            self.warn(&display, at.format, &prefix, &reason);
        }
        Ok(At {
            document: at.document,
            schema,
            display,
            format: at.format,
        })
    }

    /// The array's element schema is unknown: every `@` path below it that
    /// goes somewhere gets a warning.
    fn element_warnings(&mut self, operand: &AnyOperand, at: &At<'s>, reason: &Reason) {
        let AnyOperand::Filter(node) = operand else {
            return;
        };
        let mut paths = Vec::new();
        element_paths(node, &mut paths);
        for segments in paths {
            let mut display = format!("{}[*]", at.display);
            for segment in segments {
                display.push('.');
                push_escaped(&mut display, &segment.key);
            }
            self.warn(&display, at.format, &at.display, reason);
        }
    }

    fn warn(&mut self, path: &str, format: &str, prefix: &str, reason: &Reason) {
        let root = path.split(['.', '[']).next().unwrap_or(path);
        let warning = match reason {
            Reason::Open if prefix == root => {
                format!("{path}: {format} is open; path not verified")
            }
            Reason::Open => format!("{path}: {format} is open at {prefix}; path not verified"),
            Reason::Keyword(keyword) => {
                format!("{path}: {format} uses `{keyword}` at {prefix}; path not verified")
            }
            Reason::Type(ty) => {
                format!("{path}: {format} declares {prefix} as {ty}; the path is always absent")
            }
        };
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }
}

/// The `@` paths with at least one segment directly inside `node` (nested
/// `$any` operands are reached through their own array path).
fn element_paths<'n>(node: &'n Node, out: &mut Vec<&'n [Segment]>) {
    match node {
        Node::And(nodes) | Node::Or(nodes) => nodes.iter().for_each(|n| element_paths(n, out)),
        Node::Not(node) => element_paths(node, out),
        Node::Path(clause) => {
            if clause.path.root == Root::Element && !clause.path.segments.is_empty() {
                out.push(&clause.path.segments);
            }
        }
    }
}

/// One segment down from `schema`. Reaching a `false` schema (nothing is
/// valid there) is `NotInSchema`, at the last segment too.
fn step<'s>(document: &'s Value, schema: &'s Value, segment: &Segment) -> Step<'s> {
    match select(document, schema, segment) {
        Step::Next(next) if matches!(resolve(document, next), Ok(Value::Bool(false))) => {
            Step::NotInSchema
        }
        step => step,
    }
}

fn select<'s>(document: &'s Value, schema: &'s Value, segment: &Segment) -> Step<'s> {
    let schema = match resolve(document, schema) {
        Ok(schema) => schema,
        Err(reason) => return Step::Unverified(reason),
    };
    let object = match schema {
        Value::Object(object) => object,
        Value::Bool(false) => return Step::NotInSchema,
        _ => return Step::Unverified(Reason::Open),
    };

    if let Some(next) = object
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(&*segment.key))
    {
        return Step::Next(next);
    }
    if segment.index.is_some() {
        match object.get("items") {
            Some(items @ (Value::Object(_) | Value::Bool(_))) => return Step::Next(items),
            Some(_) => return Step::Unverified(Reason::Keyword("items")),
            None => {}
        }
        if object.contains_key("prefixItems") {
            return Step::Unverified(Reason::Keyword("prefixItems"));
        }
    }
    if let Some(keyword) = UNRESOLVED.into_iter().find(|k| object.contains_key(*k)) {
        return Step::Unverified(Reason::Keyword(keyword));
    }
    match object.get("additionalProperties") {
        Some(Value::Bool(false)) => Step::NotInSchema,
        Some(next @ Value::Object(_)) => Step::Next(next),
        _ => match excluded_type(object, segment.index.is_some()) {
            Some(ty) => Step::Unverified(Reason::Type(ty)),
            None => Step::Unverified(Reason::Open),
        },
    }
}

/// The element schema of an array schema.
fn items_of<'s>(document: &'s Value, schema: &'s Value) -> Result<&'s Value, Reason> {
    let schema = resolve(document, schema)?;
    let Value::Object(object) = schema else {
        return Err(Reason::Open);
    };
    match object.get("items") {
        Some(items @ (Value::Object(_) | Value::Bool(_))) => Ok(items),
        Some(_) => Err(Reason::Keyword("items")),
        None if object.contains_key("prefixItems") => Err(Reason::Keyword("prefixItems")),
        None => match object.get("type") {
            Some(ty) if !declares(object, "array") => Err(Reason::Type(type_name(ty))),
            _ => Err(Reason::Open),
        },
    }
}

/// Follows local `$ref`s, single-branch `allOf` and nullable `anyOf` /
/// `oneOf` wrappers.
fn resolve<'s>(document: &'s Value, mut schema: &'s Value) -> Result<&'s Value, Reason> {
    for _ in 0..MAX_HOPS {
        let Value::Object(object) = schema else {
            return Ok(schema);
        };
        if let Some(reference) = object.get("$ref") {
            if has_structural_besides(object, "$ref") {
                return Err(Reason::Keyword("$ref"));
            }
            schema = reference
                .as_str()
                .and_then(|r| local_pointer(document, r))
                .ok_or(Reason::Keyword("$ref"))?;
            continue;
        }
        if object.contains_key("$dynamicRef") {
            return Err(Reason::Keyword("$dynamicRef"));
        }
        if let Some(Value::Array(branches)) = object.get("allOf") {
            if branches.len() == 1 && !has_structural_besides(object, "allOf") {
                schema = &branches[0];
                continue;
            }
        }
        let nullable = ["anyOf", "oneOf"].into_iter().find_map(|keyword| {
            let Some(Value::Array(branches)) = object.get(keyword) else {
                return None;
            };
            if has_structural_besides(object, keyword) {
                return None;
            }
            let mut rest = branches.iter().filter(|b| !is_null_schema(b));
            match (rest.next(), rest.next()) {
                (Some(only), None) => Some(only),
                _ => None,
            }
        });
        match nullable {
            Some(only) => schema = only,
            None => return Ok(schema),
        }
    }
    Err(Reason::Keyword("$ref"))
}

fn local_pointer<'s>(document: &'s Value, reference: &str) -> Option<&'s Value> {
    let pointer = reference.strip_prefix('#')?;
    if pointer.is_empty() {
        Some(document)
    } else {
        document.pointer(pointer)
    }
}

fn has_structural_besides(object: &Map<String, Value>, keyword: &str) -> bool {
    STRUCTURAL
        .into_iter()
        .any(|k| k != keyword && object.contains_key(k))
}

fn is_null_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("null")
}

fn declares(object: &Map<String, Value>, ty: &str) -> bool {
    match object.get("type") {
        Some(Value::String(t)) => t == ty,
        Some(Value::Array(types)) => types.iter().any(|t| t.as_str() == Some(ty)),
        _ => false,
    }
}

/// `Some(type)` when the schema declares a `type` that rules out stepping
/// into it: a key needs an object, an index an object or an array.
fn excluded_type(object: &Map<String, Value>, index: bool) -> Option<String> {
    let ty = object.get("type")?;
    let allows = |t: &str| t == "object" || (index && t == "array");
    let allowed = match ty {
        Value::String(t) => allows(t),
        Value::Array(types) => types.iter().filter_map(Value::as_str).any(allows),
        _ => return None,
    };
    if allowed { None } else { Some(type_name(ty)) }
}

fn type_name(ty: &Value) -> String {
    match ty {
        Value::String(t) => t.clone(),
        other => other.to_string(),
    }
}

fn push_escaped(out: &mut String, key: &str) {
    for c in key.chars() {
        if c == '.' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
}
