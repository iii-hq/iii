// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! A persistent ordered set: a B+ tree whose nodes are shared between
//! versions. Adding or removing one value copies at most the nodes on its
//! path (`O(log n)` nodes of at most [`MAX`] slots each) and shares every
//! other node with the version it was made from, which stays valid for
//! whoever holds it. The routed hook holds one version per call: the active
//! observers of a function, in creation order (see [`super::ActiveSet`]).
//!
//! A leaf is a run of one [`Chunk`]'s slots, each holding a value, ordered by
//! the values' keys: a scan reads a leaf as one slice of values. Up to [`MAX`]
//! values fit in one leaf, which [`PersistentSet::leaves`] and
//! [`PersistentSet::iter`] read without walking any branch. A chunk's slots
//! are written once each, front to back, so the two changes the active
//! observers see most copy no leaf: a value past every key (a new observer)
//! goes into the next slot of the last leaf's chunk, which no version reads
//! yet, and the first value of a leaf (its oldest observer) leaves it by the
//! leaf starting one slot later. A leaf that has passed over [`SKIPPED`]
//! values that way is copied without them.

use std::{
    cell::UnsafeCell,
    iter,
    mem::MaybeUninit,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Values in a leaf at most.
const MAX: usize = 128;
/// A leaf other than the root with fewer is merged with a neighbour, but the
/// last leaf, which appends fill.
const MIN: usize = MAX / 4;
/// What a leaf holds when a set is built from a sorted list.
const FILL: usize = MAX * 3 / 4;
/// Children of a branch at most: fewer than a leaf holds values, since every
/// change copies the branches on its path.
const BRANCH_MAX: usize = 32;
/// A branch other than the root with fewer is merged with a neighbour.
const BRANCH_MIN: usize = BRANCH_MAX / 4;
/// What a branch holds when a set is built from a sorted list.
const BRANCH_FILL: usize = BRANCH_MAX * 3 / 4;
/// Values a leaf passes over at its front, removed from it without a copy,
/// before it is copied without them: the most a chunk keeps of values no
/// version holds anymore.
const SKIPPED: usize = MIN;
/// Levels a set has at most: every node but the root and the last of each
/// level holds at least [`MIN`] or [`BRANCH_MIN`], so 13 levels hold more
/// than 2^40 values.
const MAX_DEPTH: usize = 16;

/// What a [`PersistentSet`] orders its values by. A value's key never
/// changes while a set holds it.
pub(crate) trait Keyed {
    fn key(&self) -> u64;
}

/// Values ordered by their [`Keyed::key`], each key at most once.
pub(crate) struct PersistentSet<V> {
    root: Option<Node<V>>,
    len: usize,
}

enum Node<V> {
    /// Sorted by key.
    Leaf(Leaf<V>),
    /// Sorted by key, all at the same depth.
    Branch(Arc<[Child<V>]>),
}

impl<V> Clone for Node<V> {
    fn clone(&self) -> Self {
        match self {
            Node::Leaf(leaf) => Node::Leaf(leaf.clone()),
            Node::Branch(children) => Node::Branch(children.clone()),
        }
    }
}

/// Slots written once each, front to back: `[0, written)` hold values in
/// key order, the rest nothing yet. Leaves of several versions read runs of
/// the same chunk; a slot is written before any leaf over it exists, and
/// never again.
struct Chunk<V> {
    /// Slots written. It only grows: an append claims the next slot by
    /// moving it on, then writes the slot.
    written: AtomicUsize,
    slots: Box<[UnsafeCell<MaybeUninit<V>>]>,
}

// SAFETY: the values are shared between threads only as `&V` (a slot is
// written by the one append that claimed it, before any leaf that reads it
// exists, and is read only after that), and dropped with the chunk, wherever
// its last reference goes.
unsafe impl<V: Send + Sync> Send for Chunk<V> {}
// SAFETY: as for `Send`: concurrent users read written slots and claim
// unwritten ones through `written`.
unsafe impl<V: Send + Sync> Sync for Chunk<V> {}

impl<V> Drop for Chunk<V> {
    fn drop(&mut self) {
        let written = *self.written.get_mut();
        for slot in &mut self.slots[..written] {
            // SAFETY: `[0, written)` were written, and nothing reads them
            // once the last reference to the chunk is gone.
            unsafe { slot.get_mut().assume_init_drop() };
        }
    }
}

/// The values of slots `[start, end)` of a chunk.
struct Leaf<V> {
    chunk: Arc<Chunk<V>>,
    start: u32,
    end: u32,
}

impl<V> Clone for Leaf<V> {
    fn clone(&self) -> Self {
        Self {
            chunk: self.chunk.clone(),
            start: self.start,
            end: self.end,
        }
    }
}

impl<V> Leaf<V> {
    /// A leaf of `values` (`len` of them, a run sorted by key), in a chunk
    /// with room for `room` values in all. One allocation for the chunk's
    /// slots, one for its count.
    fn of(values: impl Iterator<Item = V>, len: usize, room: usize) -> Self {
        #[cfg(test)]
        super::work::add(len);
        let mut chunk = Chunk {
            written: AtomicUsize::new(0),
            slots: (0..room.max(len))
                .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
                .collect(),
        };
        let mut written = 0;
        for value in values.take(len) {
            chunk.slots[written].get_mut().write(value);
            written += 1;
            *chunk.written.get_mut() = written;
        }
        Self {
            chunk: Arc::new(chunk),
            start: 0,
            end: written as u32,
        }
    }

    #[inline]
    fn values(&self) -> &[V] {
        let start = self.start as usize;
        let len = (self.end - self.start) as usize;
        // SAFETY: slots `[start, end)` were written before this leaf was
        // made (`end` was at most `written` then) and are never written
        // again. `UnsafeCell<MaybeUninit<V>>` has the layout of `V`.
        unsafe { std::slice::from_raw_parts(self.chunk.slots.as_ptr().add(start).cast::<V>(), len) }
    }

    fn width(&self) -> usize {
        (self.end - self.start) as usize
    }

    /// This leaf with `value`, whose key is past all of its own, written into
    /// the chunk's next slot when that slot is the one right after this
    /// leaf's values: no version reads it, and claiming it keeps any other
    /// version from writing it. `Err` hands `value` back otherwise.
    fn appended(&self, value: V) -> Result<Self, V> {
        let end = self.end as usize;
        if end >= self.chunk.slots.len()
            || self
                .chunk
                .written
                .compare_exchange(end, end + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
        {
            return Err(value);
        }
        // SAFETY: the exchange gave this append slot `end`, unwritten until
        // now and read by no leaf: every leaf over the chunk ended at or
        // before it.
        unsafe { (*self.chunk.slots[end].get()).write(value) };
        Ok(Self {
            chunk: self.chunk.clone(),
            start: self.start,
            end: self.end + 1,
        })
    }

    /// This leaf without its first value: the same chunk from the next
    /// slot, until it has passed over [`SKIPPED`]; then a copy of the rest.
    fn without_first(&self, room: usize) -> Self
    where
        V: Clone,
    {
        if (self.start as usize) < SKIPPED {
            return Self {
                chunk: self.chunk.clone(),
                start: self.start + 1,
                end: self.end,
            };
        }
        let rest = &self.values()[1..];
        Self::of(rest.iter().cloned(), rest.len(), room)
    }
}

struct Child<V> {
    /// The smallest key under `node`.
    first: u64,
    node: Node<V>,
}

impl<V> Clone for Child<V> {
    fn clone(&self) -> Self {
        Self {
            first: self.first,
            node: self.node.clone(),
        }
    }
}

impl<V: Keyed> Child<V> {
    fn of(node: Node<V>) -> Self {
        Self {
            first: node.first(),
            node,
        }
    }
}

/// What an insert made of a node: the node, or the two halves it split
/// into.
enum Grown<V> {
    One(Node<V>),
    Split(Node<V>, Node<V>),
}

impl<V> Node<V> {
    /// Values or children held.
    fn width(&self) -> usize {
        match self {
            Node::Leaf(leaf) => leaf.width(),
            Node::Branch(children) => children.len(),
        }
    }

    /// What it holds at least, but as the root or the last of its level.
    fn least(&self) -> usize {
        match self {
            Node::Leaf(_) => MIN,
            Node::Branch(_) => BRANCH_MIN,
        }
    }
}

impl<V: Keyed> Node<V> {
    /// The smallest key. Only asked of a node that is not empty.
    fn first(&self) -> u64 {
        match self {
            Node::Leaf(leaf) => leaf.values()[0].key(),
            Node::Branch(children) => children[0].first,
        }
    }
}

/// The child whose range holds `key`: the last one starting at or before it,
/// or the first.
fn child_for<V>(children: &[Child<V>], key: u64) -> usize {
    children
        .partition_point(|child| child.first <= key)
        .saturating_sub(1)
}

/// Where a node changes: in the middle, or as the last node of its level,
/// where new observers go.
#[derive(Clone, Copy, PartialEq)]
enum Growth {
    Inside,
    Appended,
}

impl Growth {
    /// The room a leaf made here gets: a last leaf takes appends in place.
    fn room(self, len: usize) -> usize {
        match self {
            Growth::Inside => len,
            Growth::Appended => MAX,
        }
    }
}

/// `items` (`len` of them) as one leaf, or two when there are more than
/// [`MAX`]: even halves. `last` says whether the leaf is the last of its
/// level, whose chunk keeps room for appends.
fn sized_leaf<V: Clone>(
    items: impl Iterator<Item = V> + Clone,
    len: usize,
    last: Growth,
) -> Grown<V> {
    if len <= MAX {
        return Grown::One(Node::Leaf(Leaf::of(items, len, last.room(len))));
    }
    let lower = len / 2;
    Grown::Split(
        Node::Leaf(Leaf::of(items.clone(), lower, lower)),
        Node::Leaf(Leaf::of(
            items.skip(lower),
            len - lower,
            last.room(len - lower),
        )),
    )
}

/// `children` (`len` of them) as one branch, or two when there are more
/// than [`BRANCH_MAX`]: even halves, or, for the last branch of its level, a
/// lower one of `BRANCH_MAX - BRANCH_MIN` and an upper one of
/// `BRANCH_MIN + 1`, which the next appends fill. One allocation per branch.
fn sized_branch<V>(
    children: impl Iterator<Item = Child<V>> + Clone,
    len: usize,
    growth: Growth,
) -> Grown<V> {
    #[cfg(test)]
    super::work::add(len);
    if len <= BRANCH_MAX {
        return Grown::One(Node::Branch(children.collect()));
    }
    let lower = match growth {
        Growth::Inside => len / 2,
        Growth::Appended => len - (BRANCH_MIN + 1),
    };
    Grown::Split(
        Node::Branch(children.clone().take(lower).collect()),
        Node::Branch(children.skip(lower).collect()),
    )
}

impl<V: Keyed + Clone> PersistentSet<V> {
    pub(crate) fn new() -> Self {
        Self { root: None, len: 0 }
    }

    /// A set holding `values`, which are sorted by key with each key once.
    pub(crate) fn from_sorted(values: Vec<V>) -> Self {
        debug_assert!(values.windows(2).all(|pair| pair[0].key() < pair[1].key()));
        let len = values.len();
        if len == 0 {
            return Self::new();
        }
        let runs = chunks(values, FILL);
        let last = runs.len() - 1;
        let mut level: Vec<Child<V>> = runs
            .into_iter()
            .enumerate()
            .map(|(at, run)| {
                let room = if at == last { MAX } else { run.len() };
                let len = run.len();
                Child::of(Node::Leaf(Leaf::of(run.into_iter(), len, room)))
            })
            .collect();
        while level.len() > 1 {
            level = chunks(level, BRANCH_FILL)
                .into_iter()
                .map(|children| Child::of(Node::Branch(children.into())))
                .collect();
        }
        Self {
            root: level.pop().map(|child| child.node),
            len,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The value under `key`.
    pub(crate) fn get(&self, key: u64) -> Option<&V> {
        let mut node = self.root.as_ref()?;
        loop {
            match node {
                Node::Leaf(leaf) => {
                    let values = leaf.values();
                    return values
                        .binary_search_by_key(&key, Keyed::key)
                        .ok()
                        .map(|at| &values[at]);
                }
                Node::Branch(children) => node = &children[child_for(children, key)].node,
            }
        }
    }

    /// A new version with `value`; `None` when its key is taken.
    pub(crate) fn inserted(&self, value: V) -> Option<Self> {
        let Some(root) = self.root.as_ref() else {
            return Some(Self {
                root: Some(Node::Leaf(Leaf::of(iter::once(value), 1, MAX))),
                len: 1,
            });
        };
        let root = match insert(root, value, Growth::Appended)? {
            Grown::One(node) => node,
            Grown::Split(lower, upper) => {
                Node::Branch(Arc::new([Child::of(lower), Child::of(upper)]))
            }
        };
        Some(Self {
            root: Some(root),
            len: self.len + 1,
        })
    }

    /// A new version without `key`; `None` when it holds no `key`.
    pub(crate) fn removed(&self, key: u64) -> Option<Self> {
        let mut root = remove(self.root.as_ref()?, key, Growth::Appended)?;
        // A branch left with one child gives way to it.
        while let Node::Branch(children) = &root
            && children.len() == 1
        {
            let only = children[0].node.clone();
            root = only;
        }
        let len = self.len - 1;
        Some(Self {
            root: (len > 0).then_some(root),
            len,
        })
    }
}

impl<V> PersistentSet<V> {
    /// The values in key order.
    #[inline]
    pub(crate) fn iter(&self) -> Iter<'_, V> {
        let mut leaves = self.leaves();
        let leaf = leaves.next().unwrap_or_default().iter();
        Iter { leaves, leaf }
    }

    /// The leaves in key order, each a slice of values: a plain slice loop
    /// over each, for the scan the routed hook runs on every call. A set
    /// that is one leaf is read as that slice, with no walk of branches.
    #[inline]
    pub(crate) fn leaves(&self) -> Leaves<'_, V> {
        match &self.root {
            None => Leaves {
                next: None,
                deeper: None,
            },
            Some(Node::Leaf(leaf)) => Leaves {
                next: Some(leaf.values()),
                deeper: None,
            },
            Some(Node::Branch(children)) => {
                let mut deeper = Deeper {
                    branches: std::array::from_fn(|_| [].iter()),
                    depth: 0,
                };
                let next = deeper.descend(children);
                Leaves {
                    next: Some(next),
                    deeper: Some(deeper),
                }
            }
        }
    }
}

impl<V> Clone for PersistentSet<V> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            len: self.len,
        }
    }
}

