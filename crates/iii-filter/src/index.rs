//! Many filters matched against one document by an index of their equality
//! conditions.
//!
//! A filter matches a document only if every clause of its top-level `$and`
//! does. A clause on a path with `$eq` of a scalar (or `$in` of scalars)
//! therefore names the values the document's value at that path must equal.
//! [`FilterIndex`] files each filter under one such condition, `path ->
//! value -> filters`, and keeps the filters without one in a list it scans.
//! Matching a document resolves each indexed path once, looks its value up,
//! and evaluates the whole filter of only the entries found there and of the
//! scanned ones: about one lookup per indexed path plus one evaluation per
//! candidate, however many filters the index holds.
//!
//! The index rules out only entries whose condition the document fails, and
//! evaluates every other entry's filter in full, so its matches are exactly
//! the entries a scan of every filter with [`Filter::matches`] would yield,
//! in key order.
//!
//! Values are indexed with the equality of the filter language: numbers by
//! their value as an IEEE-754 double (`1` and `1.0` are one value, `-0` is
//! `0`), strings by their code points, `null` and booleans as themselves,
//! and a value of one type never equals a value of another. An array or
//! object operand is never indexed: its clause is evaluated as part of the
//! filter.
//!
//! The index is persistent: a clone shares every node with the index it was
//! made from, and an insert or a removal copies the `O(log n)` nodes on its
//! path, so a version a reader holds stays valid while a writer makes the
//! next one.

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::eval::number_as_f64;
use crate::path::Path;
use crate::pmap::{self, PMap};
use crate::{Doc, Filter};

/// A value an equality condition is indexed by.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Scalar {
    Null,
    Bool(bool),
    /// The bits of the number as a double, `-0` as `0`.
    Num(u64),
    Str(Box<str>),
}

/// A document value, borrowed, as the index compares it with [`Scalar`]s.
#[derive(Clone, Copy)]
enum Probe<'a> {
    Null,
    Bool(bool),
    Num(u64),
    Str(&'a str),
}

/// The index key of a number: equal doubles have equal keys, and `NaN`
/// (a number no JSON text holds, and which equals nothing) has none.
fn number_key(value: f64) -> Option<u64> {
    if value.is_nan() {
        None
    } else if value == 0.0 {
        Some(0.0f64.to_bits())
    } else {
        Some(value.to_bits())
    }
}

impl<'a> Probe<'a> {
    /// `None` for an array or an object, which equals no scalar.
    fn of(value: &'a Value) -> Option<Probe<'a>> {
        Some(match value {
            Value::Null => Probe::Null,
            Value::Bool(b) => Probe::Bool(*b),
            Value::Number(n) => Probe::Num(number_key(number_as_f64(n))?),
            Value::String(s) => Probe::Str(s),
            Value::Array(_) | Value::Object(_) => return None,
        })
    }

    fn rank(&self) -> u8 {
        match self {
            Probe::Null => 0,
            Probe::Bool(_) => 1,
            Probe::Num(_) => 2,
            Probe::Str(_) => 3,
        }
    }
}

impl Scalar {
    fn of(value: &Value) -> Option<Scalar> {
        Some(match Probe::of(value)? {
            Probe::Null => Scalar::Null,
            Probe::Bool(b) => Scalar::Bool(b),
            Probe::Num(n) => Scalar::Num(n),
            Probe::Str(s) => Scalar::Str(s.into()),
        })
    }

    fn rank(&self) -> u8 {
        match self {
            Scalar::Null => 0,
            Scalar::Bool(_) => 1,
            Scalar::Num(_) => 2,
            Scalar::Str(_) => 3,
        }
    }

