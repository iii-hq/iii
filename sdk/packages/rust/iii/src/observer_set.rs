//! The active observers of a function this worker hosts, as a persistent
//! set: a B+ tree, ordered by observer id, whose nodes are shared between
//! versions. Adding or removing one observer copies the `O(log n)` nodes on
//! its path (at most [`MAX`] slots each) and shares every other node with the
//! version it was made from, which a call that loaded it keeps reading.
//!
//! Each node is one allocation, its slots right after its reference count,
//! so a scan reaches a leaf's observers with one pointer from its branch. A
//! function with up to [`MAX`] observers is one leaf.

use std::{iter, sync::Arc};

use crate::observable::Observer;

/// Observers in a leaf, or children of a branch, at most.
const MAX: usize = 128;
/// A node other than the root with fewer is merged with a neighbour.
const MIN: usize = MAX / 4;
/// Levels a set has at most: every node but the root holds at least [`MIN`],
/// so 9 levels hold more than 2^40 observers.
const MAX_DEPTH: usize = 16;

/// Observers by id, each id at most once.
#[derive(Clone, Default)]
pub(crate) struct ObserverSet {
    root: Option<Node>,
    len: usize,
}

#[derive(Clone)]
enum Node {
    /// Sorted by id.
    Leaf(Arc<[Observer]>),
    /// Sorted by id, all at the same depth.
    Branch(Arc<[Child]>),
}

#[derive(Clone)]
struct Child {
    /// The observer with the smallest id under `node`.
    first: Observer,
    node: Node,
}

impl Child {
    fn of(node: Node) -> Self {
        Self {
            first: node.first().clone(),
            node,
        }
    }
}

/// What an insert made of a node: the node, or the two halves it split into.
enum Grown {
    One(Node),
    Split(Node, Node),
}

impl Node {
    fn width(&self) -> usize {
        match self {
            Node::Leaf(observers) => observers.len(),
            Node::Branch(children) => children.len(),
        }
    }

    /// The observer with the smallest id. Only asked of a node that is not
    /// empty.
    fn first(&self) -> &Observer {
        match self {
            Node::Leaf(observers) => &observers[0],
            Node::Branch(children) => &children[0].first,
        }
    }
}

/// The child whose range holds `id`: the last one starting at or before it,
/// or the first.
fn child_for(children: &[Child], id: &str) -> usize {
    children
        .partition_point(|child| child.first.id() <= id)
        .saturating_sub(1)
}

/// `items` (`len` of them) as one node, or two even halves when there are
/// more than [`MAX`]. One allocation per node made.
fn sized<T: Clone>(
    items: impl Iterator<Item = T> + Clone,
    len: usize,
    node: fn(Arc<[T]>) -> Node,
) -> Grown {
    #[cfg(test)]
    work::add(len);
    if len <= MAX {
        return Grown::One(node(items.collect()));
    }
    let lower = len / 2;
    Grown::Split(
        node(items.clone().take(lower).collect()),
        node(items.skip(lower).collect()),
    )
}

impl ObserverSet {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The observer with id `id`.
    pub(crate) fn get(&self, id: &str) -> Option<&Observer> {
        let mut node = self.root.as_ref()?;
        loop {
            match node {
                Node::Leaf(observers) => {
                    return observers
                        .binary_search_by(|held| held.id().cmp(id))
                        .ok()
                        .map(|at| &observers[at]);
                }
                Node::Branch(children) => node = &children[child_for(children, id)].node,
            }
        }
    }

    /// A new version holding `observer`, in place of the one with its id if
    /// there is one.
    pub(crate) fn with(&self, observer: &Observer) -> Self {
        let Some(root) = self.root.as_ref() else {
            return Self {
                root: Some(Node::Leaf(Arc::new([observer.clone()]))),
                len: 1,
            };
        };
        let (grown, added) = insert(root, observer);
        let root = match grown {
            Grown::One(node) => node,
            Grown::Split(lower, upper) => {
                Node::Branch(Arc::new([Child::of(lower), Child::of(upper)]))
            }
        };
        Self {
            root: Some(root),
            len: self.len + usize::from(added),
        }
    }