impl<V: Keyed + Clone> Default for PersistentSet<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, V> IntoIterator for &'a PersistentSet<V> {
    type Item = &'a V;
    type IntoIter = Iter<'a, V>;

    fn into_iter(self) -> Iter<'a, V> {
        self.iter()
    }
}

/// `items` cut into even runs of about `fill` (one shorter run when there
/// are fewer in all): between a node's least and most.
fn chunks<T>(items: Vec<T>, fill: usize) -> Vec<Vec<T>> {
    let count = items.len().div_ceil(fill).max(1);
    let (base, extra) = (items.len() / count, items.len() % count);
    let mut items = items.into_iter();
    (0..count)
        .map(|at| {
            items
                .by_ref()
                .take(base + usize::from(at < extra))
                .collect()
        })
        .collect()
}

/// `node` with `value`. `last` is [`Growth::Appended`] when `node` is the
/// last node of its level, so a key past all of its own is appended to the
/// set: in place, into the last leaf's chunk, while it has room.
fn insert<V: Keyed + Clone>(node: &Node<V>, value: V, last: Growth) -> Option<Grown<V>> {
    let key = value.key();
    match node {
        Node::Leaf(leaf) => {
            let values = leaf.values();
            let at = values.binary_search_by_key(&key, Keyed::key).err()?;
            let growth = if at == values.len() {
                last
            } else {
                Growth::Inside
            };
            if growth == Growth::Appended {
                let value = match leaf.appended(value) {
                    Ok(appended) => return Some(Grown::One(Node::Leaf(appended))),
                    Err(value) => value,
                };
                if values.len() == MAX {
                    // Full: the value starts the next leaf, and this one is
                    // shared as it is.
                    return Some(Grown::Split(
                        Node::Leaf(leaf.clone()),
                        Node::Leaf(Leaf::of(iter::once(value), 1, MAX)),
                    ));
                }
                // No room in place: a copy with room, as it is not full.
                let items = values.iter().cloned().chain(iter::once(value));
                return Some(sized_leaf(items, values.len() + 1, growth));
            }
            let items = values[..at]
                .iter()
                .cloned()
                .chain(iter::once(value))
                .chain(values[at..].iter().cloned());
            // The last leaf keeps its room for appends.
            Some(sized_leaf(items, values.len() + 1, last))
        }
        Node::Branch(children) => {
            let at = child_for(children, key);
            let growth = if at + 1 == children.len() {
                last
            } else {
                Growth::Inside
            };
            let (lower, upper) = match insert(&children[at].node, value, growth)? {
                Grown::One(child) => (Child::of(child), None),
                Grown::Split(lower, upper) => (Child::of(lower), Some(Child::of(upper))),
            };
            let len = children.len() + usize::from(upper.is_some());
            let items = children[..at]
                .iter()
                .cloned()
                .chain(iter::once(lower))
                .chain(upper)
                .chain(children[at + 1..].iter().cloned());
            Some(sized_branch(items, len, growth))
        }
    }
}