    /// Orders this key against `probe` as [`Ord`] orders two keys.
    fn cmp_probe(&self, probe: &Probe<'_>) -> Ordering {
        match (self, probe) {
            (Scalar::Null, Probe::Null) => Ordering::Equal,
            (Scalar::Bool(a), Probe::Bool(b)) => a.cmp(b),
            (Scalar::Num(a), Probe::Num(b)) => a.cmp(b),
            (Scalar::Str(a), Probe::Str(b)) => (**a).cmp(*b),
            _ => self.rank().cmp(&probe.rank()),
        }
    }
}

/// Whether an equality condition with these operands can be indexed: at
/// least one, and every one a scalar.
pub(crate) fn indexable_literals(literals: &[Value]) -> bool {
    !literals.is_empty() && literals.iter().all(|literal| Scalar::of(literal).is_some())
}

/// One entry as a lookup finds it.
#[derive(Clone)]
struct Slot<T> {
    filter: Filter,
    value: T,
}

/// The condition an entry is filed under: each of `values` of `path`.
#[derive(Clone)]
struct Condition {
    path: Arc<Path>,
    values: Arc<[Scalar]>,
}

/// An entry filed under a condition.
#[derive(Clone)]
struct Filed<T> {
    value: T,
    condition: Condition,
}

/// The entries filed under one path, by value.
#[derive(Clone)]
struct PathIndex<K, T> {
    buckets: PMap<Scalar, PMap<K, Slot<T>>>,
    /// Entries filed here.
    entries: usize,
}

/// Filters, each with a key and a value, matched against a document by an
/// index of their equality conditions (see the [module docs](self)).
///
/// [`FilterIndex::matches`] yields exactly the entries whose filter matches
/// the document, in key order: what evaluating every entry's filter in key
/// order would yield.
///
/// ```
/// use iii_filter::{Doc, Filter, FilterIndex};
/// use serde_json::json;
///
/// let mut index = FilterIndex::new();
/// for k in 0..1_000u64 {
///     let filter = Filter::parse(&json!({ "input.k": { "$eq": k } })).unwrap();
///     index.insert(k, &filter, format!("observer-{k}"));
/// }
/// let input = json!({ "k": 42 });
/// let doc = Doc { input: Some(&input), ..Doc::default() };
/// let matched: Vec<&String> = index.matches(&doc).map(|(_, value)| value).collect();
/// assert_eq!(matched, ["observer-42"]);
/// // One lookup, one filter evaluated.
/// assert_eq!(index.candidates(&doc), 1);
/// ```
pub struct FilterIndex<K, T> {
    /// Entries without a condition the index serves.
    scanned: PMap<K, Slot<T>>,
    /// Entries filed under an equality condition, with the condition.
    filed: PMap<K, Filed<T>>,
    /// The entries of `filed` by the path and value of their condition.
    paths: PMap<Arc<Path>, PathIndex<K, T>>,
}

impl<K, T> Clone for FilterIndex<K, T> {
    /// Shares every node: `O(1)`.
    fn clone(&self) -> Self {
        Self {
            scanned: self.scanned.clone(),
            filed: self.filed.clone(),
            paths: self.paths.clone(),
        }
    }
}

impl<K, T> Default for FilterIndex<K, T> {
    fn default() -> Self {
        Self {
            scanned: PMap::default(),
            filed: PMap::default(),
            paths: PMap::default(),
        }
    }
}

impl<K, T> fmt::Debug for FilterIndex<K, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FilterIndex")
            .field("len", &self.len())
            .field("scanned", &self.scanned.len())
            .field("indexed_paths", &self.paths.len())
            .finish()
    }
}

impl<K, T> FilterIndex<K, T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.scanned.len() + self.filed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scanned.is_empty() && self.filed.is_empty()
    }

    /// Every entry, in key order.
    pub fn iter(&self) -> Iter<'_, K, T>
    where
        K: Ord,
    {
        let mut scanned = self.scanned.iter();
        let mut filed = self.filed.iter();
        Iter {
            next_scanned: scanned.next(),
            next_filed: filed.next(),
            scanned,
            filed,
        }
    }

    /// The entries whose filter matches `doc`, in key order. Resolves each
    /// indexed path of `doc` once, and evaluates the filters of the entries
    /// filed under the value found there and of the entries filed under no
    /// condition ([`Self::candidates`] counts them); every other entry has a
    /// condition `doc` fails.
    pub fn matches<'s, 'd>(&'s self, doc: &Doc<'d>) -> Matches<'s, 'd, K, T>
    where
        K: Ord,
    {
        let mut streams = Streams::None;
        for (path, index) in self.paths.iter() {
            if let Some(bucket) = index.bucket(path, doc) {
                streams.push(bucket);
            }
        }
        if !self.scanned.is_empty() {
            streams.push(&self.scanned);
        }
        Matches { doc: *doc, streams }
    }

    /// How many filters a match of `doc` evaluates: the entries filed under
    /// `doc`'s values of the indexed paths, and those filed under no
    /// condition.
    pub fn candidates(&self, doc: &Doc<'_>) -> usize {
        let indexed: usize = self
            .paths
            .iter()
            .filter_map(|(path, index)| index.bucket(path, doc))
            .map(PMap::len)
            .sum();
        indexed + self.scanned.len()
    }
}

