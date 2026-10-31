//! The compiled filter tree and its evaluation.
//!
//! Evaluation borrows the document, allocates nothing and short-circuits:
//! `$and` stops at the first false clause, `$or` and `$any` at the first true
//! one.

use std::cmp::Ordering;

use serde_json::{Number, Value};

use crate::path::{Path, Root};
use crate::{Doc, Roots};

#[derive(Debug)]
pub(crate) enum Node {
    /// Implicit or explicit AND; empty matches everything.
    And(Box<[Node]>),
    Or(Box<[Node]>),
    Not(Box<Node>),
    Path(Box<PathClause>),
}

/// `"<path>": { "$op": operand, ... }`: an AND of operators on one path.
#[derive(Debug)]
pub(crate) struct PathClause {
    pub path: Path,
    pub ops: Box<[Op]>,
}

#[derive(Debug)]
pub(crate) enum Op {
    Eq(Value),
    Ne(Value),
    In(Box<[Value]>),
    Nin(Box<[Value]>),
    Gt(Bound),
    Gte(Bound),
    Lt(Bound),
    Lte(Bound),
    Exists(bool),
    Prefix(Box<str>),
    Any(AnyOperand),
}

#[derive(Debug)]
pub(crate) enum AnyOperand {
    /// `{"$any": {"$eq": "vip"}}`: operators applied to each element.
    Ops(Box<[Op]>),
    /// `{"$any": {"@.status": {"$eq": "paid"}}}`: a filter over each element.
    Filter(Box<Node>),
}

/// The operand of `$gt` / `$gte` / `$lt` / `$lte`.
#[derive(Debug)]
pub(crate) enum Bound {
    Num(f64),
    Str(Box<str>),
}

impl Node {
    pub fn matches<'a>(&self, doc: &Doc<'a>, element: Option<&'a Value>) -> bool {
        match self {
            Node::And(nodes) => nodes.iter().all(|n| n.matches(doc, element)),
            Node::Or(nodes) => nodes.iter().any(|n| n.matches(doc, element)),
            Node::Not(node) => !node.matches(doc, element),
            Node::Path(clause) => {
                let value = clause.path.resolve(doc, element);
                clause.ops.iter().all(|op| op.matches(value, doc))
            }
        }
    }

    pub fn is_match_all(&self) -> bool {
        matches!(self, Node::And(nodes) if nodes.is_empty())
    }

    /// The equality conditions a document must meet for the filter to match
    /// it: for each clause of the top-level `$and` on a document path (not
    /// `@`), its `$eq` operand, or its `$in` operands, as `(path, operands)`.
    /// A document whose value at `path` equals none of `operands` does not
    /// match (an absent path equals nothing).
    pub fn equality_terms(&self) -> impl Iterator<Item = (&Path, &[Value])> {
        let clauses: &[Node] = match self {
            Node::And(nodes) => nodes,
            _ => &[],
        };
        clauses
            .iter()
            .filter_map(|node| match node {
                Node::Path(clause) if clause.path.root != Root::Element => Some(clause),
                _ => None,
            })
            .flat_map(|clause| {
                clause.ops.iter().filter_map(move |op| match op {
                    Op::Eq(literal) => Some((&clause.path, std::slice::from_ref(literal))),
                    Op::In(literals) => Some((&clause.path, &literals[..])),
                    _ => None,
                })
            })
    }

    pub fn collect_roots(&self, roots: &mut Roots) {
        match self {
            Node::And(nodes) | Node::Or(nodes) => {
                nodes.iter().for_each(|n| n.collect_roots(roots));
            }
            Node::Not(node) => node.collect_roots(roots),
            Node::Path(clause) => match clause.path.root {
                Root::Input => roots.input = true,
                Root::Output => roots.output = true,
                Root::Error => roots.error = true,
                Root::Caller => roots.caller = true,
                Root::Metadata => roots.metadata = true,
                // `@` paths live inside `$any`, whose own path names the root.
                Root::Element => {}
            },
        }
    }
}

impl Op {
    /// `value` is `None` when the path is absent.
    fn matches<'a>(&self, value: Option<&'a Value>, doc: &Doc<'a>) -> bool {
        match self {
            Op::Eq(literal) => value.is_some_and(|v| json_eq(v, literal)),
            Op::Ne(literal) => !value.is_some_and(|v| json_eq(v, literal)),
            Op::In(literals) => value.is_some_and(|v| literals.iter().any(|l| json_eq(v, l))),
            Op::Nin(literals) => !value.is_some_and(|v| literals.iter().any(|l| json_eq(v, l))),
            Op::Gt(bound) => compare(value, bound) == Some(Ordering::Greater),
            Op::Gte(bound) => matches!(
                compare(value, bound),
                Some(Ordering::Greater | Ordering::Equal)
            ),
            Op::Lt(bound) => compare(value, bound) == Some(Ordering::Less),
            Op::Lte(bound) => matches!(
                compare(value, bound),
                Some(Ordering::Less | Ordering::Equal)
            ),
            Op::Exists(expected) => value.is_some() == *expected,
            Op::Prefix(prefix) => {
                matches!(value, Some(Value::String(s)) if s.starts_with(&**prefix))
            }
            Op::Any(operand) => {
                let Some(Value::Array(items)) = value else {
                    return false;
                };
                match operand {
                    AnyOperand::Ops(ops) => items
                        .iter()
                        .any(|item| ops.iter().all(|op| op.matches(Some(item), doc))),
                    AnyOperand::Filter(node) => {
                        items.iter().any(|item| node.matches(doc, Some(item)))
                    }
                }
            }
        }
    }
}

/// Ordering within one JSON type: numbers numerically (as f64), strings by
/// Unicode code point (UTF-8 byte order). Anything else is unordered.
#[inline]
fn compare(value: Option<&Value>, bound: &Bound) -> Option<Ordering> {
    match (value?, bound) {
        (Value::Number(n), Bound::Num(b)) => number_as_f64(n).partial_cmp(b),
        (Value::String(s), Bound::Str(b)) => Some(s.as_str().cmp(&**b)),
        _ => None,
    }
}

/// Every number compares as an f64: exact for integers within ±2^53 (the
/// only integer literals a filter may hold), and the documented behaviour for
/// payload integers beyond that. `-0` equals `0`.
#[inline]
pub(crate) fn number_as_f64(n: &Number) -> f64 {
    n.as_f64().unwrap_or(f64::NAN)
}

/// JSON deep equality: numbers by value, object keys in any order.
pub(crate) fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => number_as_f64(x) == number_as_f64(y),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| json_eq(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_eq(v, w)))
        }
        _ => false,
    }
}