/// `node` without `key`, possibly empty or under [`MIN`] (its parent merges
/// it); `None` when it holds no `key`. `last` is [`Growth::Appended`] when
/// `node` is the last node of its level.
fn remove<V: Keyed + Clone>(node: &Node<V>, key: u64, last: Growth) -> Option<Node<V>> {
    match node {
        Node::Leaf(leaf) => {
            let values = leaf.values();
            let at = values.binary_search_by_key(&key, Keyed::key).ok()?;
            if at == 0 {
                return Some(Node::Leaf(leaf.without_first(last.room(values.len() - 1))));
            }
            let rest = values[..at].iter().chain(&values[at + 1..]).cloned();
            Some(Node::Leaf(Leaf::of(
                rest,
                values.len() - 1,
                last.room(values.len() - 1),
            )))
        }
        Node::Branch(children) => {
            let at = child_for(children, key);
            let growth = if at + 1 == children.len() {
                last
            } else {
                Growth::Inside
            };
            let child = remove(&children[at].node, key, growth)?;
            let next: Vec<Child<V>> = if child.width() == 0 {
                children[..at]
                    .iter()
                    .chain(&children[at + 1..])
                    .cloned()
                    .collect()
            } else if child.width() < child.least() && children.len() > 1 {
                // Joined with a neighbour, as one node or two even halves.
                let (left, joined) = if at + 1 < children.len() {
                    let growth = if at + 2 == children.len() {
                        last
                    } else {
                        Growth::Inside
                    };
                    (at, join(&child, &children[at + 1].node, growth))
                } else {
                    (at - 1, join(&children[at - 1].node, &child, growth))
                };
                let (lower, upper) = match joined {
                    Grown::One(node) => (Child::of(node), None),
                    Grown::Split(lower, upper) => (Child::of(lower), Some(Child::of(upper))),
                };
                children[..left]
                    .iter()
                    .cloned()
                    .chain(iter::once(lower))
                    .chain(upper)
                    .chain(children[left + 2..].iter().cloned())
                    .collect()
            } else {
                children[..at]
                    .iter()
                    .cloned()
                    .chain(iter::once(Child::of(child)))
                    .chain(children[at + 1..].iter().cloned())
                    .collect()
            };
            #[cfg(test)]
            super::work::add(next.len());
            Some(Node::Branch(next.into()))
        }
    }
}

