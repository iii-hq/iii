// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! A persistent ordered set: a run of leaves, each an immutable `Arc<[V]>`
//! of values in key order, shared between versions. The routed hook holds
//! one version per call: the active observers of a function, in creation
//! order (see [`super::ActiveSet`]). A version never changes, and a value is
//! dropped with the last leaf that holds it, so with the last version that
//! holds it.
//!
//! The first leaf (the head) and the last (the tail) are held by the set
//! itself, the leaves between them by a B+ tree whose nodes are shared
//! between versions too. The two changes the active observers see most
//! touch only an edge: a value past every key (a new observer) is added to
//! the tail, and the first value (the oldest observer) leaves the head, each
//! by a copy of that one leaf of at most [`MAX`] values. Once the tail is
//! full it joins the tree, and an empty head takes the tree's first leaf,
//! which copies the nodes on the tree's edge, once every [`MAX`] changes.
//! Any other change copies the leaf it lands in and the tree nodes on its
//! path (`O(log n)` nodes of at most [`MAX`] or [`BRANCH_MAX`] slots each).
//!
//! A scan reads each leaf as a slice of values ([`PersistentSet::leaves`]),
//! and a set of at most [`MAX`] values is one leaf.

use std::{iter, sync::Arc};

/// Values in a leaf at most: what one change of a leaf copies.
const MAX: usize = 32;
/// A leaf of the tree with fewer is merged with a neighbour.
const MIN: usize = MAX / 4;
/// What a leaf holds when a set is built from a sorted list.
const FILL: usize = MAX * 3 / 4;
/// Children of a branch at most.
const BRANCH_MAX: usize = 32;
/// A branch other than the tree's root with fewer is merged with a
/// neighbour.
const BRANCH_MIN: usize = BRANCH_MAX / 4;
/// What a branch holds when a set is built from a sorted list.
const BRANCH_FILL: usize = BRANCH_MAX * 3 / 4;
/// Levels a tree has at most: every node but the root and the edges of each
/// level holds at least [`MIN`] or [`BRANCH_MIN`], so 13 levels hold more
/// than 2^40 values.
const MAX_DEPTH: usize = 16;

/// What a [`PersistentSet`] orders its values by. A value's key never
/// changes while a set holds it.
pub(crate) trait Keyed {
    fn key(&self) -> u64;
}

/// Values ordered by their [`Keyed::key`], each key at most once.
///
/// The tail holds a value whenever the set does, and the head whenever the
/// tree does: every key in the head comes before every key in the tree, and
/// every key in the tree before every key in the tail.
pub(crate) struct PersistentSet<V> {
    head: Option<Arc<[V]>>,
    tree: Option<Node<V>>,
    tail: Option<Arc<[V]>>,
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

    /// What it holds at least, but as the tree's root or an edge of its
    /// level.
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
            Node::Leaf(values) => values[0].key(),
            Node::Branch(children) => children[0].first,
        }
    }
}

/// Where a key lands: the last part whose first key is at or before it, or
/// the first part.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Head,
    Tree,
    Tail,
}

/// The child whose range holds `key`: the last one starting at or before it,
/// or the first.
fn child_for<V>(children: &[Child<V>], key: u64) -> usize {
    children
        .partition_point(|child| child.first <= key)
        .saturating_sub(1)
}

/// Where a node changes: in the middle, or as the last node of its level,
/// where new leaves go.
#[derive(Clone, Copy, PartialEq)]
enum Growth {
    Inside,
    Appended,
}

/// `items` (`len` of them) as one leaf: one allocation, counted by the
/// complexity tests.
fn leaf<V>(items: impl Iterator<Item = V>, len: usize) -> Arc<[V]> {
    #[cfg(test)]
    super::work::add(len);
    let leaf: Arc<[V]> = items.collect();
    debug_assert_eq!(leaf.len(), len);
    leaf
}

/// `values` with `value` at `at`.
fn leaf_with<V: Clone>(values: &[V], at: usize, value: V) -> Arc<[V]> {
    leaf(
        values[..at]
            .iter()
            .cloned()
            .chain(iter::once(value))
            .chain(values[at..].iter().cloned()),
        values.len() + 1,
    )
}

/// `values` without the one at `at`; `None` when it was the only one.
fn leaf_without<V: Clone>(values: &[V], at: usize) -> Option<Arc<[V]>> {
    (values.len() > 1).then(|| {
        leaf(
            values[..at].iter().chain(&values[at + 1..]).cloned(),
            values.len() - 1,
        )
    })
}

