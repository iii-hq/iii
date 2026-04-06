//! Inode table with dual-key lookup for filesystem backends.
//!
//! Provides [`MultikeyBTreeMap`] (a BTreeMap with two key types), [`InodeData`]
//! for per-inode state, and [`InodeAltKey`] for host-identity-based deduplication.

#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicI64;
use std::{borrow::Borrow, collections::BTreeMap, sync::atomic::AtomicU64};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A BTreeMap that supports 2 types of keys per value.
///
/// There is a 1:1 relationship between the two key types: for each `K1` in the
/// map there is exactly one `K2` and vice versa.
///
/// Copied from msb_krun's `src/devices/src/virtio/fs/multikey.rs` to avoid
/// depending on msb_krun internals.
#[derive(Default)]
pub(crate) struct MultikeyBTreeMap<K1, K2, V>
where
    K1: Ord,
    K2: Ord,
{
    main: BTreeMap<K1, (K2, V)>,
    alt: BTreeMap<K2, K1>,
}

/// Alternate key for inode lookup based on host filesystem identity.
///
/// On Linux, includes `mnt_id` from `statx` to prevent cross-mount collisions.
/// On macOS, uses `(ino, dev)` which is sufficient since there are no bind mounts.
#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Debug)]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) struct InodeAltKey {
    pub ino: u64,
    pub dev: u64,
    #[cfg(target_os = "linux")]
    pub mnt_id: u64,
}

/// Per-inode data tracked by the filesystem backend.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) struct InodeData {
    /// Synthetic FUSE inode number (monotonically increasing, never reused).
    pub inode: u64,

    /// Host inode number.
    pub ino: u64,

    /// Host device ID.
    pub dev: u64,

    /// FUSE lookup reference count. When this reaches 0, the inode is removed.
    pub refcount: AtomicU64,

    /// O_PATH file descriptor pinning this inode on the host filesystem.
    #[cfg(target_os = "linux")]
    pub file: std::fs::File,

    /// Mount ID from statx (Linux only, for cross-mount deduplication).
    #[cfg(target_os = "linux")]
    pub mnt_id: u64,

    /// Fd grabbed before unlink, keeping the file accessible after deletion.
    ///
    /// On macOS, `/.vol/<dev>/<ino>` may become invalid after unlink. This fd
    /// (set by `do_unlink`) keeps the file data alive for open handles. -1 means
    /// the file has not been unlinked.
    #[cfg(target_os = "macos")]
    pub unlinked_fd: AtomicI64,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl<K1, K2, V> MultikeyBTreeMap<K1, K2, V>
where
    K1: Clone + Ord,
    K2: Clone + Ord,
{
    /// Create a new empty MultikeyBTreeMap.
    pub fn new() -> Self {
        MultikeyBTreeMap {
            main: BTreeMap::default(),
            alt: BTreeMap::default(),
        }
    }

    /// Returns a reference to the value corresponding to the primary key.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K1: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.main.get(key).map(|(_, v)| v)
    }

    /// Returns a reference to the value corresponding to the alternate key.
    ///
    /// Performs 2 lookups: alt -> primary key, then primary key -> value.
    pub fn get_alt<Q2>(&self, key: &Q2) -> Option<&V>
    where
        K2: Borrow<Q2>,
        Q2: Ord + ?Sized,
    {
        if let Some(k) = self.alt.get(key) {
            self.get(k)
        } else {
            None
        }
    }

    /// Insert a new entry with both keys. Returns the old value if either key
    /// was already present.
    pub fn insert(&mut self, k1: K1, k2: K2, v: V) -> Option<V> {
        let oldval = if let Some(oldkey) = self.alt.insert(k2.clone(), k1.clone()) {
            self.main.remove(&oldkey)
        } else {
            None
        };
        self.main
            .insert(k1, (k2.clone(), v))
            .or(oldval)
            .map(|(oldk2, v)| {
                if oldk2 != k2 {
                    self.alt.remove(&oldk2);
                }
                v
            })
    }

    /// Remove an entry by its primary key.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K1: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.main.remove(key).map(|(k2, v)| {
            self.alt.remove(&k2);
            v
        })
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.alt.clear();
        self.main.clear();
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl InodeAltKey {
    /// Create a new alternate key from stat fields.
    #[cfg(target_os = "linux")]
    pub fn new(ino: u64, dev: u64, mnt_id: u64) -> Self {
        Self { ino, dev, mnt_id }
    }

    /// Create a new alternate key from stat fields.
    #[cfg(target_os = "macos")]
    pub fn new(ino: u64, dev: u64) -> Self {
        Self { ino, dev }
    }
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut map = MultikeyBTreeMap::<u64, String, &str>::new();
        assert!(map.insert(1, "one".to_string(), "first").is_none());
        assert_eq!(map.get(&1), Some(&"first"));
    }

    #[test]
    fn get_alt() {
        let mut map = MultikeyBTreeMap::<u64, String, &str>::new();
        map.insert(1, "one".to_string(), "first");
        assert_eq!(map.get_alt(&"one".to_string()), Some(&"first"));
        assert_eq!(map.get_alt(&"two".to_string()), None);
    }

    #[test]
    fn remove() {
        let mut map = MultikeyBTreeMap::<u64, String, &str>::new();
        map.insert(1, "one".to_string(), "first");
        assert_eq!(map.remove(&1), Some("first"));
        assert_eq!(map.get(&1), None);
        assert_eq!(map.get_alt(&"one".to_string()), None);
    }

    #[test]
    fn insert_overwrites_existing_alt_key() {
        let mut map = MultikeyBTreeMap::<u64, String, &str>::new();
        map.insert(1, "one".to_string(), "first");
        // Insert with a different primary key but same alt key replaces the entry.
        let old = map.insert(2, "one".to_string(), "second");
        assert_eq!(old, Some("first"));
        assert_eq!(map.get(&1), None);
        assert_eq!(map.get(&2), Some(&"second"));
        assert_eq!(map.get_alt(&"one".to_string()), Some(&"second"));
    }

    #[test]
    fn clear() {
        let mut map = MultikeyBTreeMap::<u64, String, &str>::new();
        map.insert(1, "one".to_string(), "first");
        map.insert(2, "two".to_string(), "second");
        map.clear();
        assert_eq!(map.get(&1), None);
        assert_eq!(map.get(&2), None);
        assert_eq!(map.get_alt(&"one".to_string()), None);
    }
}