/// Two neighbouring nodes of the same depth, `lower` first, as one, or as
/// two even halves. `upper` is the last node of its level with
/// [`Growth::Appended`].
fn join<V: Keyed + Clone>(lower: &Node<V>, upper: &Node<V>, growth: Growth) -> Grown<V> {
    match (lower, upper) {
        (Node::Leaf(a), Node::Leaf(b)) => {
            let len = a.width() + b.width();
            let items = a.values().iter().chain(b.values()).cloned();
            if len <= MAX {
                return Grown::One(Node::Leaf(Leaf::of(items, len, growth.room(len))));
            }
            let half = len / 2;
            Grown::Split(
                Node::Leaf(Leaf::of(items.clone(), half, half)),
                Node::Leaf(Leaf::of(
                    items.skip(half),
                    len - half,
                    growth.room(len - half),
                )),
            )
        }
        (Node::Branch(a), Node::Branch(b)) => sized_branch(
            a.iter().chain(b.iter()).cloned(),
            a.len() + b.len(),
            Growth::Inside,
        ),
        _ => unreachable!("neighbours are at the same depth"),
    }
}

/// The leaves of a [`PersistentSet`] in key order (see
/// [`PersistentSet::leaves`]).
pub(crate) struct Leaves<'a, V> {
    /// The leaf to yield next.
    next: Option<&'a [V]>,
    /// Where the leaves after it are, in a set deeper than one leaf.
    deeper: Option<Deeper<'a, V>>,
}