impl<K: Ord, T> FilterIndex<K, T> {
    pub fn get(&self, key: &K) -> Option<&T> {
        match self.scanned.get(key) {
            Some(slot) => Some(&slot.value),
            None => self.filed.get(key).map(|filed| &filed.value),
        }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.scanned.contains_key(key) || self.filed.contains_key(key)
    }
}

impl<K: Ord + Clone, T: Clone> FilterIndex<K, T> {
    /// Files `value` under `key` with `filter`, in place of the entry `key`
    /// had; returns that entry's value. `O(log n)`, plus the number of `$in`
    /// operands when the entry is filed under them.
    pub fn insert(&mut self, key: K, filter: &Filter, value: T) -> Option<T> {
        let replaced = self.remove(&key);
        let slot = Slot {
            filter: filter.clone(),
            value,
        };
        let Some(condition) = self.condition_for(filter) else {
            self.scanned.insert(key, slot);
            return replaced;
        };
        if !self.paths.contains_key(&condition.path) {
            self.paths.insert(
                condition.path.clone(),
                PathIndex {
                    buckets: PMap::default(),
                    entries: 0,
                },
            );
        }
        let index = self
            .paths
            .get_mut(&condition.path)
            .expect("the path is indexed");
        for scalar in condition.values.iter() {
            match index.buckets.get_mut(scalar) {
                Some(bucket) => {
                    bucket.insert(key.clone(), slot.clone());
                }
                None => {
                    let mut bucket = PMap::default();
                    bucket.insert(key.clone(), slot.clone());
                    index.buckets.insert(scalar.clone(), bucket);
                }
            }
        }
        index.entries += 1;
        self.filed.insert(
            key,
            Filed {
                value: slot.value,
                condition,
            },
        );
        replaced
    }

    /// Takes the entry under `key` out; returns its value. `O(log n)`, plus
    /// the number of values it is filed under.
    pub fn remove(&mut self, key: &K) -> Option<T> {
        if let Some(slot) = self.scanned.remove(key) {
            return Some(slot.value);
        }
        let Filed { value, condition } = self.filed.remove(key)?;
        let Some(index) = self.paths.get_mut(&condition.path) else {
            return Some(value);
        };
        for scalar in condition.values.iter() {
            let Some(bucket) = index.buckets.get_mut(scalar) else {
                continue;
            };
            bucket.remove(key);
            if bucket.is_empty() {
                index.buckets.remove(scalar);
            }
        }
        index.entries -= 1;
        if index.entries == 0 {
            self.paths.remove(&condition.path);
        }
        Some(value)
    }

    /// The condition `filter` is filed under: the equality condition of its
    /// top-level `$and` that the fewest entries share now (an `$eq` before
    /// an `$in`); `None` (the scanned list) when it has none an index serves.
    fn condition_for(&self, filter: &Filter) -> Option<Condition> {
        let mut best: Option<Choice> = None;
        for (path, literals) in filter.equality_terms() {
            let Some(mut values) = literals.iter().map(Scalar::of).collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if values.is_empty() {
                continue;
            }
            values.sort();
            values.dedup();
            let (path, held) = match self.paths.get_by(|held| (**held).cmp(path)) {
                Some((held_path, index)) => (held_path.clone(), Some(index)),
                None => (Arc::new(path.clone()), None),
            };
            let sharing: usize = held.map_or(0, |index| {
                values
                    .iter()
                    .filter_map(|value| index.buckets.get(value))
                    .map(PMap::len)
                    .sum()
            });
            let score = (values.len() > 1, sharing, values.len());
            if best.as_ref().is_none_or(|held| score < held.score) {
                best = Some(Choice {
                    score,
                    path,
                    values,
                });
            }
        }
        best.map(|Choice { path, values, .. }| Condition {
            path,
            values: values.into(),
        })
    }
}

