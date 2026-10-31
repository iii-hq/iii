//! A persistent ordered map: an AVL tree whose nodes are shared between
//! versions. Cloning a map is one reference-count increment; inserting or
//! removing one entry copies the `O(log n)` nodes on its path (fewer when a
//! node is not shared: a version nobody else holds is changed in place) and
//! shares every other node with the version it was made from, which stays
//! valid for whoever holds it.
//!
//! Lookups take a comparison function, so a map keyed by an owned value can
//! be searched with a borrowed one. Iteration is in key order and allocates
//! nothing: its stack lives inline, sized for the tallest tree a `usize`
//! count of entries can make.

use std::cmp::Ordering;
use std::sync::Arc;

/// Height of the tallest AVL tree with at most `usize::MAX` nodes, rounded
/// up: `1.4405 * log2(n + 2)` is below 93.
const MAX_HEIGHT: usize = 96;

pub(crate) struct PMap<K, V> {
    root: Link<K, V>,
    len: usize,
}

type Link<K, V> = Option<Arc<Node<K, V>>>;

#[derive(Clone)]
struct Node<K, V> {
    key: K,
    value: V,
    /// Of the subtree rooted here; a leaf is 1.
    height: u8,
    left: Link<K, V>,
    right: Link<K, V>,
}

impl<K, V> Clone for PMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            len: self.len,
        }
    }
}

impl<K, V> Default for PMap<K, V> {
    fn default() -> Self {
        Self { root: None, len: 0 }
    }
}

fn height<K, V>(link: &Link<K, V>) -> u8 {
    link.as_ref().map_or(0, |node| node.height)
}

impl<K, V> Node<K, V> {
    fn fix_height(&mut self) {
        self.height = 1 + height(&self.left).max(height(&self.right));
    }
}

impl<K, V> PMap<K, V> {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The entry whose key `cmp` finds equal. `cmp(key)` orders `key`
    /// against the key looked for, as `key.cmp(&wanted)` would.
    pub(crate) fn get_by(&self, cmp: impl Fn(&K) -> Ordering) -> Option<(&K, &V)> {
        let mut link = &self.root;
        while let Some(node) = link {
            match cmp(&node.key) {
                Ordering::Less => link = &node.right,
                Ordering::Greater => link = &node.left,
                Ordering::Equal => return Some((&node.key, &node.value)),
            }
        }
        None
    }

    /// Every entry, in key order.
    pub(crate) fn iter(&self) -> Iter<'_, K, V> {
        let mut iter = Iter {
            stack: [None; MAX_HEIGHT],
            depth: 0,
            remaining: self.len,
        };
        iter.descend(self.root.as_deref());
        iter
    }
}

impl<K: Ord, V> PMap<K, V> {
    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        self.get_by(|held| held.cmp(key)).map(|(_, value)| value)
    }

    pub(crate) fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }
}

impl<K: Ord + Clone, V: Clone> PMap<K, V> {
    /// Puts `value` under `key`; returns the value it replaced.
    pub(crate) fn insert(&mut self, key: K, value: V) -> Option<V> {
        let replaced = insert(&mut self.root, key, value);
        if replaced.is_none() {
            self.len += 1;
        }
        replaced
    }

    /// Takes the entry under `key` out; returns its value. A key the map
    /// does not hold changes nothing (no node is copied).
    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        if !self.contains_key(key) {
            return None;
        }
        let removed = remove(&mut self.root, key);
        if removed.is_some() {
            self.len -= 1;
        }
        removed
    }
}

fn insert<K: Ord + Clone, V: Clone>(link: &mut Link<K, V>, key: K, value: V) -> Option<V> {
    let Some(node) = link else {
        *link = Some(Arc::new(Node {
            key,
            value,
            height: 1,
            left: None,
            right: None,
        }));
        return None;
    };
    let node = Arc::make_mut(node);
    let replaced = match key.cmp(&node.key) {
        Ordering::Less => insert(&mut node.left, key, value),
        Ordering::Greater => insert(&mut node.right, key, value),
        Ordering::Equal => return Some(std::mem::replace(&mut node.value, value)),
    };
    rebalance(link);
    replaced
}

fn remove<K: Ord + Clone, V: Clone>(link: &mut Link<K, V>, key: &K) -> Option<V> {
    let node = Arc::make_mut(link.as_mut()?);
    let removed = match key.cmp(&node.key) {
        Ordering::Less => remove(&mut node.left, key),
        Ordering::Greater => remove(&mut node.right, key),
        Ordering::Equal => {
            let value = node.value.clone();
            match (node.left.take(), node.right.take()) {
                (None, None) => *link = None,
                (Some(child), None) | (None, Some(child)) => *link = Some(child),
                (left, Some(right)) => {
                    let mut right = Some(right);
                    let (key, next) = remove_first(&mut right);
                    node.key = key;
                    node.value = next;
                    node.left = left;
                    node.right = right;
                }
            }
            Some(value)
        }
    };
    if link.is_some() {
        rebalance(link);
    }
    removed
}

