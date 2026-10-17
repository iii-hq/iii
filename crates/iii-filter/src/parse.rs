//! Validation (filters.md §2, §4) and normalization (§5) in one pass.
//!
//! Every filter object is parsed bottom-up into a compiled [`Node`] and its
//! normal form at the same time:
//!
//! - `$and` (explicit or implicit) is flattened into the enclosing filter;
//! - the clauses of each filter are sorted by key (path, `$not`, `$or`), then
//!   by the compact JSON of their operand, and exact duplicates are dropped;
//! - a filter with clauses is written `{"$and": [{clause}, ...]}`, one clause
//!   per object; a filter without clauses is `{}`;
//! - operators inside an operator object are sorted by name; operands are
//!   kept as written (`$ne` / `$nin` included);
//! - paths are kept as written: with only `\.` and `\\` as escapes, every
//!   key sequence has exactly one spelling.
//!
//! These limits are defined so that normalizing never increases them, so a
//! normal form passes every check its source passed (the JSON depth below
//! grows with the `$and` wrappers, but stays far under its limit):
//!
//! - depth: the top-level filter is at depth 1; the filters of a `$or`, the
//!   operand of `$not` and the operand of `$any` are one level deeper than the
//!   object holding them; the filters of a `$and` stay at the same level.
//! - nodes: every operator (`$eq` ... `$any`) and every `$or` / `$not` counts
//!   one; paths and `$and` are free.
//! - literals: the `$eq` / `$ne` operand and each `$in` / `$nin` item nest at
//!   most [`MAX_LITERAL_DEPTH`] arrays / objects.
//!
//! Two checks run first, without recursion, in this order:
//!
//! 1. the encoded size, on the filter as written. The normal form can be
//!    longer than its source, so [`SizeLimit::Skip`] compiles a normal form
//!    received from the engine without it;
//! 2. the raw JSON nesting, [`MAX_JSON_DEPTH`] arrays / objects in the whole
//!    value, on every input (the normal form of a valid filter stays far
//!    below it).
//!
//! Then the structure is parsed. Every error is `INVALID_FILTER`; the order
//! only decides the message. Every recursion from here on is bounded:
//! filters by [`MAX_DEPTH`], literals by [`MAX_LITERAL_DEPTH`], and `$and`,
//! which adds no filter depth, is walked with an explicit stack.

use std::io;

use serde_json::{Map, Number, Value};

use crate::error::FilterError;
use crate::eval::{AnyOperand, Bound, Node, Op, PathClause, number_as_f64};
use crate::limits::{
    MAX_DEPTH, MAX_ENCODED_BYTES, MAX_JSON_DEPTH, MAX_LITERAL_DEPTH, MAX_NODES, MAX_SAFE_INTEGER,
    MAX_SET_ITEMS,
};
use crate::path::Path;

pub(crate) struct Parsed {
    pub node: Node,
    pub normalized: Value,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizeLimit {
    /// A filter as an observer wrote it.
    Enforce,
    /// A normal form the engine already validated.
    Skip,
}

pub(crate) fn parse(src: &Value, size_limit: SizeLimit) -> Result<Parsed, FilterError> {
    if size_limit == SizeLimit::Enforce {
        ensure_encoded_size(src)?;
    }
    ensure_json_depth(src)?;
    let mut nodes = NodeBudget(0);
    let (node, normalized) = parse_filter(src, 1, false, &mut nodes)?;
    Ok(Parsed { node, normalized })
}

struct NodeBudget(usize);

impl NodeBudget {
    fn take(&mut self) -> Result<(), FilterError> {
        self.0 += 1;
        if self.0 > MAX_NODES {
            return Err(FilterError::invalid(format!(
                "more than {MAX_NODES} operator / clause nodes"
            )));
        }
        Ok(())
    }
}

/// A normalized clause: `{ key: operand }`.
struct Clause {
    key: String,
    operand: Value,
    /// Compact JSON of `operand` with object keys sorted, the tie-breaker
    /// when keys are equal.
    operand_json: String,
    node: Node,
}

impl Clause {
    fn new(key: String, operand: Value, node: Node) -> Clause {
        let mut operand_json = String::new();
        write_canonical(&operand, &mut operand_json);
        Clause {
            key,
            operand,
            operand_json,
            node,
        }
    }
}

/// Compact JSON with object keys sorted by byte order, whatever order the map
/// keeps (serde_json's `preserve_order` feature keeps insertion order). Only
/// strings and numbers go through serde_json. The recursion is bounded: the
/// operands of a parsed filter are at most `MAX_DEPTH` filters and
/// `MAX_LITERAL_DEPTH` literal levels deep.
fn write_canonical(value: &Value, out: &mut String) {
    fn leaf<T: serde::Serialize + ?Sized>(value: &T, out: &mut String) {
        out.push_str(&serde_json::to_string(value).expect("a JSON leaf always serializes"));
    }
    match value {
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (key, member)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                leaf(key, out);
                out.push(':');
                write_canonical(member, out);
            }
            out.push('}');
        }
        other => leaf(other, out),
    }
}

