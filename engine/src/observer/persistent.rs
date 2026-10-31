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

use std::sync::Arc;

/// Values in a leaf, or children of a branch, at most.
const MAX: usize = 32;
/// A node other than the root with fewer is merged with a neighbour.
const MIN: usize = MAX / 4;
/// What a node holds when a set is built from a sorted list.
const FILL: usize = MAX * 3 / 4;
/// Levels a set has at most: every node but the root holds at least [`MIN`],
/// so 14 levels hold more than 2^40 values.
const MAX_DEPTH: usize = 16;

/// Values ordered by a `u64` key, each key at most once.
pub(crate) struct PersistentSet<V> {
    root: Option<Arc<Node<V>>>,
    len: usize,
}

enum Node<V> {
    /// Sorted by key.
    Leaf(Vec<(u64, V)>),
    /// Sorted by key, all at the same depth.
    Branch(Vec<Child<V>>),
}

struct Child<V> {
    /// The smallest key under `node`.
    first: u64,
    node: Arc<Node<V>>,
}

impl<V> Clone for Child<V> {
    fn clone(&self) -> Self {
        Self {
            first: self.first,
            node: self.node.clone(),
        }
    }
}

impl<V> Child<V> {
    fn of(node: Node<V>) -> Self {
        Self {
            first: node.first(),
            node: Arc::new(node),
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

    /// The smallest key. Only asked of a node that is not empty.
    fn first(&self) -> u64 {
        match self {
            Node::Leaf(values) => values[0].0,
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

/// `items` as one node, or two halves when it holds more than [`MAX`].
fn sized<T, V>(mut items: Vec<T>, node: fn(Vec<T>) -> Node<V>) -> Grown<V> {
    #[cfg(test)]
    super::work::add(items.len());
    if items.len() <= MAX {
        return Grown::One(node(items));
    }
    let upper = items.split_off(items.len() / 2);
    Grown::Split(node(items), node(upper))
}

impl<V: Clone> PersistentSet<V> {
    pub(crate) fn new() -> Self {
        Self { root: None, len: 0 }
    }

    /// A set holding `values`, which are sorted by key with each key once.
    pub(crate) fn from_sorted(values: Vec<(u64, V)>) -> Self {
        debug_assert!(values.windows(2).all(|pair| pair[0].0 < pair[1].0));
        let len = values.len();
        if len == 0 {
            return Self::new();
        }
        let mut level: Vec<Child<V>> = chunks(values)
            .into_iter()
            .map(|leaf| Child::of(Node::Leaf(leaf)))
            .collect();
        while level.len() > 1 {
            level = chunks(level)
                .into_iter()
                .map(|children| Child::of(Node::Branch(children)))
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
        let mut node = self.root.as_deref()?;
        loop {
            match node {
                Node::Leaf(values) => {
                    return values
                        .binary_search_by_key(&key, |(k, _)| *k)
                        .ok()
                        .map(|at| &values[at].1);
                }
                Node::Branch(children) => node = &children[child_for(children, key)].node,
            }
        }
    }

    /// A new version with `value` under `key`; `None` when `key` is taken.
    pub(crate) fn inserted(&self, key: u64, value: V) -> Option<Self> {
        let Some(root) = self.root.as_deref() else {
            return Some(Self {
                root: Some(Arc::new(Node::Leaf(vec![(key, value)]))),
                len: 1,
            });
        };
        let root = match insert(root, key, value)? {
            Grown::One(node) => node,
            Grown::Split(lower, upper) => Node::Branch(vec![Child::of(lower), Child::of(upper)]),
        };
        Some(Self {
            root: Some(Arc::new(root)),
            len: self.len + 1,
        })
    }

    /// A new version without `key`; `None` when it holds no `key`.
    pub(crate) fn removed(&self, key: u64) -> Option<Self> {
        let mut root = Arc::new(remove(self.root.as_deref()?, key)?);
        // A branch left with one child gives way to it.
        while let Node::Branch(children) = &*root
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

    /// The values in key order.
    pub(crate) fn iter(&self) -> Iter<'_, V> {
        let mut iter = Iter {
            branches: std::array::from_fn(|_| [].iter()),
            depth: 0,
            leaf: [].iter(),
            remaining: self.len,
        };
        if let Some(root) = self.root.as_deref() {
            iter.descend(root);
        }
        iter
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

impl<V: Clone> Default for PersistentSet<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, V: Clone> IntoIterator for &'a PersistentSet<V> {
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

fn insert<V: Clone>(node: &Node<V>, key: u64, value: V) -> Option<Grown<V>> {
    match node {
        Node::Leaf(values) => {
            let at = values.binary_search_by_key(&key, |(k, _)| *k).err()?;
            let mut next = Vec::with_capacity(values.len() + 1);
            next.extend_from_slice(&values[..at]);
            next.push((key, value));
            next.extend_from_slice(&values[at..]);
            Some(sized(next, Node::Leaf))
        }
        Node::Branch(children) => {
            let at = child_for(children, key);
            let grown = insert(&children[at].node, key, value)?;
            let mut next = Vec::with_capacity(children.len() + 1);
            next.extend_from_slice(&children[..at]);
            match grown {
                Grown::One(child) => next.push(Child::of(child)),
                Grown::Split(lower, upper) => {
                    next.push(Child::of(lower));
                    next.push(Child::of(upper));
                }
            }
            next.extend_from_slice(&children[at + 1..]);
            Some(sized(next, Node::Branch))
        }
    }
}

/// `node` without `key`, possibly empty or under [`MIN`] (its parent merges
/// it); `None` when it holds no `key`.
fn remove<V: Clone>(node: &Node<V>, key: u64) -> Option<Node<V>> {
    match node {
        Node::Leaf(values) => {
            let at = values.binary_search_by_key(&key, |(k, _)| *k).ok()?;
            let mut next = Vec::with_capacity(values.len() - 1);
            next.extend_from_slice(&values[..at]);
            next.extend_from_slice(&values[at + 1..]);
            #[cfg(test)]
            super::work::add(next.len());
            Some(Node::Leaf(next))
        }
        Node::Branch(children) => {
            let at = child_for(children, key);
            let child = remove(&children[at].node, key)?;
            let mut next: Vec<Child<V>> = Vec::with_capacity(children.len());
            if child.width() == 0 {
                next.extend_from_slice(&children[..at]);
                next.extend_from_slice(&children[at + 1..]);
            } else if child.width() < MIN && children.len() > 1 {
                // Joined with a neighbour, as one node or two even halves.
                let (left, joined) = if at + 1 < children.len() {
                    (at, join(&child, &children[at + 1].node))
                } else {
                    (at - 1, join(&children[at - 1].node, &child))
                };
                next.extend_from_slice(&children[..left]);
                match joined {
                    Grown::One(node) => next.push(Child::of(node)),
                    Grown::Split(lower, upper) => {
                        next.push(Child::of(lower));
                        next.push(Child::of(upper));
                    }
                }
                next.extend_from_slice(&children[left + 2..]);
            } else {
                next.extend_from_slice(&children[..at]);
                next.push(Child::of(child));
                next.extend_from_slice(&children[at + 1..]);
            }
            #[cfg(test)]
            super::work::add(next.len());
            Some(Node::Branch(next))
        }
    }
}

/// Two neighbouring nodes of the same depth, `lower` first, as one.
fn join<V: Clone>(lower: &Node<V>, upper: &Node<V>) -> Grown<V> {
    match (lower, upper) {
        (Node::Leaf(a), Node::Leaf(b)) => sized([a.as_slice(), b].concat(), Node::Leaf),
        (Node::Branch(a), Node::Branch(b)) => sized([a.as_slice(), b].concat(), Node::Branch),
        _ => unreachable!("neighbours are at the same depth"),
    }
}

/// The values of a [`PersistentSet`] in key order.
pub(crate) struct Iter<'a, V> {
    /// The children left to visit at each level above the current leaf.
    branches: [std::slice::Iter<'a, Child<V>>; MAX_DEPTH],
    depth: usize,
    leaf: std::slice::Iter<'a, (u64, V)>,
    remaining: usize,
}

impl<'a, V> Iter<'a, V> {
    /// Goes down `node`'s first children to its first leaf.
    fn descend(&mut self, mut node: &'a Node<V>) {
        loop {
            match node {
                Node::Leaf(values) => {
                    self.leaf = values.iter();
                    return;
                }
                Node::Branch(children) => {
                    let mut rest = children.iter();
                    let Some(first) = rest.next() else {
                        return;
                    };
                    assert!(self.depth < MAX_DEPTH, "a set deeper than MAX_DEPTH");
                    self.branches[self.depth] = rest;
                    self.depth += 1;
                    node = &first.node;
                }
            }
        }
    }
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = &'a V;

    #[inline]
    fn next(&mut self) -> Option<&'a V> {
        loop {
            if let Some((_, value)) = self.leaf.next() {
                self.remaining -= 1;
                return Some(value);
            }
            // The next leaf: the next child of the nearest level that has one.
            loop {
                if self.depth == 0 {
                    return None;
                }
                if let Some(child) = self.branches[self.depth - 1].next() {
                    self.descend(&child.node);
                    break;
                }
                self.depth -= 1;
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<V> ExactSizeIterator for Iter<'_, V> {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// The depth of every leaf, and whether every node but the root holds at
    /// least `MIN` and at most `MAX`, with each branch's `first` right.
    fn check<V: Clone>(set: &PersistentSet<V>) -> usize {
        fn walk<V>(node: &Node<V>, depth: usize, root: bool, depths: &mut Vec<usize>) -> usize {
            let width = node.width();
            assert!(width <= MAX, "{width} > MAX");
            assert!(root || width >= MIN, "{width} < MIN below the root");
            match node {
                Node::Leaf(values) => {
                    assert!(values.windows(2).all(|pair| pair[0].0 < pair[1].0));
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
            .as_deref()
            .map_or(0, |root| walk(root, 1, true, &mut depths));
        assert_eq!(counted, set.len);
        assert!(depths.windows(2).all(|pair| pair[0] == pair[1]), "balanced");
        depths.first().copied().unwrap_or(0)
    }

    fn contents(set: &PersistentSet<u64>) -> Vec<u64> {
        set.iter().copied().collect()
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
                match set.inserted(key, key) {
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
    }

    /// Appending in order, the common case (new observers are the newest),
    /// keeps the tree shallow; so does building from a sorted list.
    #[test]
    fn appends_and_bulk_builds_stay_shallow() {
        let mut set = PersistentSet::new();
        for key in 0..50_000u64 {
            set = set.inserted(key, key).unwrap();
        }
        let depth = check(&set);
        assert!(depth <= 5, "depth {depth}");
        assert_eq!(contents(&set), (0..50_000).collect::<Vec<_>>());

        for len in [0, 1, MIN - 1, MIN, FILL, MAX, MAX + 1, 1_000, 50_000] {
            let built = PersistentSet::from_sorted((0..len as u64).map(|k| (k, k)).collect());
            check(&built);
            assert_eq!(contents(&built), (0..len as u64).collect::<Vec<_>>());
            assert_eq!(built.iter().len(), len);
        }
    }

    /// Removing from the front, as old observers go first, merges what
    /// empties out, leaving no trail of thin leaves.
    #[test]
    fn removing_from_the_front_keeps_the_tree_filled() {
        let mut set = PersistentSet::from_sorted((0..10_000u64).map(|k| (k, k)).collect());
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