    /// A new version without the observer with id `id`; `None` when it holds
    /// none.
    pub(crate) fn without(&self, id: &str) -> Option<Self> {
        let mut root = remove(self.root.as_ref()?, id)?;
        // A branch left with one child gives way to it.
        loop {
            let only = match &root {
                Node::Branch(children) if children.len() == 1 => children[0].node.clone(),
                _ => break,
            };
            root = only;
        }
        let len = self.len - 1;
        Some(Self {
            root: (len > 0).then_some(root),
            len,
        })
    }

    /// The observers in id order.
    pub(crate) fn iter(&self) -> Iter<'_> {
        let mut iter = Iter {
            branches: std::array::from_fn(|_| [].iter()),
            depth: 0,
            leaf: [].iter(),
        };
        if let Some(root) = self.root.as_ref() {
            iter.descend(root);
        }
        iter
    }
}

impl<'a> IntoIterator for &'a ObserverSet {
    type Item = &'a Observer;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

/// `node` with `observer`, and whether its id is new to it.
fn insert(node: &Node, observer: &Observer) -> (Grown, bool) {
    match node {
        Node::Leaf(observers) => {
            match observers.binary_search_by(|held| held.id().cmp(observer.id())) {
                Ok(at) => {
                    #[cfg(test)]
                    work::add(observers.len());
                    let replaced = observers[..at]
                        .iter()
                        .chain(iter::once(observer))
                        .chain(&observers[at + 1..])
                        .cloned()
                        .collect();
                    (Grown::One(Node::Leaf(replaced)), false)
                }
                Err(at) => {
                    let items = observers[..at]
                        .iter()
                        .chain(iter::once(observer))
                        .chain(&observers[at..])
                        .cloned();
                    (sized(items, observers.len() + 1, Node::Leaf), true)
                }
            }
        }
        Node::Branch(children) => {
            let at = child_for(children, observer.id());
            let (grown, added) = insert(&children[at].node, observer);
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
            (sized(items, len, Node::Branch), added)
        }
    }
}

/// `node` without the observer with id `id`, possibly empty or under [`MIN`]
/// (its parent merges it); `None` when it holds none.
fn remove(node: &Node, id: &str) -> Option<Node> {
    match node {
        Node::Leaf(observers) => {
            let at = observers.binary_search_by(|held| held.id().cmp(id)).ok()?;
            #[cfg(test)]
            work::add(observers.len() - 1);
            Some(Node::Leaf(
                observers[..at]
                    .iter()
                    .chain(&observers[at + 1..])
                    .cloned()
                    .collect(),
            ))
        }
        Node::Branch(children) => {
            let at = child_for(children, id);
            let child = remove(&children[at].node, id)?;
            let next: Vec<Child> = if child.width() == 0 {
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
            work::add(next.len());
            Some(Node::Branch(next.into()))
        }
    }
}

/// Two neighbouring nodes of the same depth, `lower` first, as one.
fn join(lower: &Node, upper: &Node) -> Grown {
    match (lower, upper) {
        (Node::Leaf(a), Node::Leaf(b)) => sized(
            a.iter().chain(b.iter()).cloned(),
            a.len() + b.len(),
            Node::Leaf,
        ),
        (Node::Branch(a), Node::Branch(b)) => sized(
            a.iter().chain(b.iter()).cloned(),
            a.len() + b.len(),
            Node::Branch,
        ),
        _ => unreachable!("neighbours are at the same depth"),
    }
}

/// What the complexity tests count: observer slots copied into new nodes on
/// this thread.
#[cfg(test)]
pub(crate) mod work {
    use std::cell::Cell;

    thread_local! {
        static SLOTS: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn add(slots: usize) {
        SLOTS.with(|counted| counted.set(counted.get() + slots));
    }

    /// What was counted since the last call.
    pub(crate) fn take() -> usize {
        SLOTS.with(|counted| counted.replace(0))
    }
}

/// The observers of an [`ObserverSet`] in id order.
pub(crate) struct Iter<'a> {
    /// The children left to visit at each level above the current leaf.
    branches: [std::slice::Iter<'a, Child>; MAX_DEPTH],
    depth: usize,
    leaf: std::slice::Iter<'a, Observer>,
}

impl<'a> Iter<'a> {
    /// Goes down `node`'s first children to its first leaf.
    fn descend(&mut self, mut node: &'a Node) {
        loop {
            match node {
                Node::Leaf(observers) => {
                    self.leaf = observers.iter();
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

    /// Moves on to the next leaf: the next child of the nearest level that
    /// has one. `false` when there is none.
    fn next_leaf(&mut self) -> bool {
        loop {
            if self.depth == 0 {
                return false;
            }
            match self.branches[self.depth - 1].next() {
                Some(child) => {
                    self.descend(&child.node);
                    return true;
                }
                None => self.depth -= 1,
            }
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a Observer;

    #[inline]
    fn next(&mut self) -> Option<&'a Observer> {
        loop {
            if let Some(observer) = self.leaf.next() {
                return Some(observer);
            }
            if !self.next_leaf() {
                return None;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Checks the tree's shape: balanced, every node but the root holding
    /// [`MIN`] to [`MAX`], ids sorted and each branch's `first` right.
    /// Returns its depth.
    pub(crate) fn check(set: &ObserverSet) -> usize {
        fn walk(node: &Node, depth: usize, root: bool, depths: &mut Vec<usize>) -> usize {
            let width = node.width();
            assert!(width <= MAX, "{width} > MAX");
            assert!(root || width >= MIN, "{width} < MIN below the root");
            match node {
                Node::Leaf(observers) => {
                    assert!(observers.windows(2).all(|pair| pair[0].id() < pair[1].id()));
                    depths.push(depth);
                    observers.len()
                }
                Node::Branch(children) => {
                    assert!(!root || children.len() >= 2, "a root branch of one");
                    children
                        .iter()
                        .map(|child| {
                            assert_eq!(child.first.id(), child.node.first().id());
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
        assert_eq!(counted, set.len());
        assert!(depths.windows(2).all(|pair| pair[0] == pair[1]), "balanced");
        depths.first().copied().unwrap_or(0)
    }

    fn ids(set: &ObserverSet) -> Vec<String> {
        set.iter().map(|o| o.id().to_string()).collect()
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// Random adds and removes agree with a `BTreeSet` of ids at every
    /// step, the tree stays balanced and filled, and every earlier version
    /// still reads as it was.
    #[test]
    fn random_adds_and_removes_match_a_btreeset() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut set = ObserverSet::default();
        let mut model = BTreeSet::new();
        let mut versions = Vec::new();
        for step in 0..12_000 {
            let id = format!("o{:04}", rng.next() % 2_000);
            if rng.next() % 3 == 0 {
                match set.without(&id) {
                    Some(next) => {
                        assert!(model.remove(&id));
                        set = next;
                    }
                    None => assert!(!model.contains(&id)),
                }
            } else {
                let had = model.contains(&id);
                set = set.with(&Observer::test(&id, step));
                model.insert(id.clone());
                assert_eq!(
                    set.get(&id).map(Observer::seq),
                    Some(step),
                    "replaced: {had}"
                );
            }
            assert_eq!(set.len(), model.len());
            if step % 991 == 0 {
                check(&set);
                assert_eq!(ids(&set), model.iter().cloned().collect::<Vec<_>>());
                versions.push((set.clone(), ids(&set)));
            }
        }
        for (version, expected) in &versions {
            assert_eq!(&ids(version), expected, "an earlier version changed");
        }
        let mut left: Vec<String> = model.into_iter().collect();
        while !left.is_empty() {
            let id = left.swap_remove((rng.next() % left.len() as u64) as usize);
            set = set.without(&id).expect("held");
            check(&set);
        }
        assert_eq!(set.len(), 0);
        assert_eq!(set.iter().count(), 0);
    }
}
