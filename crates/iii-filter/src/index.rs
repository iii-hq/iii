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
//! candidate, however many filters the index holds. The lists a document
//! hits are merged in key order: a few by comparing their heads, more
//! through a heap of their heads, so a document that hits `s` lists costs
//! `O(log s)` per candidate. Where the lookups would spare little, the
//! index evaluates every filter in key order, as a scan does: when it files
//! fewer than two entries per indexed path, a lookup costs about what the
//! evaluation it spares costs, and when a document's lookups leave half the
//! entries or more, merging them costs more than evaluating the rest. A
//! match therefore costs at most about a scan, plus one lookup per indexed
//! path.
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
use std::collections::BinaryHeap;
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
    slot: Slot<T>,
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
    /// condition `doc` fails. It evaluates every entry's filter instead,
    /// when it files fewer than two entries per indexed path (without a
    /// lookup), or when the lookups leave half the entries or more.
    pub fn matches<'s, 'd>(&'s self, doc: &Doc<'d>) -> Matches<'s, 'd, K, T>
    where
        K: Ord,
    {
        let len = self.len();
        let mut streams = Streams::None;
        let mut every = self.paths.len() * 2 > len;
        if !every {
            let mut candidates = self.scanned.len();
            for (path, index) in self.paths.iter() {
                if let Some(bucket) = index.bucket(path, doc) {
                    candidates += bucket.len();
                    streams.push(bucket);
                }
            }
            every = candidates > 0 && candidates * 2 >= len;
        }
        if every {
            streams = self.every();
        } else {
            if !self.scanned.is_empty() {
                streams.push(&self.scanned);
            }
            streams.heap_many();
        }
        Matches { doc: *doc, streams }
    }

    /// Every entry, in key order, each to be evaluated.
    #[cold]
    #[inline(never)]
    fn every(&self) -> Streams<'_, K, T> {
        if self.scanned.is_empty() {
            Streams::Filed(self.filed.iter())
        } else if self.filed.is_empty() {
            Streams::One(self.scanned.iter())
        } else {
            Streams::Every(Box::new(Every {
                filed: Stream::of(self.filed.iter()),
                scanned: Stream::of(self.scanned.iter()),
            }))
        }
    }

    /// How many entries `doc`'s lookups leave: those filed under `doc`'s
    /// values of the indexed paths, and those filed under no condition.
    /// Every other entry has a condition `doc` fails. A match evaluates
    /// these filters, or every filter (see [`Self::matches`]).
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
            None => self.filed.get(key).map(|filed| &filed.slot.value),
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
        self.filed.insert(key, Filed { slot, condition });
        replaced
    }

    /// Takes the entry under `key` out; returns its value. `O(log n)`, plus
    /// the number of values it is filed under.
    pub fn remove(&mut self, key: &K) -> Option<T> {
        if let Some(slot) = self.scanned.remove(key) {
            return Some(slot.value);
        }
        let Filed { slot, condition } = self.filed.remove(key)?;
        let value = slot.value;
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

/// A condition [`FilterIndex::condition_for`] may file an entry under.
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
    /// At most [`FEW_LISTS`]: the next candidate is the smallest head, found
    /// by comparing each.
    Few(Vec<Stream<'s, K, Slot<T>>>),
    /// More: their heads in a heap, smallest first.
    Heap(Heap<'s, K, T>),
    /// Every entry, when none is scanned.
    Filed(pmap::Iter<'s, K, Filed<T>>),
    /// Every entry: the filed ones and the scanned ones, merged (boxed: a
    /// list's iterator holds its stack inline, and the other plans keep the
    /// match the size of one).
    Every(Box<Every<'s, K, T>>),
}

/// Every entry of an index, merged in key order.
struct Every<'s, K, T> {
    filed: Stream<'s, K, Filed<T>>,
    scanned: Stream<'s, K, Slot<T>>,
}

/// How many lists a match merges by comparing their heads; past it, a heap
/// of the heads costs fewer comparisons per candidate.
const FEW_LISTS: usize = 4;

/// Many lists merged through a heap of their heads.
struct Heap<'s, K, T> {
    heads: BinaryHeap<Head<'s, K, T>>,
    /// Each list past its head.
    rests: Vec<pmap::Iter<'s, K, Slot<T>>>,
}

/// The next entry of list `list`.
struct Head<'s, K, T> {
    key: &'s K,
    slot: &'s Slot<T>,
    list: usize,
}

impl<K: Ord, T> PartialEq for Head<'_, K, T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<K: Ord, T> Eq for Head<'_, K, T> {}

impl<K: Ord, T> PartialOrd for Head<'_, K, T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Reversed, so the [`BinaryHeap`] (a max-heap) yields the smallest key
/// first. Keys are unique in an index, so no two heads compare equal.
impl<K: Ord, T> Ord for Head<'_, K, T> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.key.cmp(self.key)
    }
}