/// `values` (more than [`MAX`]) as two leaves of even halves.
fn halves<V: Clone>(values: &[V]) -> (Arc<[V]>, Arc<[V]>) {
    let (lower, upper) = values.split_at(values.len() / 2);
    (
        leaf(lower.iter().cloned(), lower.len()),
        leaf(upper.iter().cloned(), upper.len()),
    )
}

/// Which end of the set a leaf is, for [`grown_leaf`].
#[derive(Clone, Copy, PartialEq)]
enum End {
    /// The head: values come in at its front.
    First,
    /// The tail: values come in at its back.
    Last,
    /// A leaf of the tree.
    Neither,
}

/// `values` with `value` at `at`, as one leaf, or as two when that is more
/// than [`MAX`]: at the tail's back, the full leaf as it is and the value
/// alone after it, which the next values fill; at the head's front, the
/// value alone before it; even halves otherwise.
fn grown_leaf<V: Clone>(
    values: &Arc<[V]>,
    at: usize,
    value: V,
    end: End,
) -> (Arc<[V]>, Option<Arc<[V]>>) {
    if values.len() < MAX {
        return (leaf_with(values, at, value), None);
    }
    if end == End::Last && at == values.len() {
        return (values.clone(), Some(leaf(iter::once(value), 1)));
    }
    if end == End::First && at == 0 {
        return (leaf(iter::once(value), 1), Some(values.clone()));
    }
    let all: Vec<V> = values[..at]
        .iter()
        .cloned()
        .chain(iter::once(value))
        .chain(values[at..].iter().cloned())
        .collect();
    let (lower, upper) = halves(&all);
    (lower, Some(upper))
}

/// `children` (`len` of them) as one branch, or two when there are more
/// than [`BRANCH_MAX`]: even halves, or, for the last branch of its level, a
/// lower one of `BRANCH_MAX - BRANCH_MIN` and an upper one of
/// `BRANCH_MIN + 1`, which the next leaves fill. One allocation per branch.
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

