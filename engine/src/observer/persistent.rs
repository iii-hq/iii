// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! A persistent ordered set: a B+ tree whose nodes are shared between
//! versions. Adding or removing one value copies the nodes on its path
//! (`O(log n)` nodes of at most [`MAX`] slots each) and shares every other
//! node with the version it was made from, which stays valid for whoever
//! holds it. The routed hook holds one version per call: the active
//! observers of a function, in creation order (see [`super::ActiveSet`]).
//!
//! Each node is one allocation, its slots right after its reference count,
//! and a leaf holds the values themselves, each ordered by its own key: a
//! scan reads a leaf as one slice of values. Up to [`MAX`] values fit in one
//! leaf, which [`PersistentSet::leaves`] and [`PersistentSet::iter`] read
//! without walking any branch.

use std::{iter, sync::Arc};

/// Values in a leaf, or children of a branch, at most.
const MAX: usize = 128;
/// A node other than the root with fewer is merged with a neighbour.
const MIN: usize = MAX / 4;
/// What a node holds when a set is built from a sorted list.
const FILL: usize = MAX * 3 / 4;
/// Levels a set has at most: every node but the root holds at least [`MIN`],
/// so 9 levels hold more than 2^40 values.
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
    Leaf(Arc<[V]>),
    /// Sorted by key, all at the same depth.
    Branch(Arc<[Child<V>]>),
}

impl<V> Clone for Node<V> {
    fn clone(&self) -> Self {
        match self {
            Node::Leaf(values) => Node::Leaf(values.clone()),
            Node::Branch(children) => Node::Branch(children.clone()),
        }
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
            Node::Leaf(values) => values.len(),
            Node::Branch(children) => children.len(),
        }
    }
}

impl<V: Keyed> Node<V> {
    /// The smallest key. Only asked of a node that is not empty.
    fn first(&self) -> u64 {
        match self {
            Node::Leaf(values) => values[0].key(),
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

/// Where a node grows: in the middle, or at the end of the last node of its
/// level, where new observers go.
#[derive(Clone, Copy, PartialEq)]
enum Growth {
    Inside,
    Appended,
}

/// `items` (`len` of them) as one node, or two when there are more than
/// [`MAX`]: even halves, or, for a node appended to, a lower node of
/// `MAX - MIN` and an upper one of `MIN + 1`, which the next appends fill.
/// One allocation per node made.
fn sized<T: Clone, V>(
    items: impl Iterator<Item = T> + Clone,
    len: usize,
    node: fn(Arc<[T]>) -> Node<V>,
    growth: Growth,
) -> Grown<V> {
    #[cfg(test)]
    super::work::add(len);
    if len <= MAX {
        return Grown::One(node(items.collect()));
    }
    let lower = match growth {
        Growth::Inside => len / 2,
        Growth::Appended => len - (MIN + 1),
    };
    Grown::Split(
        node(items.clone().take(lower).collect()),
        node(items.skip(lower).collect()),
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
        let mut level: Vec<Child<V>> = chunks(values)
            .into_iter()
            .map(|leaf| Child::of(Node::Leaf(leaf.into())))
            .collect();
        while level.len() > 1 {
            level = chunks(level)
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

    /// The value under `key`.
    pub(crate) fn get(&self, key: u64) -> Option<&V> {
        let mut node = self.root.as_ref()?;
        loop {
            match node {
                Node::Leaf(values) => {
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
                root: Some(Node::Leaf(Arc::new([value]))),
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
        let mut root = remove(self.root.as_ref()?, key)?;
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
            Some(Node::Leaf(values)) => Leaves {
                next: Some(values),
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

/// `items` cut into runs of [`MIN`] to [`MAX`] (one shorter run when there
/// are fewer than [`MIN`] in all), as even as they come.
fn chunks<T>(items: Vec<T>) -> Vec<Vec<T>> {
    #[cfg(test)]
    super::work::add(items.len());
    let count = items.len().div_ceil(FILL).max(1);
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
/// set.
fn insert<V: Keyed + Clone>(node: &Node<V>, value: V, last: Growth) -> Option<Grown<V>> {
    let key = value.key();
    match node {
        Node::Leaf(values) => {
            let at = values.binary_search_by_key(&key, Keyed::key).err()?;
            let growth = if at == values.len() {
                last
            } else {
                Growth::Inside
            };
            let items = values[..at]
                .iter()
                .cloned()
                .chain(iter::once(value))
                .chain(values[at..].iter().cloned());
            Some(sized(items, values.len() + 1, Node::Leaf, growth))
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
            Some(sized(items, len, Node::Branch, growth))
        }
    }
}

/// `node` without `key`, possibly empty or under [`MIN`] (its parent merges
/// it); `None` when it holds no `key`.
fn remove<V: Keyed + Clone>(node: &Node<V>, key: u64) -> Option<Node<V>> {
    match node {
        Node::Leaf(values) => {
            let at = values.binary_search_by_key(&key, Keyed::key).ok()?;
            #[cfg(test)]
            super::work::add(values.len() - 1);
            Some(Node::Leaf(
                values[..at]
                    .iter()
                    .chain(&values[at + 1..])
                    .cloned()
                    .collect(),
            ))
        }
        Node::Branch(children) => {
            let at = child_for(children, key);
            let child = remove(&children[at].node, key)?;
            let next: Vec<Child<V>> = if child.width() == 0 {
                children[..at]
                    .iter()
                    .chain(&children[at + 1..])
                    .cloned()
                    .collect()
            } else if child.width() < MIN && children.len() > 1 {
                // Joined with a neighbour, as one node or two even halves.
                let (left, joined) = if at + 1 < children.len() {
                    (at, join(&child, &children[at + 1].node))
                } else {
                    (at - 1, join(&children[at - 1].node, &child))
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

/// Two neighbouring nodes of the same depth, `lower` first, as one.
fn join<V: Keyed + Clone>(lower: &Node<V>, upper: &Node<V>) -> Grown<V> {
    match (lower, upper) {
        (Node::Leaf(a), Node::Leaf(b)) => sized(
            a.iter().chain(b.iter()).cloned(),
            a.len() + b.len(),
            Node::Leaf,
            Growth::Inside,
        ),
        (Node::Branch(a), Node::Branch(b)) => sized(
            a.iter().chain(b.iter()).cloned(),
            a.len() + b.len(),
            Node::Branch,
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
                Node::Leaf(values) => return values,
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
                        Node::Leaf(values) => values,
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

    /// The depth of every leaf, and whether every node but the root holds at
    /// least `MIN` and at most `MAX`, with each branch's `first` right.
    fn check<V: Keyed + Clone>(set: &PersistentSet<V>) -> usize {
        fn walk<V: Keyed>(
            node: &Node<V>,
            depth: usize,
            root: bool,
            depths: &mut Vec<usize>,
        ) -> usize {
            let width = node.width();
            assert!(width <= MAX, "{width} > MAX");
            assert!(root || width >= MIN, "{width} < MIN below the root");
            match node {
                Node::Leaf(values) => {
                    assert!(values.windows(2).all(|pair| pair[0].key() < pair[1].key()));
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
                        .map(|child| {
                            assert_eq!(child.first, child.node.first());
                            walk(&child.node, depth + 1, false, depths)
                        })
                        .sum()
                }
            }
        }
        let mut depths = Vec::new();
        let counted = set
            .root
            .as_ref()
            .map_or(0, |root| walk(root, 1, true, &mut depths));
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