/// The branches above the leaf a [`Leaves`] is at.
struct Deeper<'a, V> {
    /// The children left to visit at each level above the current leaf.
    branches: [std::slice::Iter<'a, Child<V>>; MAX_DEPTH],
    depth: usize,
}

impl<'a, V> Deeper<'a, V> {
    /// Goes down the first children of `children` to its first leaf.
    fn descend(&mut self, mut children: &'a [Child<V>]) -> &'a [V] {
        loop {
            let mut rest = children.iter();
            let Some(first) = rest.next() else {
                return &[];
            };
            assert!(self.depth < MAX_DEPTH, "a set deeper than MAX_DEPTH");
            self.branches[self.depth] = rest;
            self.depth += 1;
            match &first.node {
                Node::Leaf(leaf) => return leaf.values(),
                Node::Branch(next) => children = next,
            }
        }
    }

    /// The next leaf: under the next child of the nearest level that has
    /// one.
    fn next_leaf(&mut self) -> Option<&'a [V]> {
        loop {
            if self.depth == 0 {
                return None;
            }
            match self.branches[self.depth - 1].next() {
                Some(child) => {
                    return Some(match &child.node {
                        Node::Leaf(leaf) => leaf.values(),
                        Node::Branch(children) => self.descend(children),
                    });
                }
                None => self.depth -= 1,
            }
        }
    }
}