/// `children` with the one at `at` made into `grown`: replaced, or replaced
/// by the two halves it split into.
fn regrown<V: Keyed>(
    children: &[Child<V>],
    at: usize,
    grown: Grown<V>,
    growth: Growth,
) -> Grown<V> {
    let (lower, upper) = match grown {
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
    sized_branch(items, len, growth)
}

/// `children` with the one at `at` made into `shrunk`: gone when `None`,
/// joined with a neighbour when it holds fewer than its least, replaced
/// otherwise. `None` when no child is left. `last` is [`Growth::Appended`]
/// when the branch is the last of its level.
fn shrunk<V: Keyed + Clone>(
    children: &[Child<V>],
    at: usize,
    shrunk: Option<Node<V>>,
    last: Growth,
) -> Option<Node<V>> {
    let next: Arc<[Child<V>]> = match shrunk {
        None => {
            if children.len() == 1 {
                return None;
            }
            #[cfg(test)]
            super::work::add(children.len() - 1);
            children[..at]
                .iter()
                .chain(&children[at + 1..])
                .cloned()
                .collect()
        }
        Some(child) if child.width() < child.least() && children.len() > 1 => {
            // Joined with a neighbour, as one node or two even halves.
            let (left, joined) = if at + 1 < children.len() {
                let growth = if at + 2 == children.len() {
                    last
                } else {
                    Growth::Inside
                };
                (at, join(&child, &children[at + 1].node, growth))
            } else {
                (at - 1, join(&children[at - 1].node, &child, last))
            };
            let (lower, upper) = match joined {
                Grown::One(node) => (Child::of(node), None),
                Grown::Split(lower, upper) => (Child::of(lower), Some(Child::of(upper))),
            };
            let len = children.len() - 1 + usize::from(upper.is_some());
            #[cfg(test)]
            super::work::add(len);
            let next: Arc<[Child<V>]> = children[..left]
                .iter()
                .cloned()
                .chain(iter::once(lower))
                .chain(upper)
                .chain(children[left + 2..].iter().cloned())
                .collect();
            debug_assert_eq!(next.len(), len);
            next
        }
        Some(child) => {
            #[cfg(test)]
            super::work::add(children.len());
            children[..at]
                .iter()
                .cloned()
                .chain(iter::once(Child::of(child)))
                .chain(children[at + 1..].iter().cloned())
                .collect()
        }
    };
    Some(Node::Branch(next))
}

/// `node` with `value`; `None` when its key is taken. `last` is
/// [`Growth::Appended`] when `node` is the last node of its level.
fn insert<V: Keyed + Clone>(node: &Node<V>, value: V, last: Growth) -> Option<Grown<V>> {
    let key = value.key();
    match node {
        Node::Leaf(values) => {
            let at = values.binary_search_by_key(&key, Keyed::key).err()?;
            Some(match grown_leaf(values, at, value, End::Neither) {
                (one, None) => Grown::One(Node::Leaf(one)),
                (lower, Some(upper)) => Grown::Split(Node::Leaf(lower), Node::Leaf(upper)),
            })
        }
        Node::Branch(children) => {
            let at = child_for(children, key);
            let growth = if at + 1 == children.len() {
                last
            } else {
                Growth::Inside
            };
            let grown = insert(&children[at].node, value, growth)?;
            Some(regrown(children, at, grown, growth))
        }
    }
}

/// `node` without `key`: `Some(None)` when nothing is left of it, and
/// possibly under its least otherwise (its parent merges it); `None` when it
/// holds no `key`. `last` is [`Growth::Appended`] when `node` is the last
/// node of its level.
fn remove<V: Keyed + Clone>(node: &Node<V>, key: u64, last: Growth) -> Option<Option<Node<V>>> {
    match node {
        Node::Leaf(values) => {
            let at = values.binary_search_by_key(&key, Keyed::key).ok()?;
            Some(leaf_without(values, at).map(Node::Leaf))
        }
        Node::Branch(children) => {
            let at = child_for(children, key);
            let growth = if at + 1 == children.len() {
                last
            } else {
                Growth::Inside
            };
            let child = remove(&children[at].node, key, growth)?;
            Some(shrunk(children, at, child, last))
        }
    }
}

/// `node` with `leaf`, whose keys are past all of its own, as its last
/// leaf.
fn push_last<V: Keyed + Clone>(node: &Node<V>, leaf: Arc<[V]>) -> Grown<V> {
    match node {
        Node::Leaf(_) => Grown::Split(node.clone(), Node::Leaf(leaf)),
        Node::Branch(children) => {
            let at = children.len() - 1;
            let grown = push_last(&children[at].node, leaf);
            regrown(children, at, grown, Growth::Appended)
        }
    }
}

/// `node` with `leaf`, whose keys come before all of its own, as its first
/// leaf.
fn push_first<V: Keyed + Clone>(node: &Node<V>, leaf: Arc<[V]>) -> Grown<V> {
    match node {
        Node::Leaf(_) => Grown::Split(Node::Leaf(leaf), node.clone()),
        Node::Branch(children) => {
            let grown = push_first(&children[0].node, leaf);
            regrown(children, 0, grown, Growth::Inside)
        }
    }
}

/// `node`'s first leaf, and `node` without it (`None` when that was all of
/// it). `last` is [`Growth::Appended`] when `node` is the last node of its
/// level.
fn pop_first<V: Keyed + Clone>(node: &Node<V>, last: Growth) -> (Arc<[V]>, Option<Node<V>>) {
    match node {
        Node::Leaf(values) => (values.clone(), None),
        Node::Branch(children) => {
            let growth = if children.len() == 1 {
                last
            } else {
                Growth::Inside
            };
            let (leaf, rest) = pop_first(&children[0].node, growth);
            (leaf, shrunk(children, 0, rest, last))
        }
    }
}

/// `node`'s last leaf, and `node` without it (`None` when that was all of
/// it). `node` is the last node of its level.
fn pop_last<V: Keyed + Clone>(node: &Node<V>) -> (Arc<[V]>, Option<Node<V>>) {
    match node {
        Node::Leaf(values) => (values.clone(), None),
        Node::Branch(children) => {
            let at = children.len() - 1;
            let (leaf, rest) = pop_last(&children[at].node);
            (leaf, shrunk(children, at, rest, Growth::Appended))
        }
    }
}

/// A tree's root: a branch left with one child gives way to it.
fn rooted<V>(mut root: Node<V>) -> Node<V> {
    while let Node::Branch(children) = &root
        && children.len() == 1
    {
        let only = children[0].node.clone();
        root = only;
    }
    root
}

/// What a change of the tree made of its root: the root, or a branch over
/// the two halves it split into.
fn grown_root<V: Keyed>(grown: Grown<V>) -> Node<V> {
    match grown {
        Grown::One(node) => node,
        Grown::Split(lower, upper) => Node::Branch(Arc::new([Child::of(lower), Child::of(upper)])),
    }
}

/// Two neighbouring nodes of the same depth, `lower` first, as one, or as
/// two even halves. `growth` is [`Growth::Appended`] when `upper` is the last
/// node of its level.
fn join<V: Keyed + Clone>(lower: &Node<V>, upper: &Node<V>, growth: Growth) -> Grown<V> {
    match (lower, upper) {
        (Node::Leaf(a), Node::Leaf(b)) => {
            let len = a.len() + b.len();
            if len <= MAX {
                return Grown::One(Node::Leaf(leaf(a.iter().chain(b.iter()).cloned(), len)));
            }
            let all: Vec<V> = a.iter().chain(b.iter()).cloned().collect();
            let (lower, upper) = halves(&all);
            Grown::Split(Node::Leaf(lower), Node::Leaf(upper))
        }
        (Node::Branch(a), Node::Branch(b)) => {
            sized_branch(a.iter().chain(b.iter()).cloned(), a.len() + b.len(), growth)
        }
        _ => unreachable!("neighbours are at the same depth"),
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

/// A tree over `leaves` (at least one), in key order.
fn tree_of<V: Keyed>(leaves: Vec<Arc<[V]>>) -> Node<V> {
    let mut level: Vec<Child<V>> = leaves
        .into_iter()
        .map(|values| Child::of(Node::Leaf(values)))
        .collect();
    while level.len() > 1 {
        level = chunks(level, BRANCH_FILL)
            .into_iter()
            .map(|children| Child::of(Node::Branch(children.into())))
            .collect();
    }
    level.pop().expect("a tree of at least one leaf").node
}

impl<V: Keyed + Clone> PersistentSet<V> {
    pub(crate) fn new() -> Self {
        Self {
            head: None,
            tree: None,
            tail: None,
            len: 0,
        }
    }

    /// A set holding `values`, which are sorted by key with each key once.
    pub(crate) fn from_sorted(values: Vec<V>) -> Self {
        debug_assert!(values.windows(2).all(|pair| pair[0].key() < pair[1].key()));
        let len = values.len();
        if len == 0 {
            return Self::new();
        }
        if len <= MAX {
            return Self {
                head: None,
                tree: None,
                tail: Some(leaf(values.into_iter(), len)),
                len,
            };
        }
        let mut runs: Vec<Arc<[V]>> = chunks(values, FILL)
            .into_iter()
            .map(|run| {
                let len = run.len();
                leaf(run.into_iter(), len)
            })
            .collect();
        let tail = runs.pop();
        let head = Some(runs.remove(0));
        Self {
            head,
            tree: (!runs.is_empty()).then(|| tree_of(runs)),
            tail,
            len,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The part that holds `key`, or would: the last part whose first key is
    /// at or before it, or the first part. `None` when the set is empty.
    fn part_for(&self, key: u64) -> Option<Part> {
        let tail = self.tail.as_ref()?;
        if key >= tail[0].key() {
            return Some(Part::Tail);
        }
        if let Some(tree) = &self.tree
            && key >= tree.first()
        {
            return Some(Part::Tree);
        }
        Some(if self.head.is_some() {
            Part::Head
        } else {
            Part::Tail
        })
    }

    /// The value under `key`.
    pub(crate) fn get(&self, key: u64) -> Option<&V> {
        let values: &[V] = match self.part_for(key)? {
            Part::Head => self.head.as_deref()?,
            Part::Tail => self.tail.as_deref()?,
            Part::Tree => {
                let mut node = self.tree.as_ref()?;
                loop {
                    match node {
                        Node::Leaf(values) => break values,
                        Node::Branch(children) => node = &children[child_for(children, key)].node,
                    }
                }
            }
        };
        values
            .binary_search_by_key(&key, Keyed::key)
            .ok()
            .map(|at| &values[at])
    }

    /// A new version with `value`; `None` when its key is taken.
    pub(crate) fn inserted(&self, value: V) -> Option<Self> {
        let key = value.key();
        let Some(part) = self.part_for(key) else {
            return Some(Self {
                head: None,
                tree: None,
                tail: Some(leaf(iter::once(value), 1)),
                len: 1,
            });
        };
        let (head, tree, tail) = match part {
            Part::Tree => {
                let tree = insert(self.tree.as_ref()?, value, Growth::Appended)?;
                (self.head.clone(), Some(grown_root(tree)), self.tail.clone())
            }
            Part::Tail => {
                let tail = self.tail.as_ref()?;
                let at = tail.binary_search_by_key(&key, Keyed::key).err()?;
                match grown_leaf(tail, at, value, End::Last) {
                    (tail, None) => (self.head.clone(), self.tree.clone(), Some(tail)),
                    // A full tail leaves its lower part to the tree, or makes
                    // it the head when there is none.
                    (lower, Some(tail)) => match (&self.head, &self.tree) {
                        (None, _) => (Some(lower), None, Some(tail)),
                        (Some(head), None) => {
                            (Some(head.clone()), Some(Node::Leaf(lower)), Some(tail))
                        }
                        (Some(head), Some(tree)) => (
                            Some(head.clone()),
                            Some(grown_root(push_last(tree, lower))),
                            Some(tail),
                        ),
                    },
                }
            }
            Part::Head => {
                let head = self.head.as_ref()?;
                let at = head.binary_search_by_key(&key, Keyed::key).err()?;
                match grown_leaf(head, at, value, End::First) {
                    (head, None) => (Some(head), self.tree.clone(), self.tail.clone()),
                    // A full head leaves its upper part to the tree.
                    (head, Some(upper)) => {
                        let tree = match &self.tree {
                            None => Node::Leaf(upper),
                            Some(tree) => grown_root(push_first(tree, upper)),
                        };
                        (Some(head), Some(tree), self.tail.clone())
                    }
                }
            }
        };
        Some(Self {
            head,
            tree,
            tail,
            len: self.len + 1,
        })
    }

    /// A new version without `key`; `None` when it holds no `key`.
    pub(crate) fn removed(&self, key: u64) -> Option<Self> {
        let (head, tree, tail) = match self.part_for(key)? {
            Part::Tree => {
                let tree = remove(self.tree.as_ref()?, key, Growth::Appended)?;
                (self.head.clone(), tree.map(rooted), self.tail.clone())
            }
            Part::Tail => {
                let tail = self.tail.as_ref()?;
                let at = tail.binary_search_by_key(&key, Keyed::key).ok()?;
                match (leaf_without(tail, at), &self.tree) {
                    (Some(tail), _) => (self.head.clone(), self.tree.clone(), Some(tail)),
                    // The tree's last leaf is the tail now, or the head when
                    // there is no tree.
                    (None, Some(tree)) => {
                        let (last, rest) = pop_last(tree);
                        (self.head.clone(), rest.map(rooted), Some(last))
                    }
                    (None, None) => (None, None, self.head.clone()),
                }
            }
            Part::Head => {
                let head = self.head.as_ref()?;
                let at = head.binary_search_by_key(&key, Keyed::key).ok()?;
                match (leaf_without(head, at), &self.tree) {
                    (Some(head), _) => (Some(head), self.tree.clone(), self.tail.clone()),
                    // The tree's first leaf is the head now.
                    (None, Some(tree)) => {
                        let (first, rest) = pop_first(tree, Growth::Appended);
                        (Some(first), rest.map(rooted), self.tail.clone())
                    }
                    (None, None) => (None, None, self.tail.clone()),
                }
            }
        };
        Some(Self {
            head,
            tree,
            tail,
            len: self.len - 1,
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
    /// over each, for the scan the routed hook runs on every call. A set of
    /// at most [`MAX`] values is one leaf, read with no walk of branches.
    #[inline]
    pub(crate) fn leaves(&self) -> Leaves<'_, V> {
        let tail = self.tail.as_deref();
        let Some(head) = self.head.as_deref() else {
            return Leaves {
                next: tail,
                queued: None,
                deeper: None,
                tail: None,
            };
        };
        match &self.tree {
            None => Leaves {
                next: Some(head),
                queued: None,
                deeper: None,
                tail,
            },
            Some(Node::Leaf(values)) => Leaves {
                next: Some(head),
                queued: Some(values),
                deeper: None,
                tail,
            },
            Some(Node::Branch(children)) => {
                let mut deeper = Deeper {
                    branches: std::array::from_fn(|_| [].iter()),
                    depth: 0,
                };
                let queued = deeper.descend(children);
                Leaves {
                    next: Some(head),
                    queued: Some(queued),
                    deeper: Some(deeper),
                    tail,
                }
            }
        }
    }
}

impl<V> Clone for PersistentSet<V> {
    fn clone(&self) -> Self {
        Self {
            head: self.head.clone(),
            tree: self.tree.clone(),
            tail: self.tail.clone(),
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

/// The leaves of a [`PersistentSet`] in key order (see
/// [`PersistentSet::leaves`]).
pub(crate) struct Leaves<'a, V> {
    /// The leaf to yield next.
    next: Option<&'a [V]>,
    /// The one after it: the tree's first leaf, after the head.
    queued: Option<&'a [V]>,
    /// Where the tree's other leaves are, in a tree deeper than one leaf.
    deeper: Option<Deeper<'a, V>>,
    /// The last leaf, after the tree's.
    tail: Option<&'a [V]>,
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
        self.next = match self.queued.take() {
            Some(queued) => Some(queued),
            None => self
                .deeper
                .as_mut()
                .and_then(Deeper::next_leaf)
                .or_else(|| self.tail.take()),
        };
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
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    impl Keyed for u64 {
        fn key(&self) -> u64 {
            *self
        }
    }

    /// The depth of the tree's leaves, checking the set's shape: the tail
    /// holds a value whenever the set does and the head whenever the tree
    /// does, the edges hold at most `MAX`, every key comes in order across
    /// the parts, and in the tree every node but the root and the edges of
    /// its level holds at least its least and every node at most its most,
    /// with each branch's `first` right.
    fn check<V: Keyed>(set: &PersistentSet<V>) -> usize {
        fn walk<V: Keyed>(
            node: &Node<V>,
            depth: usize,
            root: bool,
            edge: bool,
            depths: &mut Vec<usize>,
        ) -> usize {
            let width = node.width();
            let most = match node {
                Node::Leaf(_) => MAX,
                Node::Branch(_) => BRANCH_MAX,
            };
            assert!(width <= most, "{width} > {most}");
            assert!(width > 0, "an empty node");
            assert!(
                root || edge || width >= node.least(),
                "{width} < {} inside the tree",
                node.least()
            );
            match node {
                Node::Leaf(values) => {
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
                            let edge = edge && (at == 0 || at + 1 == children.len());
                            walk(&child.node, depth + 1, false, edge, depths)
                        })
                        .sum()
                }
            }
        }
        assert_eq!(
            set.tail.is_some(),
            set.len > 0,
            "a tail while there are values"
        );
        assert!(
            set.tree.is_none() || set.head.is_some(),
            "a head with a tree"
        );
        for edge in [&set.head, &set.tail].into_iter().flatten() {
            assert!(
                !edge.is_empty() && edge.len() <= MAX,
                "an edge of {}",
                edge.len()
            );
        }
        let mut depths = Vec::new();
        let in_tree = set
            .tree
            .as_ref()
            .map_or(0, |root| walk(root, 1, true, true, &mut depths));
        let in_edges = set.head.as_ref().map_or(0, |head| head.len())
            + set.tail.as_ref().map_or(0, |tail| tail.len());
        assert_eq!(in_tree + in_edges, set.len);
        assert!(depths.windows(2).all(|pair| pair[0] == pair[1]), "balanced");
        let keys: Vec<u64> = set.iter().map(Keyed::key).collect();
        assert!(
            keys.windows(2).all(|pair| pair[0] < pair[1]),
            "in key order"
        );
        assert_eq!(keys.len(), set.len);
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
    /// step, the set keeps its shape, and every earlier version still reads
    /// as it was.
    #[test]
    fn random_inserts_and_removes_match_a_btreemap() {
        for (seed, keys, steps) in [
            (0x9e37_79b9_7f4a_7c15u64, 3_000u64, 20_000),
            (0x2545_f491_4f6c_dd1d, 100, 5_000),
            (0x5851_f42d_4c95_7f2d, 40, 5_000),
        ] {
            let mut rng = Rng(seed);
            let mut set = PersistentSet::new();
            let mut model = BTreeMap::new();
            let mut versions: Vec<(PersistentSet<u64>, Vec<u64>)> = Vec::new();
            for step in 0..steps {
                let key = rng.next() % keys;
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
                if keys < 1_000 || step % 97 == 0 {
                    check(&set);
                }
                if step % 997 == 0 {
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
                assert!(set.get(key).is_none());
            }
            assert!(set.is_empty());
            assert_eq!(set.iter().count(), 0);
            assert_eq!(set.leaves().count(), 0);
        }
    }

    /// Observers churning as they do (the oldest leaves, a newest joins,
    /// now and then one in the middle) agree with a `BTreeMap`, from one
    /// observer up to thousands and back.
    #[test]
    fn churn_at_the_edges_matches_a_btreemap() {
        let mut rng = Rng(0x0123_4567_89ab_cdef);
        let mut set = PersistentSet::new();
        let mut model = BTreeMap::new();
        let mut next = 0u64;
        for step in 0..60_000u64 {
            // Grow to about 3,000, shrink to nothing, grow again.
            let target = if (step / 15_000).is_multiple_of(2) {
                3_000
            } else {
                0
            };
            let roll = rng.next() % 16;
            if roll == 0 && !model.is_empty() {
                // One in the middle leaves.
                let nth = (rng.next() % model.len() as u64) as usize;
                let key = *model.keys().nth(nth).unwrap();
                set = set.removed(key).unwrap();
                model.remove(&key);
            } else if roll == 1 && !model.is_empty() {
                // One in the middle comes back (a key between two held).
                let key = rng.next() % next.max(1);
                if let Some(grown) = set.inserted(key) {
                    assert!(model.insert(key, key).is_none());
                    set = grown;
                }
            } else if model.len() < target || (model.len() == target && roll < 9) {
                set = set.inserted(next).unwrap();
                model.insert(next, next);
                next += 1;
            } else if let Some((&first, _)) = model.iter().next() {
                set = set.removed(first).unwrap();
                model.remove(&first);
            }
            if step % 101 == 0 || model.len() < 2 * MAX {
                check(&set);
                assert_eq!(contents(&set), model.keys().copied().collect::<Vec<_>>());
            }
        }
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
        assert!(depth <= 4, "depth {depth}");
        assert_eq!(contents(&set), (0..50_000).collect::<Vec<_>>());

        for len in [
            0,
            1,
            MIN - 1,
            MIN,
            FILL,
            MAX,
            MAX + 1,
            2 * MAX,
            2 * MAX + 1,
            1_000,
            50_000,
        ] {
            let built = PersistentSet::from_sorted((0..len as u64).collect());
            check(&built);
            assert_eq!(contents(&built), (0..len as u64).collect::<Vec<_>>());
        }
    }

    /// A value shared as the active sets share an observer: a clone is the
    /// same value, and `alive` counts the values none of whose clones has
    /// gone yet, so a test sees each one dropped with the last version that
    /// holds it.
    #[derive(Clone)]
    struct Counted {
        key: u64,
        _life: Arc<Life>,
    }

    struct Life(Arc<AtomicUsize>);

    impl Counted {
        fn new(key: u64, alive: &Arc<AtomicUsize>) -> Self {
            alive.fetch_add(1, Ordering::Relaxed);
            Self {
                key,
                _life: Arc::new(Life(alive.clone())),
            }
        }
    }

    impl Keyed for Counted {
        fn key(&self) -> u64 {
            self.key
        }
    }

    impl Drop for Life {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Every value made is dropped once all the versions that held it are,
    /// and once only the last version is left, only its values live.
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
                } else if let Some(first) = set.iter().next().map(|v: &Counted| v.key) {
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
            drop(versions);
            assert_eq!(
                alive.load(Ordering::Relaxed),
                set.len(),
                "a value goes with the last version that holds it"
            );
        }
        assert_eq!(alive.load(Ordering::Relaxed), 0, "each value dropped once");
    }

    /// A value taken out (an observer leaving) goes with the last version
    /// that holds it, wherever it was: first, last, or in the tree.
    #[test]
    fn a_removed_value_goes_with_the_last_version_that_holds_it() {
        let alive = Arc::new(AtomicUsize::new(0));
        let live = || alive.load(Ordering::Relaxed);
        let counted = |keys: std::ops::Range<u64>| -> Vec<Counted> {
            keys.map(|key| Counted::new(key, &alive)).collect()
        };
        {
            let mut set = PersistentSet::from_sorted(counted(0..10));
            let held = set.clone();
            set = set.removed(0).unwrap();
            assert_eq!(live(), 10, "an earlier version still holds 0");
            drop(held);
            assert_eq!(live(), 9, "0 goes with the last version that holds it");
            assert_eq!(set.iter().next().map(|value| value.key), Some(1));

            // The oldest leaving, one after another, through the head and
            // the tree's leaves it takes: each goes with its removal.
            let mut set = PersistentSet::from_sorted(counted(100..400));
            for key in 100..390 {
                set = set.removed(key).unwrap();
                assert_eq!(live(), 9 + set.len(), "after removing {key}");
                check(&set);
            }

            // A version held across removals keeps what it holds until it
            // goes.
            let before = live();
            let mut set = PersistentSet::from_sorted(counted(1_000..1_300));
            let held = set.clone();
            for key in (1_000..1_010).chain([1_150, 1_299]) {
                set = set.removed(key).unwrap();
            }
            assert_eq!(live(), before + 300, "the held version holds them");
            drop(held);
            assert_eq!(live(), before + 288);
        }
        assert_eq!(live(), 0);
    }

    /// Versions read and dropped on other threads, in any order, while one
    /// thread changes the set: each reads as it was, every value is dropped
    /// once, and once only the last version is left, only its values live.
    #[test]
    fn versions_dropped_on_other_threads_drop_each_value_once() {
        let alive = Arc::new(AtomicUsize::new(0));
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut set =
            PersistentSet::from_sorted((0..300u64).map(|key| Counted::new(key, &alive)).collect());
        let (mut first, mut next) = (0u64, 300u64);
        std::thread::scope(|scope| {
            let senders: Vec<_> = (0..4)
                .map(|seed| {
                    let (send, receive) =
                        std::sync::mpsc::channel::<(PersistentSet<Counted>, u64, u64)>();
                    scope.spawn(move || {
                        let mut rng = Rng(0x9e37_79b9 + seed);
                        let mut held = Vec::new();
                        for version in receive {
                            held.push(version);
                            if held.len() > 8 {
                                let (version, from, to) =
                                    held.swap_remove((rng.next() % held.len() as u64) as usize);
                                let keys: Vec<u64> = version.iter().map(|v| v.key).collect();
                                assert_eq!(keys, (from..to).collect::<Vec<_>>());
                            }
                        }
                    });
                    send
                })
                .collect();
            // The oldest goes or a newest joins, as observers churn, with
            // between 250 and 350 of them.
            for step in 0..20_003u64 {
                let len = set.len();
                let remove =
                    step >= 20_000 || len > 350 || (len >= 250 && rng.next().is_multiple_of(2));
                if remove {
                    set = set.removed(first).unwrap();
                    first += 1;
                } else {
                    set = set.inserted(Counted::new(next, &alive)).unwrap();
                    next += 1;
                }
                let to = (step % senders.len() as u64) as usize;
                senders[to].send((set.clone(), first, next)).unwrap();
            }
        });
        check(&set);
        assert_eq!(alive.load(Ordering::Relaxed), set.len());
        drop(set);
        assert_eq!(alive.load(Ordering::Relaxed), 0, "each value dropped once");
    }

    /// The commonest changes copy one edge leaf: a value past every key goes
    /// into a copy of the tail, and the first value leaves a copy of the
    /// head; the tree's edge is copied once every `MAX` of them, as the tail
    /// joins it or the head takes its first leaf. An earlier version reads
    /// as it was through both.
    #[test]
    fn appends_and_front_removals_copy_an_edge() {
        let mut set = PersistentSet::from_sorted((0..1_000u64).collect());
        let before = set.clone();
        super::super::work::take();
        for key in 1_000..1_320u64 {
            set = set.inserted(key).unwrap();
        }
        let appended = super::super::work::take();
        assert!(
            appended < 320 * (MAX / 2 + 8),
            "{appended} slots copied for 320 appends"
        );
        for key in 0..320u64 {
            set = set.removed(key).unwrap();
        }
        let removed = super::super::work::take();
        assert!(
            removed < 320 * (MAX / 2 + 8),
            "{removed} slots copied for 320 front removals"
        );
        check(&set);
        assert_eq!(contents(&set), (320..1_320).collect::<Vec<_>>());
        assert_eq!(contents(&before), (0..1_000).collect::<Vec<_>>());

        // Two versions appending past the same tail each get their own.
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
        check(&set);
        assert!(set.tree.is_none(), "ten values fit in the edges");
        assert_eq!(contents(&set), (9_990..10_000).collect::<Vec<_>>());
    }
}