/// A condition [`FilterIndex::place`] may file an entry under.
struct Choice {
    /// Lower is better: an `$in` after an `$eq`, then the entries already
    /// filed under its values, then how many values it has.
    score: (bool, usize, usize),
    path: Arc<Path>,
    values: Vec<Scalar>,
}

impl<K, T> PathIndex<K, T> {
    /// The entries filed under `doc`'s value at `path`.
    fn bucket(&self, path: &Path, doc: &Doc<'_>) -> Option<&PMap<K, Slot<T>>> {
        let probe = Probe::of(path.resolve(doc, None)?)?;
        self.buckets
            .get_by(|held| held.cmp_probe(&probe))
            .map(|(_, bucket)| bucket)
    }
}

/// The lists a match reads: at most one bucket per indexed path, and the
/// scanned list. Each entry is in exactly one of them.
// One list is read with its inline stack, on the stack of the match: no
// allocation for a lookup that hits one bucket, or for a scan.
#[allow(clippy::large_enum_variant)]
enum Streams<'s, K, T> {
    None,
    One(pmap::Iter<'s, K, Slot<T>>),
    Many(Vec<Stream<'s, K, T>>),
}

/// One list, with its next entry read ahead.
struct Stream<'s, K, T> {
    head: Option<(&'s K, &'s Slot<T>)>,
    rest: pmap::Iter<'s, K, Slot<T>>,
}

impl<'s, K, T> Stream<'s, K, T> {
    fn of(mut rest: pmap::Iter<'s, K, Slot<T>>) -> Self {
        Self {
            head: rest.next(),
            rest,
        }
    }
}

impl<'s, K, T> Streams<'s, K, T> {
    fn push(&mut self, list: &'s PMap<K, Slot<T>>) {
        *self = match std::mem::replace(self, Streams::None) {
            Streams::None => Streams::One(list.iter()),
            Streams::One(first) => Streams::Many(vec![Stream::of(first), Stream::of(list.iter())]),
            Streams::Many(mut streams) => {
                streams.push(Stream::of(list.iter()));
                Streams::Many(streams)
            }
        };
    }
}

/// Every entry of a [`FilterIndex`], in key order (see [`FilterIndex::iter`]).
pub struct Iter<'s, K, T> {
    next_scanned: Option<(&'s K, &'s Slot<T>)>,
    next_filed: Option<(&'s K, &'s Filed<T>)>,
    scanned: pmap::Iter<'s, K, Slot<T>>,
    filed: pmap::Iter<'s, K, Filed<T>>,
}

impl<'s, K: Ord, T> Iterator for Iter<'s, K, T> {
    type Item = (&'s K, &'s T);

    fn next(&mut self) -> Option<Self::Item> {
        let scanned_first = match (self.next_scanned, self.next_filed) {
            (None, None) => return None,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some((a, _)), Some((b, _))) => a < b,
        };
        if scanned_first {
            let (key, slot) = self.next_scanned.take()?;
            self.next_scanned = self.scanned.next();
            Some((key, &slot.value))
        } else {
            let (key, filed) = self.next_filed.take()?;
            self.next_filed = self.filed.next();
            Some((key, &filed.value))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.scanned.len()
            + self.filed.len()
            + usize::from(self.next_scanned.is_some())
            + usize::from(self.next_filed.is_some());
        (len, Some(len))
    }
}

impl<K: Ord, T> ExactSizeIterator for Iter<'_, K, T> {}