/// One list, with its next entry read ahead.
struct Stream<'s, K, V> {
    head: Option<(&'s K, &'s V)>,
    rest: pmap::Iter<'s, K, V>,
}

impl<'s, K, V> Stream<'s, K, V> {
    fn of(mut rest: pmap::Iter<'s, K, V>) -> Self {
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
            Streams::One(first) => Streams::Few(vec![Stream::of(first), Stream::of(list.iter())]),
            Streams::Few(mut streams) => {
                streams.push(Stream::of(list.iter()));
                Streams::Few(streams)
            }
            streams @ (Streams::Heap(_) | Streams::Filed(_) | Streams::Every(_)) => streams,
        };
    }

    /// Puts more than [`FEW_LISTS`] lists in a heap of their heads, in
    /// place (the lists' iterators are large: a match moves them once).
    #[inline]
    fn heap_many(&mut self)
    where
        K: Ord,
    {
        if matches!(self, Streams::Few(streams) if streams.len() > FEW_LISTS) {
            self.heap_of_heads();
        }
    }

    #[cold]
    #[inline(never)]
    fn heap_of_heads(&mut self)
    where
        K: Ord,
    {
        let Streams::Few(streams) = self else {
            return;
        };
        let mut heads = BinaryHeap::with_capacity(streams.len());
        let mut rests = Vec::with_capacity(streams.len());
        for (list, Stream { head, rest }) in std::mem::take(streams).into_iter().enumerate() {
            if let Some((key, slot)) = head {
                heads.push(Head { key, slot, list });
            }
            rests.push(rest);
        }
        *self = Streams::Heap(Heap { heads, rests });
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
            Some((key, &filed.slot.value))
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
            Streams::Filed(list) => list.next().map(|(key, entry)| (key, &entry.slot)),
            Streams::Every(every) => every.next(),
            Streams::Heap(heap) => heap.next(),
            Streams::Few(streams) => {
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

impl<'s, K: Ord, T> Every<'s, K, T> {
    /// The smaller head of the two lists.
    fn next(&mut self) -> Option<(&'s K, &'s Slot<T>)> {
        let filed_first = match (self.filed.head, self.scanned.head) {
            (None, None) => return None,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some((a, _)), Some((b, _))) => a < b,
        };
        if filed_first {
            let (key, entry) = self.filed.head.take()?;
            self.filed.head = self.filed.rest.next();
            Some((key, &entry.slot))
        } else {
            let (key, slot) = self.scanned.head.take()?;
            self.scanned.head = self.scanned.rest.next();
            Some((key, slot))
        }
    }
}

impl<'s, K: Ord, T> Heap<'s, K, T> {
    /// The smallest head, and the next entry of its list in its place.
    fn next(&mut self) -> Option<(&'s K, &'s Slot<T>)> {
        let Head { key, slot, list } = self.heads.pop()?;
        if let Some((key, slot)) = self.rests[list].next() {
            self.heads.push(Head { key, slot, list });
        }
        Some((key, slot))
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

    /// The plan a match of `doc` follows.
    fn plan(index: &FilterIndex<u32, u32>, doc: &Value) -> &'static str {
        match index.matches(&Doc::from_value(doc)).streams {
            Streams::None => "none",
            Streams::One(_) => "one",
            Streams::Few(_) => "few",
            Streams::Heap(_) => "heap",
            Streams::Filed(_) | Streams::Every(_) => "every",
        }
    }

    /// However many lists a document hits, their entries come out in key
    /// order, each once: a few merged by comparing heads, more through the
    /// heap of their heads.
    #[test]
    fn the_lists_a_document_hits_merge_in_key_order() {
        for paths in 1..=12u32 {
            let mut index = FilterIndex::new();
            let mut input = serde_json::Map::new();
            let mut expected = Vec::new();
            // Keys interleaved across the paths, four values on each, and
            // every thirteenth key in the scanned list; the document has `1`
            // on every path.
            for key in 0..240u32 {
                let path = key % paths;
                input.insert(format!("p{path}"), json!(1));
                let src = if key % 13 == 12 {
                    expected.push(key);
                    json!({ format!("input.p{path}"): { "$gte": 1 } })
                } else {
                    let value = (key / paths) % 4 + 1;
                    if value == 1 {
                        expected.push(key);
                    }
                    json!({ format!("input.p{path}"): { "$eq": value } })
                };
                index.insert(key, &filter(src), key);
            }
            let doc = Value::Object(serde_json::Map::from_iter([(
                "input".to_owned(),
                Value::Object(input),
            )]));
            let lists = paths as usize + 1;
            let wanted = if lists > FEW_LISTS { "heap" } else { "few" };
            assert_eq!(plan(&index, &doc), wanted, "{lists} lists");
            assert_eq!(matched(&index, &doc), expected, "{lists} lists");
        }
    }

    /// Every filter is evaluated, in key order, where lookups would spare
    /// little: an index filing fewer than two entries per path, and a
    /// document whose lookups leave half the entries or more.
    #[test]
    fn where_lookups_spare_little_every_filter_is_evaluated() {
        let mut index = FilterIndex::new();
        for key in 0..8u32 {
            let src = json!({ format!("input.p{key}"): { "$eq": 1 } });
            index.insert(key, &filter(src), key);
        }
        index.insert(8, &filter(json!({ "input.p0": { "$exists": true } })), 8);
        let doc = json!({ "input": { "p0": 1, "p3": 1, "p5": 2 } });
        assert_eq!(plan(&index, &doc), "every", "eight paths for nine entries");
        assert_eq!(matched(&index, &doc), [0, 3, 8]);

        let mut index = FilterIndex::new();
        for key in 0..10u32 {
            let value = u32::from(key >= 5);
            index.insert(key, &filter(json!({ "input.k": { "$eq": value } })), key);
        }
        let half = json!({ "input": { "k": 1 } });
        assert_eq!(plan(&index, &half), "every", "the lookup leaves half");
        assert_eq!(matched(&index, &half), [5, 6, 7, 8, 9]);
        assert_eq!(index.candidates(&Doc::from_value(&half)), 5);
        let none = json!({ "input": { "k": 2 } });
        assert_eq!(plan(&index, &none), "none");
        assert!(matched(&index, &none).is_empty());
        index.insert(10, &filter(json!({ "input.k": { "$eq": 2 } })), 10);
        assert_eq!(plan(&index, &none), "one");
        assert_eq!(matched(&index, &none), [10]);
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
        // Only the condition an entry is filed under rules it out: filed
        // under `t7`, the seventh is evaluated for a call on `t7` whatever
        // its `op`, and the first, filed under `insert`, is not.
        let doc = json!({ "input": { "op": "delete", "table": "t7" } });
        assert_eq!(index.candidates(&Doc::from_value(&doc)), 1);
        assert!(matched(&index, &doc).is_empty());
        let doc = json!({ "input": { "op": "delete", "table": "t0" } });
        assert_eq!(index.candidates(&Doc::from_value(&doc)), 0);
    }
}