impl<'a, V> Iterator for Leaves<'a, V> {
    type Item = &'a [V];

    #[inline]
    fn next(&mut self) -> Option<&'a [V]> {
        let leaf = self.next.take()?;
        self.next = self.deeper.as_mut().and_then(Deeper::next_leaf);
        Some(leaf)
    }
}

/// The values of a [`PersistentSet`] in key order.
pub(crate) struct Iter<'a, V> {
    leaves: Leaves<'a, V>,
    leaf: std::slice::Iter<'a, V>,
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = &'a V;

    #[inline]
    fn next(&mut self) -> Option<&'a V> {
        loop {
            if let Some(value) = self.leaf.next() {
                return Some(value);
            }
            self.leaf = self.leaves.next()?.iter();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    impl Keyed for u64 {
        fn key(&self) -> u64 {
            *self
        }
    }

    /// The depth of every leaf, and whether every node but the root and the
    /// last of its level holds at least `MIN`, and every node at most `MAX`,
    /// with each branch's `first` right and each leaf within its chunk's
    /// written slots.
    fn check<V: Keyed + Clone>(set: &PersistentSet<V>) -> usize {
        fn walk<V: Keyed>(
            node: &Node<V>,
            depth: usize,
            root: bool,
            last: bool,
            depths: &mut Vec<usize>,
        ) -> usize {
            let width = node.width();
            let most = match node {
                Node::Leaf(_) => MAX,
                Node::Branch(_) => BRANCH_MAX,
            };
            assert!(width <= most, "{width} > {most}");
            assert!(
                root || last || width >= node.least(),
                "{width} < {} below the root",
                node.least()
            );
            match node {
                Node::Leaf(leaf) => {
                    let values = leaf.values();
                    assert!(values.windows(2).all(|pair| pair[0].key() < pair[1].key()));
                    assert!(leaf.end as usize <= leaf.chunk.written.load(Ordering::Acquire));
                    assert!((leaf.start as usize) <= SKIPPED);
                    depths.push(depth);
                    values.len()
                }
                Node::Branch(children) => {
                    assert!(!root || children.len() >= 2, "a root branch of one");
                    assert!(
                        children
                            .windows(2)
                            .all(|pair| pair[0].first < pair[1].first)
                    );
                    children
                        .iter()
                        .enumerate()
                        .map(|(at, child)| {
                            assert_eq!(child.first, child.node.first());
                            let last = last && at + 1 == children.len();
                            walk(&child.node, depth + 1, false, last, depths)
                        })
                        .sum()
                }
            }
        }
        let mut depths = Vec::new();
        let counted = set
            .root
            .as_ref()
            .map_or(0, |root| walk(root, 1, true, true, &mut depths));
        assert_eq!(counted, set.len);
        assert!(depths.windows(2).all(|pair| pair[0] == pair[1]), "balanced");
        depths.first().copied().unwrap_or(0)
    }

    fn contents(set: &PersistentSet<u64>) -> Vec<u64> {
        let values: Vec<u64> = set.iter().copied().collect();
        let by_leaf: Vec<u64> = set.leaves().flat_map(|leaf| leaf.iter().copied()).collect();
        assert_eq!(values, by_leaf, "the leaves hold what the iterator yields");
        values
    }

    /// A small deterministic generator (xorshift), so a failure reproduces.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// Inserts and removes in random order agree with a `BTreeMap` at every
    /// step, the tree stays balanced and filled, and every earlier version
    /// still reads as it was.
    #[test]
    fn random_inserts_and_removes_match_a_btreemap() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut set = PersistentSet::new();
        let mut model = BTreeMap::new();
        let mut versions: Vec<(PersistentSet<u64>, Vec<u64>)> = Vec::new();
        for step in 0..20_000 {
            let key = rng.next() % 3_000;
            if rng.next().is_multiple_of(3) {
                match set.removed(key) {
                    Some(next) => {
                        assert!(model.remove(&key).is_some());
                        set = next;
                    }
                    None => assert!(!model.contains_key(&key)),
                }
            } else {
                match set.inserted(key) {
                    Some(next) => {
                        assert!(model.insert(key, key).is_none());
                        set = next;
                    }
                    None => assert!(model.contains_key(&key)),
                }
            }
            if step % 997 == 0 {
                check(&set);
                assert_eq!(contents(&set), model.keys().copied().collect::<Vec<_>>());
                versions.push((set.clone(), contents(&set)));
            }
            assert_eq!(set.len, model.len());
            assert_eq!(set.get(key).is_some(), model.contains_key(&key));
        }
        for (version, expected) in &versions {
            assert_eq!(&contents(version), expected, "an earlier version changed");
        }
        // Down to nothing, removing in random order.
        let mut keys: Vec<u64> = model.keys().copied().collect();
        while !keys.is_empty() {
            let key = keys.swap_remove((rng.next() % keys.len() as u64) as usize);
            set = set.removed(key).expect("held");
            check(&set);
        }
        assert!(set.is_empty());
        assert_eq!(set.iter().count(), 0);
        assert_eq!(set.leaves().count(), 0);
    }

    /// Appending in order, the common case (new observers are the newest),
    /// keeps the tree shallow; so does building from a sorted list.
    #[test]
    fn appends_and_bulk_builds_stay_shallow() {
        let mut set = PersistentSet::new();
        for key in 0..50_000u64 {
            set = set.inserted(key).unwrap();
        }
        let depth = check(&set);
        assert!(depth <= 5, "depth {depth}");
        assert_eq!(contents(&set), (0..50_000).collect::<Vec<_>>());

        for len in [0, 1, MIN - 1, MIN, FILL, MAX, MAX + 1, 1_000, 50_000] {
            let built = PersistentSet::from_sorted((0..len as u64).collect());
            check(&built);
            assert_eq!(contents(&built), (0..len as u64).collect::<Vec<_>>());
        }
    }

    /// A value that counts how many of it are alive, clones included, so a
    /// test sees each one dropped once, whichever versions held it.
    struct Counted(u64, Arc<AtomicUsize>);

    impl Counted {
        fn new(key: u64, alive: &Arc<AtomicUsize>) -> Self {
            alive.fetch_add(1, Ordering::Relaxed);
            Self(key, alive.clone())
        }
    }

    impl Clone for Counted {
        fn clone(&self) -> Self {
            Self::new(self.0, &self.1)
        }
    }

    impl Keyed for Counted {
        fn key(&self) -> u64 {
            self.0
        }
    }

    impl Drop for Counted {
        fn drop(&mut self) {
            self.1.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Every value made is dropped once all the versions that held it are:
    /// nothing written into a chunk is lost or dropped twice, whether it
    /// went in place or in a copy.
    #[test]
    fn every_value_is_dropped_once() {
        let alive = Arc::new(AtomicUsize::new(0));
        let mut rng = Rng(0x5851_f42d_4c95_7f2d);
        {
            let mut set = PersistentSet::new();
            let mut versions = Vec::new();
            let mut next = 0u64;
            for step in 0..20_000 {
                let roll = rng.next() % 10;
                if roll < 6 {
                    // Mostly appends, sometimes a key inside.
                    let key = if roll == 0 {
                        rng.next() % (next + 1)
                    } else {
                        next
                    };
                    next += 1;
                    let value = Counted::new(key * 2 + 1, &alive);
                    if let Some(grown) = set.inserted(value) {
                        set = grown;
                    }
                } else if let Some(first) = set.iter().next().map(|v: &Counted| v.0) {
                    let key = if roll < 9 {
                        first
                    } else {
                        rng.next() % (next * 2 + 2)
                    };
                    if let Some(shrunk) = set.removed(key) {
                        set = shrunk;
                    }
                }
                if step % 101 == 0 {
                    versions.push(set.clone());
                }
                if versions.len() > 20 {
                    versions.remove((rng.next() % 20) as usize);
                }
            }
            check(&set);
            for version in &versions {
                check(version);
            }
        }
        assert_eq!(alive.load(Ordering::Relaxed), 0, "each value dropped once");
    }

    /// The commonest changes copy no leaf: a value past every key goes into
    /// the next slot of the last leaf's chunk, and the first value leaves a
    /// leaf by the leaf starting a slot later, until it has passed over
    /// `SKIPPED`. An earlier version reads as it was through both.
    #[test]
    fn appends_and_front_removals_copy_no_leaf() {
        fn chunks(set: &PersistentSet<u64>) -> Vec<*const Chunk<u64>> {
            fn walk(node: &Node<u64>, out: &mut Vec<*const Chunk<u64>>) {
                match node {
                    Node::Leaf(leaf) => out.push(Arc::as_ptr(&leaf.chunk)),
                    Node::Branch(children) => children.iter().for_each(|c| walk(&c.node, out)),
                }
            }
            let mut out = Vec::new();
            if let Some(root) = &set.root {
                walk(root, &mut out);
            }
            out
        }
        let mut set = PersistentSet::from_sorted((0..1_000u64).collect());
        let before = set.clone();
        super::super::work::take();
        let mut new_chunks = 0;
        for key in 1_000..1_100u64 {
            let next = set.inserted(key).unwrap();
            let (was, now) = (chunks(&set), chunks(&next));
            new_chunks += now.iter().filter(|c| !was.contains(c)).count();
            set = next;
        }
        // Only a leaf filling up starts another; the branch above the last
        // leaf is copied, one slot a child, on each.
        assert_eq!(new_chunks, 1, "one leaf filled up");
        let appended = super::super::work::take();
        assert!(
            appended < 100 * 16,
            "{appended} slots copied for 100 appends"
        );
        let mut new_chunks = 0;
        for key in 0..20u64 {
            let next = set.removed(key).unwrap();
            let (was, now) = (chunks(&set), chunks(&next));
            new_chunks += now.iter().filter(|c| !was.contains(c)).count();
            set = next;
        }
        assert_eq!(new_chunks, 0, "the first leaf starts later each time");
        let removed = super::super::work::take();
        assert!(
            removed < 20 * 16,
            "{removed} slots copied for 20 front removals"
        );
        check(&set);
        assert_eq!(contents(&set), (20..1_100).collect::<Vec<_>>());
        assert_eq!(contents(&before), (0..1_000).collect::<Vec<_>>());

        // Two versions appending past the same leaf: the first writes in
        // place, the second copies.
        let base = PersistentSet::from_sorted((0..10u64).collect());
        let a = base.inserted(10).unwrap();
        let b = base.inserted(11).unwrap();
        assert_eq!(contents(&a), (0..=10).collect::<Vec<_>>());
        assert_eq!(contents(&b), (0..10).chain([11]).collect::<Vec<_>>());
        assert_eq!(contents(&base), (0..10).collect::<Vec<_>>());
    }

    /// Removing from the front, as old observers go first, merges what
    /// empties out, leaving no trail of thin leaves.
    #[test]
    fn removing_from_the_front_keeps_the_tree_filled() {
        let mut set = PersistentSet::from_sorted((0..10_000u64).collect());
        for key in 0..9_990u64 {
            set = set.removed(key).unwrap();
            if key % 501 == 0 {
                check(&set);
            }
        }
        assert_eq!(check(&set), 1, "ten values fit in one leaf");
        assert_eq!(contents(&set), (9_990..10_000).collect::<Vec<_>>());
    }
}