/// Takes the smallest entry out of a subtree that is not empty.
fn remove_first<K: Clone, V: Clone>(link: &mut Link<K, V>) -> (K, V) {
    let node = Arc::make_mut(link.as_mut().expect("a subtree that is not empty"));
    if node.left.is_some() {
        let first = remove_first(&mut node.left);
        rebalance(link);
        first
    } else {
        let first = (node.key.clone(), node.value.clone());
        let right = node.right.take();
        *link = right;
        first
    }
}

/// Restores the AVL balance at `link`, whose subtrees are balanced and differ
/// in height by at most two, and its height.
fn rebalance<K: Clone, V: Clone>(link: &mut Link<K, V>) {
    let Some(node) = link.as_mut() else {
        return;
    };
    let node = Arc::make_mut(node);
    node.fix_height();
    let (left, right) = (height(&node.left), height(&node.right));
    if left > right + 1 {
        let child = node.left.as_ref().expect("the taller side");
        if height(&child.left) < height(&child.right) {
            rotate_left(&mut node.left);
        }
        rotate_right(link);
    } else if right > left + 1 {
        let child = node.right.as_ref().expect("the taller side");
        if height(&child.right) < height(&child.left) {
            rotate_right(&mut node.right);
        }
        rotate_left(link);
    }
}

fn rotate_right<K: Clone, V: Clone>(link: &mut Link<K, V>) {
    let mut top = link.take().expect("a node to rotate");
    let parent = Arc::make_mut(&mut top);
    let mut pivot = parent.left.take().expect("a left child to rotate up");
    let child = Arc::make_mut(&mut pivot);
    parent.left = child.right.take();
    parent.fix_height();
    child.right = Some(top);
    child.fix_height();
    *link = Some(pivot);
}

fn rotate_left<K: Clone, V: Clone>(link: &mut Link<K, V>) {
    let mut top = link.take().expect("a node to rotate");
    let parent = Arc::make_mut(&mut top);
    let mut pivot = parent.right.take().expect("a right child to rotate up");
    let child = Arc::make_mut(&mut pivot);
    parent.right = child.left.take();
    parent.fix_height();
    child.left = Some(top);
    child.fix_height();
    *link = Some(pivot);
}

/// In-order iteration with an inline stack.
pub(crate) struct Iter<'a, K, V> {
    stack: [Option<&'a Node<K, V>>; MAX_HEIGHT],
    depth: usize,
    remaining: usize,
}