/// The entries of a [`FilterIndex`] whose filter matches a document, in key
/// order (see [`FilterIndex::matches`]).
pub struct Matches<'s, 'd, K, T> {
    doc: Doc<'d>,
    streams: Streams<'s, K, T>,
}

impl<'s, K: Ord, T> Matches<'s, '_, K, T> {
    /// The next candidate in key order: the smallest head of the lists.
    fn next_candidate(&mut self) -> Option<(&'s K, &'s Slot<T>)> {
        match &mut self.streams {
            Streams::None => None,
            Streams::One(list) => list.next(),
            Streams::Many(streams) => {
                let mut smallest: Option<usize> = None;
                for (at, stream) in streams.iter().enumerate() {
                    let Some((key, _)) = stream.head else {
                        continue;
                    };
                    if smallest
                        .is_none_or(|best| streams[best].head.is_some_and(|(held, _)| key < held))
                    {
                        smallest = Some(at);
                    }
                }
                let stream = &mut streams[smallest?];
                let candidate = stream.head.take();
                stream.head = stream.rest.next();
                candidate
            }
        }
    }
}

impl<'s, K: Ord, T> Iterator for Matches<'s, '_, K, T> {
    type Item = (&'s K, &'s T);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (key, slot) = self.next_candidate()?;
            if slot.filter.matches(&self.doc) {
                return Some((key, &slot.value));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn filter(src: Value) -> Filter {
        Filter::parse(&src).unwrap()
    }

    fn matched(index: &FilterIndex<u32, u32>, doc: &Value) -> Vec<u32> {
        index
            .matches(&Doc::from_value(doc))
            .map(|(_, value)| *value)
            .collect()
    }

    #[test]
    fn a_filter_is_indexable_when_the_index_files_it() {
        for (src, indexable) in [
            (json!({ "input.k": { "$eq": 1 } }), true),
            (
                json!({ "input.k": { "$in": ["a", null] }, "input.n": { "$gt": 1 } }),
                true,
            ),
            (json!({ "input.k": { "$eq": [1] } }), false),
            (json!({ "input.k": { "$in": [] } }), false),
            (json!({ "input.k": { "$in": [1, { "a": 1 }] } }), false),
            (json!({ "$or": [{ "input.k": { "$eq": 1 } }] }), false),
            (json!({ "input.k": { "$any": { "$eq": 1 } } }), false),
            (json!({}), false),
        ] {
            let filter = filter(src.clone());
            assert_eq!(filter.is_indexable(), indexable, "{src}");
            let mut index = FilterIndex::new();
            index.insert(1u32, &filter, 1u32);
            assert_eq!(index.scanned.is_empty(), indexable, "{src}");
        }
    }

    #[test]
    fn numbers_are_one_value_whatever_their_spelling() {
        let mut index = FilterIndex::new();
        index.insert(1, &filter(json!({ "input.n": { "$eq": 1 } })), 1);
        index.insert(2, &filter(json!({ "input.n": { "$eq": 0 } })), 2);
        index.insert(3, &filter(json!({ "input.n": { "$in": [1.5, -2] } })), 3);
        let doc = |text: &str| -> Value {
            serde_json::from_str(&format!(r#"{{"input":{{"n":{text}}}}}"#)).unwrap()
        };
        assert_eq!(matched(&index, &doc("1.0")), [1]);
        assert_eq!(matched(&index, &doc("1e0")), [1]);
        assert_eq!(matched(&index, &doc("-0.0")), [2]);
        assert_eq!(matched(&index, &doc("-0")), [2]);
        assert_eq!(matched(&index, &doc("-2.0")), [3]);
        assert_eq!(matched(&index, &doc("1.5")), [3]);
        assert!(matched(&index, &doc("\"1\"")).is_empty());
        assert!(matched(&index, &doc("true")).is_empty());
        assert!(matched(&index, &doc("[1]")).is_empty());
        assert!(matched(&index, &json!({ "input": {} })).is_empty());
        assert_eq!(index.candidates(&Doc::from_value(&doc("7"))), 0);
    }

    #[test]
    fn a_filter_without_an_indexable_condition_is_scanned() {
        let mut index = FilterIndex::new();
        index.insert(1, &filter(json!({ "input.n": { "$gt": 1 } })), 1);
        index.insert(2, &filter(json!({ "input.a": { "$eq": [1, 2] } })), 2);
        index.insert(
            3,
            &filter(json!({ "$or": [{ "input.n": { "$eq": 1 } }] })),
            3,
        );
        index.insert(4, &filter(json!({})), 4);
        index.insert(5, &filter(json!({ "input.k": { "$eq": "x" } })), 5);
        let doc = json!({ "input": { "n": 1, "a": [1, 2], "k": "y" } });
        assert_eq!(index.candidates(&Doc::from_value(&doc)), 4);
        assert_eq!(matched(&index, &doc), [2, 3, 4]);
        let doc = json!({ "input": { "n": 2, "k": "x" } });
        assert_eq!(matched(&index, &doc), [1, 4, 5]);
    }

    #[test]
    fn matches_come_in_key_order_across_paths_and_the_scanned_list() {
        let mut index = FilterIndex::new();
        index.insert(30, &filter(json!({ "input.a": { "$eq": 1 } })), 30);
        index.insert(10, &filter(json!({ "input.b": { "$eq": 1 } })), 10);
        index.insert(20, &filter(json!({})), 20);
        index.insert(5, &filter(json!({ "input.a": { "$in": [1, 2] } })), 5);
        index.insert(
            25,
            &filter(json!({ "caller.worker_name": { "$eq": "w" } })),
            25,
        );
        let doc = json!({ "input": { "a": 1, "b": 1 }, "caller": { "worker_name": "w" } });
        assert_eq!(matched(&index, &doc), [5, 10, 20, 25, 30]);
    }

    #[test]
    fn removal_unfiles_an_entry_from_every_value_and_path() {
        let mut index = FilterIndex::new();
        index.insert(1, &filter(json!({ "input.a": { "$in": [1, 2, 2.0] } })), 1);
        index.insert(2, &filter(json!({ "input.a": { "$eq": 2 } })), 2);
        let before = index.clone();
        assert_eq!(index.remove(&1), Some(1));
        assert_eq!(index.remove(&1), None);
        assert_eq!(matched(&index, &json!({ "input": { "a": 2 } })), [2]);
        assert!(matched(&index, &json!({ "input": { "a": 1 } })).is_empty());
        assert_eq!(index.remove(&2), Some(2));
        assert!(index.paths.is_empty() && index.scanned.is_empty() && index.is_empty());
        assert!(index.filed.is_empty());
        // The version taken before is untouched.
        assert_eq!(matched(&before, &json!({ "input": { "a": 2 } })), [1, 2]);
        assert_eq!(before.len(), 2);
    }

    #[test]
    fn an_insert_under_a_held_key_replaces_its_entry() {
        let mut index = FilterIndex::new();
        index.insert(1, &filter(json!({ "input.a": { "$eq": 1 } })), 10);
        assert_eq!(
            index.insert(1, &filter(json!({ "input.a": { "$eq": 2 } })), 11),
            Some(10)
        );
        assert_eq!(index.len(), 1);
        assert!(matched(&index, &json!({ "input": { "a": 1 } })).is_empty());
        assert_eq!(matched(&index, &json!({ "input": { "a": 2 } })), [11]);
        assert_eq!(index.get(&1), Some(&11));
    }

    #[test]
    fn an_entry_goes_under_the_condition_fewest_entries_share() {
        let mut index = FilterIndex::new();
        for i in 0..10u32 {
            let src = json!({ "input.op": { "$eq": "insert" }, "input.table": { "$eq": format!("t{i}") } });
            index.insert(i, &filter(src), i);
        }
        // The first goes under `input.op` (the first condition, nothing
        // shared yet); every later one under its own table.
        let doc = json!({ "input": { "op": "insert", "table": "t7" } });
        assert_eq!(index.candidates(&Doc::from_value(&doc)), 2);
        assert_eq!(matched(&index, &doc), [7]);
    }
}