/// Parses one filter object into its compiled node and normal form.
fn parse_filter(
    src: &Value,
    depth: usize,
    in_any: bool,
    nodes: &mut NodeBudget,
) -> Result<(Node, Value), FilterError> {
    let mut clauses = Vec::new();
    collect_clauses(src, depth, in_any, nodes, &mut clauses)?;
    Ok(finish(clauses))
}

/// Collects the clauses of `src` and of every filter its `$and`s hold. The
/// `$and` closure is walked with an explicit stack: `$and` adds no depth, so
/// its nesting is bounded only by the encoded size, not by `MAX_DEPTH`.
fn collect_clauses(
    src: &Value,
    depth: usize,
    in_any: bool,
    nodes: &mut NodeBudget,
    out: &mut Vec<Clause>,
) -> Result<(), FilterError> {
    if depth > MAX_DEPTH {
        return Err(FilterError::invalid(format!(
            "filters nested deeper than {MAX_DEPTH} levels"
        )));
    }

    let mut pending = vec![src];
    while let Some(src) = pending.pop() {
        let Value::Object(map) = src else {
            return Err(FilterError::invalid(format!(
                "a filter must be a JSON object, found {}",
                kind(src)
            )));
        };
        for (key, value) in map {
            match key.as_str() {
                "$and" => {
                    pending.extend(filter_list("$and", value)?.iter().rev());
                }
                "$or" => {
                    nodes.take()?;
                    let items = filter_list("$or", value)?;
                    let mut children = Vec::with_capacity(items.len());
                    let mut encoded = Vec::with_capacity(items.len());
                    for item in items {
                        let (node, normalized) = parse_filter(item, depth + 1, in_any, nodes)?;
                        children.push(node);
                        encoded.push(normalized);
                    }
                    out.push(Clause::new(
                        "$or".to_owned(),
                        Value::Array(encoded),
                        Node::Or(children.into()),
                    ));
                }
                "$not" => {
                    nodes.take()?;
                    if !value.is_object() {
                        return Err(FilterError::invalid(format!(
                            "`$not` takes a filter object, found {}",
                            kind(value)
                        )));
                    }
                    let (node, normalized) = parse_filter(value, depth + 1, in_any, nodes)?;
                    out.push(Clause::new(
                        "$not".to_owned(),
                        normalized,
                        Node::Not(Box::new(node)),
                    ));
                }
                op if is_value_operator(op) => {
                    return Err(FilterError::invalid(format!(
                        "`{op}` needs a path: write {{\"<path>\": {{\"{op}\": ...}}}}"
                    )));
                }
                op if op.starts_with('$') => {
                    return Err(FilterError::invalid(format!(
                        "unknown operator `{op}` (a filter holds paths, `$and`, `$or` and `$not`)"
                    )));
                }
                raw => {
                    let path = Path::parse(raw, in_any)?;
                    let (ops, operand) = parse_operator_object(raw, value, depth, nodes)?;
                    out.push(Clause::new(
                        raw.to_owned(),
                        operand,
                        Node::Path(Box::new(PathClause { path, ops })),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Sorts, deduplicates and wraps the clauses of one filter.
fn finish(mut clauses: Vec<Clause>) -> (Node, Value) {
    clauses.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then_with(|| a.operand_json.cmp(&b.operand_json))
    });
    clauses.dedup_by(|a, b| a.key == b.key && a.operand_json == b.operand_json);
    if clauses.is_empty() {
        return (Node::And(Box::new([])), Value::Object(Map::new()));
    }

    let mut nodes = Vec::with_capacity(clauses.len());
    let mut encoded = Vec::with_capacity(clauses.len());
    for clause in clauses {
        let mut object = Map::new();
        object.insert(clause.key, clause.operand);
        encoded.push(Value::Object(object));
        nodes.push(clause.node);
    }
    let mut top = Map::new();
    top.insert("$and".to_owned(), Value::Array(encoded));
    (Node::And(nodes.into()), Value::Object(top))
}

fn filter_list<'v>(name: &str, value: &'v Value) -> Result<&'v [Value], FilterError> {
    match value {
        Value::Array(items) if items.is_empty() => Err(FilterError::invalid(format!(
            "`{name}` takes a non-empty array of filters"
        ))),
        Value::Array(items) => Ok(items),
        other => Err(FilterError::invalid(format!(
            "`{name}` takes an array of filters, found {}",
            kind(other)
        ))),
    }
}

fn parse_operator_object(
    path: &str,
    value: &Value,
    depth: usize,
    nodes: &mut NodeBudget,
) -> Result<(Box<[Op]>, Value), FilterError> {
    let shorthand = || {
        FilterError::invalid(format!(
            "`{path}`: expected an operator object such as {{\"$eq\": ...}}; literal values are not allowed"
        ))
    };
    let Value::Object(map) = value else {
        return Err(shorthand());
    };
    if map.is_empty() {
        return Err(FilterError::invalid(format!(
            "`{path}`: empty operator object"
        )));
    }
    if map.keys().any(|k| !k.starts_with('$')) {
        return Err(shorthand());
    }
    parse_operators(path, map, depth, nodes)
}

/// Parses the operators of an operator object; `depth` is the level of the
/// object holding them.
fn parse_operators(
    path: &str,
    map: &Map<String, Value>,
    depth: usize,
    nodes: &mut NodeBudget,
) -> Result<(Box<[Op]>, Value), FilterError> {
    let mut parsed: Vec<(&str, Op, Value)> = Vec::with_capacity(map.len());
    for (name, operand) in map {
        nodes.take()?;
        let (op, normalized) = parse_operator(path, name, operand, depth, nodes)?;
        parsed.push((name, op, normalized));
    }
    parsed.sort_by(|a, b| a.0.cmp(b.0));

    let mut ops = Vec::with_capacity(parsed.len());
    let mut encoded = Map::new();
    for (name, op, normalized) in parsed {
        encoded.insert(name.to_owned(), normalized);
        ops.push(op);
    }
    Ok((ops.into(), Value::Object(encoded)))
}

fn parse_operator(
    path: &str,
    name: &str,
    operand: &Value,
    depth: usize,
    nodes: &mut NodeBudget,
) -> Result<(Op, Value), FilterError> {
    let wrong = |expected: &str| {
        FilterError::invalid(format!(
            "`{path}`: `{name}` takes {expected}, found {}",
            kind(operand)
        ))
    };
    let op = match name {
        "$eq" | "$ne" => {
            check_literal(path, name, operand)?;
            if name == "$eq" {
                Op::Eq(operand.clone())
            } else {
                Op::Ne(operand.clone())
            }
        }
        "$in" | "$nin" => {
            let Value::Array(items) = operand else {
                return Err(wrong("an array"));
            };
            if items.len() > MAX_SET_ITEMS {
                return Err(FilterError::invalid(format!(
                    "`{path}`: `{name}` has {} items (limit {MAX_SET_ITEMS})",
                    items.len()
                )));
            }
            for item in items {
                check_literal(path, name, item)?;
            }
            let items: Box<[Value]> = items.clone().into();
            if name == "$in" {
                Op::In(items)
            } else {
                Op::Nin(items)
            }
        }
        "$gt" | "$gte" | "$lt" | "$lte" => {
            let bound = match operand {
                Value::Number(n) => {
                    check_number(path, name, n)?;
                    Bound::Num(number_as_f64(n))
                }
                Value::String(s) => Bound::Str(s.as_str().into()),
                _ => return Err(wrong("a number or a string")),
            };
            match name {
                "$gt" => Op::Gt(bound),
                "$gte" => Op::Gte(bound),
                "$lt" => Op::Lt(bound),
                _ => Op::Lte(bound),
            }
        }
        "$exists" => match operand {
            Value::Bool(b) => Op::Exists(*b),
            _ => return Err(wrong("a boolean")),
        },
        "$prefix" => match operand {
            Value::String(s) => Op::Prefix(s.as_str().into()),
            _ => return Err(wrong("a string")),
        },
        "$any" => return parse_any(path, operand, depth, nodes),
        "$and" | "$or" | "$not" => {
            return Err(FilterError::invalid(format!(
                "`{path}`: `{name}` combines filters and is not a value operator"
            )));
        }
        _ => {
            return Err(FilterError::invalid(format!(
                "`{path}`: unknown operator `{name}`"
            )));
        }
    };
    Ok((op, operand.clone()))
}

/// `$any` takes an operator object (all keys are value operators) or a
/// filter over the element (`@` paths, `$and`, `$or`, `$not`). `{}` is the
/// empty filter: it matches any non-empty array.
fn parse_any(
    path: &str,
    operand: &Value,
    depth: usize,
    nodes: &mut NodeBudget,
) -> Result<(Op, Value), FilterError> {
    let Value::Object(map) = operand else {
        return Err(FilterError::invalid(format!(
            "`{path}`: `$any` takes an operator object or a filter over `@`, found {}",
            kind(operand)
        )));
    };
    if depth + 1 > MAX_DEPTH {
        return Err(FilterError::invalid(format!(
            "filters nested deeper than {MAX_DEPTH} levels"
        )));
    }
    if !map.is_empty() && map.keys().all(|k| is_value_operator(k)) {
        let (ops, normalized) = parse_operators(path, map, depth + 1, nodes)?;
        Ok((Op::Any(AnyOperand::Ops(ops)), normalized))
    } else {
        let (node, normalized) = parse_filter(operand, depth + 1, true, nodes)?;
        Ok((Op::Any(AnyOperand::Filter(Box::new(node))), normalized))
    }
}

fn is_value_operator(name: &str) -> bool {
    matches!(
        name,
        "$eq"
            | "$ne"
            | "$in"
            | "$nin"
            | "$gt"
            | "$gte"
            | "$lt"
            | "$lte"
            | "$exists"
            | "$prefix"
            | "$any"
    )
}

/// Checks one literal (an `$eq` / `$ne` operand or an `$in` / `$nin` item):
/// numbers within ±2^53 and at most [`MAX_LITERAL_DEPTH`] nested arrays /
/// objects. The recursion stops at that depth.
fn check_literal(path: &str, name: &str, value: &Value) -> Result<(), FilterError> {
    fn walk(path: &str, name: &str, value: &Value, level: usize) -> Result<(), FilterError> {
        match value {
            Value::Number(n) => check_number(path, name, n),
            Value::Array(_) | Value::Object(_) if level > MAX_LITERAL_DEPTH => {
                Err(FilterError::invalid(format!(
                    "`{path}`: `{name}` nests a literal deeper than {MAX_LITERAL_DEPTH} arrays / objects"
                )))
            }
            Value::Array(items) => items
                .iter()
                .try_for_each(|v| walk(path, name, v, level + 1)),
            Value::Object(map) => map
                .values()
                .try_for_each(|v| walk(path, name, v, level + 1)),
            _ => Ok(()),
        }
    }
    walk(path, name, value, 1)
}

fn check_number(path: &str, name: &str, n: &Number) -> Result<(), FilterError> {
    if number_in_range(n) {
        Ok(())
    } else {
        Err(FilterError::invalid(format!(
            "`{path}`: `{name}` holds the number {n}, outside ±2^53"
        )))
    }
}

/// `|n| <= 2^53`. Integers are compared exactly; any other number by its f64
/// value (a float beyond 2^53 is integral too, and JavaScript cannot tell the
/// two apart).
fn number_in_range(n: &Number) -> bool {
    if let Some(u) = n.as_u64() {
        return u <= MAX_SAFE_INTEGER;
    }
    if let Some(i) = n.as_i64() {
        return i.unsigned_abs() <= MAX_SAFE_INTEGER;
    }
    n.as_f64()
        .is_some_and(|f| f.abs() <= MAX_SAFE_INTEGER as f64)
}

/// Fails once the compact JSON encoding (UTF-8 bytes) exceeds
/// [`MAX_ENCODED_BYTES`]. Containers are walked with an explicit stack and
/// only leaves (strings, keys, numbers) go through serde_json, so an
/// arbitrarily deep value cannot exhaust the call stack.
fn ensure_encoded_size(value: &Value) -> Result<(), FilterError> {
    struct Budget(usize);
    impl Budget {
        fn spend(&mut self, bytes: usize) -> io::Result<()> {
            match self.0.checked_sub(bytes) {
                Some(left) => {
                    self.0 = left;
                    Ok(())
                }
                None => Err(io::Error::other("encoded size limit")),
            }
        }
    }
    impl io::Write for Budget {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.spend(buf.len()).map(|()| buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn measure(value: &Value) -> Result<(), serde_json::Error> {
        let mut budget = Budget(MAX_ENCODED_BYTES);
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            match value {
                // `[`, `]` and the commas between items.
                Value::Array(items) => {
                    budget
                        .spend(2 + items.len().saturating_sub(1))
                        .map_err(serde_json::Error::io)?;
                    pending.extend(items);
                }
                // `{`, `}`, the commas, and a key and a colon per entry.
                Value::Object(map) => {
                    budget
                        .spend(2 + map.len().saturating_sub(1) + map.len())
                        .map_err(serde_json::Error::io)?;
                    for (key, member) in map {
                        serde_json::to_writer(&mut budget, key)?;
                        pending.push(member);
                    }
                }
                leaf => serde_json::to_writer(&mut budget, leaf)?,
            }
        }
        Ok(())
    }

    measure(value).map_err(|_| {
        FilterError::invalid(format!(
            "the filter encodes to more than {MAX_ENCODED_BYTES} bytes"
        ))
    })
}

/// Fails when arrays / objects nest deeper than [`MAX_JSON_DEPTH`] anywhere
/// in the value (`{}` and `[]` are one level). Walked with an explicit stack
/// that never goes below the limit.
fn ensure_json_depth(value: &Value) -> Result<(), FilterError> {
    let mut pending = vec![(value, 1usize)];
    while let Some((value, level)) = pending.pop() {
        let members: &mut dyn Iterator<Item = &Value> = match value {
            Value::Array(items) => &mut items.iter(),
            Value::Object(map) => &mut map.values(),
            _ => continue,
        };
        if level > MAX_JSON_DEPTH {
            return Err(FilterError::invalid(format!(
                "the filter nests deeper than {MAX_JSON_DEPTH} arrays / objects"
            )));
        }
        pending.extend(members.map(|member| (member, level + 1)));
    }
    Ok(())
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical(value: &Value) -> String {
        let mut out = String::new();
        write_canonical(value, &mut out);
        out
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_level() {
        // Built by insertion so that, with `preserve_order`, the map keeps
        // the unsorted order and only the explicit sort puts it right.
        let mut inner = Map::new();
        inner.insert("z".to_owned(), Value::from(0));
        inner.insert("y".to_owned(), Value::from("é\n\"q\""));
        let mut a = Map::new();
        a.insert(
            "d".to_owned(),
            Value::Array(vec![Value::from(1.5), Value::Object(inner)]),
        );
        a.insert("c".to_owned(), Value::Null);
        let mut top = Map::new();
        top.insert("b".to_owned(), Value::from(-1));
        top.insert("a".to_owned(), Value::Object(a));
        top.insert("B".to_owned(), Value::Bool(true));
        assert_eq!(
            canonical(&Value::Object(top)),
            r#"{"B":true,"a":{"c":null,"d":[1.5,{"y":"é\n\"q\"","z":0}]},"b":-1}"#
        );
    }

    #[test]
    fn canonical_json_matches_serde_json_on_sorted_maps() {
        // Every map here is written in sorted order, so serde_json agrees
        // with or without `preserve_order`.
        let value = serde_json::json!({
            "$and": [{ "input.a": { "$in": [1, 2.5, "x", null, { "k": [true] }] } }],
            "$or": [{}, { "s": "\u{1F600}\t" }]
        });
        assert_eq!(canonical(&value), serde_json::to_string(&value).unwrap());
    }
}