impl<'a, K, V> Iter<'a, K, V> {
    fn descend(&mut self, mut node: Option<&'a Node<K, V>>) {
        while let Some(at) = node {
            self.stack[self.depth] = Some(at);
            self.depth += 1;
            node = at.left.as_deref();
        }
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.depth == 0 {
            return None;
        }
        self.depth -= 1;
        let node = self.stack[self.depth].take().expect("a pushed node");
        self.descend(node.right.as_deref());
        self.remaining -= 1;
        Some((&node.key, &node.value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<K, V> ExactSizeIterator for Iter<'_, K, V> {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// xorshift64*, as in `tests/support/gen.rs`.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
    }

    /// The AVL invariants: keys in order, heights right, balanced.
    fn check<K: Ord + Clone + std::fmt::Debug, V>(map: &PMap<K, V>) {
        fn walk<K: Ord + Clone + std::fmt::Debug, V>(
            link: &Link<K, V>,
            low: Option<&K>,
            high: Option<&K>,
        ) -> (u8, usize) {
            let Some(node) = link else {
                return (0, 0);
            };
            assert!(low.is_none_or(|low| *low < node.key), "order");
            assert!(high.is_none_or(|high| node.key < *high), "order");
            let (left, left_len) = walk(&node.left, low, Some(&node.key));
            let (right, right_len) = walk(&node.right, Some(&node.key), high);
            assert_eq!(node.height, 1 + left.max(right), "height");
            assert!(left.abs_diff(right) <= 1, "balance");
            (node.height, left_len + right_len + 1)
        }
        let (_, len) = walk(&map.root, None, None);
        assert_eq!(len, map.len());
    }

    #[test]
    fn matches_a_btree_map_and_keeps_every_version() {
        for seed in 1..=8u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut map: PMap<u64, u64> = PMap::default();
            let mut model: BTreeMap<u64, u64> = BTreeMap::new();
            let mut versions: Vec<(PMap<u64, u64>, BTreeMap<u64, u64>)> = Vec::new();
            for step in 0..4_000u64 {
                let key = rng.next() % 512;
                if rng.next() % 3 == 0 {
                    assert_eq!(
                        map.remove(&key),
                        model.remove(&key),
                        "seed {seed} step {step}"
                    );
                } else {
                    assert_eq!(
                        map.insert(key, step),
                        model.insert(key, step),
                        "seed {seed} step {step}"
                    );
                }
                assert_eq!(map.len(), model.len());
                if step % 97 == 0 {
                    versions.push((map.clone(), model.clone()));
                }
                if step % 401 == 0 {
                    check(&map);
                }
            }
            check(&map);
            let held: Vec<(u64, u64)> = map.iter().map(|(k, v)| (*k, *v)).collect();
            let expected: Vec<(u64, u64)> = model.iter().map(|(k, v)| (*k, *v)).collect();
            assert_eq!(held, expected);
            assert_eq!(map.iter().len(), model.len());
            for key in 0..512 {
                assert_eq!(map.get(&key), model.get(&key));
            }
            // Every version taken along the way still reads as it did.
            for (version, model) in &versions {
                check(version);
                let held: Vec<(u64, u64)> = version.iter().map(|(k, v)| (*k, *v)).collect();
                let expected: Vec<(u64, u64)> = model.iter().map(|(k, v)| (*k, *v)).collect();
                assert_eq!(held, expected, "seed {seed}");
            }
        }
    }

    #[test]
    fn ascending_and_descending_inserts_stay_balanced() {
        let mut up: PMap<u64, ()> = PMap::default();
        let mut down: PMap<u64, ()> = PMap::default();
        for i in 0..10_000u64 {
            up.insert(i, ());
            down.insert(10_000 - i, ());
        }
        check(&up);
        check(&down);
        // 1.44 * log2(10_002) is about 19.1.
        assert!(height(&up.root) <= 19 && height(&down.root) <= 19);
        for i in (0..10_000u64).step_by(2) {
            up.remove(&i);
        }
        check(&up);
        assert_eq!(up.len(), 5_000);
        assert!(up.iter().all(|(k, _)| k % 2 == 1));
    }

    #[test]
    fn a_change_copies_only_the_path_it_takes() {
        let mut map: PMap<u64, u64> = PMap::default();
        for i in 0..1_024u64 {
            map.insert(i, i);
        }
        let before = map.clone();
        map.insert(5_000, 0);
        // Nodes the two versions share, by address: all but the path.
        fn nodes<K, V>(link: &Link<K, V>, out: &mut Vec<*const Node<K, V>>) {
            if let Some(node) = link {
                out.push(Arc::as_ptr(node));
                nodes(&node.left, out);
                nodes(&node.right, out);
            }
        }
        let (mut a, mut b) = (Vec::new(), Vec::new());
        nodes(&before.root, &mut a);
        nodes(&map.root, &mut b);
        let a: std::collections::HashSet<_> = a.into_iter().collect();
        let copied = b.iter().filter(|node| !a.contains(node)).count();
        assert!(copied <= 2 * MAX_HEIGHT.min(16), "copied {copied} nodes");
        assert_eq!(before.len(), 1_024);
        assert!(before.get(&5_000).is_none());
        assert_eq!(map.get(&5_000), Some(&0));
    }

    #[test]
    fn a_borrowed_key_finds_an_owned_one() {
        let mut map: PMap<Box<str>, u32> = PMap::default();
        for (i, word) in ["delta", "alpha", "charlie", "bravo"].iter().enumerate() {
            map.insert((*word).into(), i as u32);
        }
        let probe = "charlie";
        let found = map.get_by(|held| (**held).cmp(probe));
        assert_eq!(found.map(|(_, v)| *v), Some(2));
        assert!(map.get_by(|held| (**held).cmp("echo")).is_none());
        let keys: Vec<&str> = map.iter().map(|(k, _)| &**k).collect();
        assert_eq!(keys, ["alpha", "bravo", "charlie", "delta"]);
    }

    #[test]
    fn removing_a_missing_key_shares_everything() {
        let mut map: PMap<u64, u64> = PMap::default();
        for i in 0..100 {
            map.insert(i * 2, i);
        }
        let before = map.clone();
        assert_eq!(map.remove(&3), None);
        assert!(Arc::ptr_eq(
            before.root.as_ref().unwrap(),
            map.root.as_ref().unwrap()
        ));
    }
}
