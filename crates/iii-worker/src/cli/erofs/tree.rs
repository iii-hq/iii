use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

const DEFAULT_MAX_TOTAL_SIZE: u64 = 10 * 1024 * 1024 * 1024; // 10 GiB
const DEFAULT_MAX_FILE_SIZE: u64 = 5 * 1024 * 1024 * 1024; // 5 GiB
const DEFAULT_MAX_ENTRY_COUNT: u64 = 1_000_000;
const DEFAULT_MAX_PATH_LENGTH: usize = 4096;
const DEFAULT_MAX_PATH_DEPTH: usize = 128;
const DEFAULT_MAX_SYMLINK_TARGET: usize = 4096;

const DEFAULT_DIR_MODE: u16 = 0o755;

/// Overlayfs whiteout: char device with major=0, minor=0 signals deletion.
pub(crate) const WHITEOUT_MAJOR: u32 = 0;
pub(crate) const WHITEOUT_MINOR: u32 = 0;

/// Overlayfs opaque directory xattr: hides all lower-layer entries.
pub(crate) const OPAQUE_XATTR_NAME: &[u8] = b"trusted.overlay.opaque";
pub(crate) const OPAQUE_XATTR_VALUE: &[u8] = b"y";

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// File content storage — either in-memory for small files or spooled to
/// disk for large files to keep memory usage bounded.
#[derive(Clone)]
pub enum FileData {
    /// Small file content held in memory.
    Memory(Vec<u8>),
    /// In-memory content shared by hardlinked regular-file aliases.
    SharedMemory(Arc<[u8]>),
    /// Large file content written to a shared spool file on disk.
    /// Multiple `FileData::Spool` entries can reference different regions
    /// of the same underlying spool file via `Arc`.
    Spool {
        spool: Arc<std::sync::Mutex<std::fs::File>>,
        offset: u64,
        len: u64,
    },
}

impl std::fmt::Debug for FileData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileData::Memory(data) => f.debug_tuple("Memory").field(&data.len()).finish(),
            FileData::SharedMemory(data) => {
                f.debug_tuple("SharedMemory").field(&data.len()).finish()
            }
            FileData::Spool { offset, len, .. } => f
                .debug_struct("Spool")
                .field("offset", offset)
                .field("len", len)
                .finish(),
        }
    }
}

impl PartialEq for FileData {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (FileData::Memory(a), FileData::Memory(b)) => a == b,
            (FileData::Memory(a), FileData::SharedMemory(b))
            | (FileData::SharedMemory(b), FileData::Memory(a)) => a.as_slice() == b.as_ref(),
            (FileData::SharedMemory(a), FileData::SharedMemory(b)) => a.as_ref() == b.as_ref(),
            _ => false,
        }
    }
}

/// Threshold below which file data is kept in memory (64 KiB).
/// Files at or above this size are spooled to disk during tar ingestion.
pub const SPOOL_THRESHOLD: u64 = 64 * 1024;

/// A writable spool file for large file data during tar ingestion.
pub struct DataSpool {
    file: std::fs::File,
    shared: Arc<std::sync::Mutex<std::fs::File>>,
    offset: u64,
}

pub struct ResourceLimits {
    pub max_total_size: u64,
    pub max_file_size: u64,
    pub max_entry_count: u64,
    pub max_path_length: usize,
    pub max_path_depth: usize,
    pub max_symlink_target: usize,
}

#[derive(Clone)]
pub struct InodeMetadata {
    pub uid: u32,
    pub gid: u32,
    pub mode: u16,
    pub mtime: u64,
    pub mtime_nsec: u32,
}

#[derive(Clone)]
pub struct Xattr {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegularFileId(u64);

#[derive(Clone)]
pub enum TreeNode {
    RegularFile(RegularFileNode),
    Directory(DirectoryNode),
    Symlink(SymlinkNode),
    CharDevice(DeviceNode),
    BlockDevice(DeviceNode),
    Fifo(InodeMetadata),
    Socket(InodeMetadata),
}

#[derive(Clone)]
pub struct RegularFileNode {
    pub id: RegularFileId,
    pub metadata: InodeMetadata,
    pub xattrs: Vec<Xattr>,
    pub data: FileData,
    pub nlink: u32,
}

#[derive(Clone)]
pub struct DirectoryNode {
    pub metadata: InodeMetadata,
    pub xattrs: Vec<Xattr>,
    pub entries: BTreeMap<OsString, TreeNode>,
}

#[derive(Clone)]
pub struct SymlinkNode {
    pub metadata: InodeMetadata,
    pub target: Vec<u8>,
}

#[derive(Clone)]
pub struct DeviceNode {
    pub metadata: InodeMetadata,
    pub major: u32,
    pub minor: u32,
}

#[derive(Clone)]
pub struct FileTree {
    pub root: DirectoryNode,
}

#[derive(Debug)]
pub enum FileTreeError {
    PathEmpty,
    PathTraversal(String),
    NotADirectory(String),
    EntryExists(String),
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl FileData {
    /// Total byte length of the file content.
    pub fn len(&self) -> usize {
        match self {
            FileData::Memory(v) => v.len(),
            FileData::SharedMemory(v) => v.len(),
            FileData::Spool { len, .. } => *len as usize,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Read the entire content into memory. For `Memory` variant this
    /// clones; for `Spool` this reads from disk.
    pub fn read_all(&self) -> std::io::Result<Vec<u8>> {
        match self {
            FileData::Memory(v) => Ok(v.clone()),
            FileData::SharedMemory(v) => Ok(v.to_vec()),
            FileData::Spool { spool, offset, len } => {
                let mut buf = vec![0u8; *len as usize];
                let mut file = spool
                    .lock()
                    .map_err(|_| std::io::Error::other("spool lock poisoned"))?;
                file.seek(SeekFrom::Start(*offset))?;
                file.read_exact(&mut buf)?;
                Ok(buf)
            }
        }
    }

    /// Borrow the in-memory bytes directly (only for `Memory` variant).
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            FileData::Memory(v) => Some(v),
            FileData::SharedMemory(v) => Some(v),
            FileData::Spool { .. } => None,
        }
    }

    /// Write content to an output writer, reading from spool if needed.
    /// Avoids loading the entire file into memory for large spooled files.
    pub fn write_to(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
        self.write_range(0, self.len(), out)
    }

    /// Write a byte range of the content to an output writer.
    pub fn write_range(
        &self,
        start: usize,
        len: usize,
        out: &mut impl std::io::Write,
    ) -> std::io::Result<()> {
        match self {
            FileData::Memory(v) => out.write_all(&v[start..start + len]),
            FileData::SharedMemory(v) => out.write_all(&v[start..start + len]),
            FileData::Spool { spool, offset, .. } => {
                let mut file = spool
                    .lock()
                    .map_err(|_| std::io::Error::other("spool lock poisoned"))?;
                file.seek(SeekFrom::Start(*offset + start as u64))?;
                let mut remaining = len;
                let mut buf = [0u8; 65536];
                while remaining > 0 {
                    let to_read = remaining.min(buf.len());
                    file.read_exact(&mut buf[..to_read])?;
                    out.write_all(&buf[..to_read])?;
                    remaining -= to_read;
                }
                Ok(())
            }
        }
    }

    /// Return a shared reference suitable for a hardlinked alias.
    pub fn clone_ref(&mut self) -> FileData {
        match self {
            FileData::Memory(data) => {
                let shared: Arc<[u8]> = std::mem::take(data).into();
                *self = FileData::SharedMemory(Arc::clone(&shared));
                FileData::SharedMemory(shared)
            }
            FileData::SharedMemory(data) => FileData::SharedMemory(Arc::clone(data)),
            FileData::Spool { spool, offset, len } => FileData::Spool {
                spool: Arc::clone(spool),
                offset: *offset,
                len: *len,
            },
        }
    }
}

impl RegularFileId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for RegularFileId {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSpool {
    /// Create a new spool file at the given path.
    pub fn new(path: &std::path::Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(path)?;
        let shared = Arc::new(std::sync::Mutex::new(file.try_clone()?));
        Ok(Self {
            file,
            shared,
            offset: 0,
        })
    }

    /// Write data to the spool and return a `FileData::Spool` reference.
    pub fn write_data(&mut self, data: &[u8]) -> std::io::Result<FileData> {
        use std::io::Write;
        let offset = self.offset;
        self.file.write_all(data)?;
        self.offset += data.len() as u64;
        Ok(FileData::Spool {
            spool: Arc::clone(&self.shared),
            offset,
            len: data.len() as u64,
        })
    }

    pub fn current_offset(&self) -> u64 {
        self.offset
    }

    pub fn write_chunk(&mut self, data: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        self.file.write_all(data)?;
        self.offset += data.len() as u64;
        Ok(())
    }

    pub fn data_ref(&self, offset: u64, len: u64) -> FileData {
        FileData::Spool {
            spool: Arc::clone(&self.shared),
            offset,
            len,
        }
    }
}

impl DirectoryNode {
    pub fn new(metadata: InodeMetadata) -> Self {
        Self {
            metadata,
            xattrs: Vec::new(),
            entries: BTreeMap::new(),
        }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for FileTree {
    fn default() -> Self {
        Self::new()
    }
}

impl FileTree {
    pub fn new() -> Self {
        Self {
            root: DirectoryNode::new(InodeMetadata::default()),
        }
    }

    pub fn insert(&mut self, path: &[u8], node: TreeNode) -> Result<(), FileTreeError> {
        use std::collections::btree_map::Entry;

        let components = split_path(path)?;
        if components.is_empty() {
            return Err(FileTreeError::PathEmpty);
        }

        let (parent_components, file_name) = components.split_at(components.len() - 1);

        // Traverse to the parent directory, creating missing intermediates.
        // Uses the BTreeMap entry API to do a single lookup per component
        // instead of contains_key + insert + get_mut (3 lookups).
        let mut current = &mut self.root;
        for component in parent_components {
            let key = OsStr::from_bytes(component).to_os_string();
            current = match current.entries.entry(key) {
                Entry::Vacant(e) => {
                    let dir = TreeNode::Directory(DirectoryNode::new(InodeMetadata::default()));
                    match e.insert(dir) {
                        TreeNode::Directory(d) => d,
                        _ => unreachable!(),
                    }
                }
                Entry::Occupied(e) => match e.into_mut() {
                    TreeNode::Directory(d) => d,
                    _ => {
                        let path_str = String::from_utf8_lossy(component).into_owned();
                        return Err(FileTreeError::NotADirectory(path_str));
                    }
                },
            };
        }

        // Insert the final node. Directory-over-directory merges metadata
        // but keeps existing entries. Non-directory replaces non-directory.
        let key = OsStr::from_bytes(file_name[0]).to_os_string();
        match current.entries.entry(key) {
            Entry::Vacant(e) => {
                e.insert(node);
            }
            Entry::Occupied(mut e) => match (e.get(), &node) {
                (TreeNode::Directory(_), TreeNode::Directory(_)) => {
                    if let TreeNode::Directory(existing) = e.get_mut()
                        && let TreeNode::Directory(new_dir) = node
                    {
                        existing.metadata = new_dir.metadata;
                        existing.xattrs = new_dir.xattrs;
                    }
                }
                (TreeNode::Directory(_), _) => {
                    let path_str = String::from_utf8_lossy(file_name[0]).into_owned();
                    return Err(FileTreeError::EntryExists(path_str));
                }
                _ => {
                    e.insert(node);
                }
            },
        }

        Ok(())
    }

    pub fn get(&self, path: &[u8]) -> Option<&TreeNode> {
        let components = split_path(path).ok()?;
        if components.is_empty() {
            return None;
        }

        let (parent_components, file_name) = components.split_at(components.len() - 1);

        let mut current = &self.root;
        for component in parent_components {
            let key = OsStr::from_bytes(component);
            match current.entries.get(key) {
                Some(TreeNode::Directory(dir)) => {
                    current = dir;
                }
                _ => return None,
            }
        }

        current.entries.get(OsStr::from_bytes(file_name[0]))
    }

    pub fn get_mut(&mut self, path: &[u8]) -> Option<&mut TreeNode> {
        let components = split_path(path).ok()?;
        if components.is_empty() {
            return None;
        }

        let (parent_components, file_name) = components.split_at(components.len() - 1);

        let mut current = &mut self.root;
        for component in parent_components {
            let key = OsStr::from_bytes(component);
            match current.entries.get_mut(key) {
                Some(TreeNode::Directory(dir)) => {
                    current = dir;
                }
                _ => return None,
            }
        }

        current.entries.get_mut(OsStr::from_bytes(file_name[0]))
    }

    pub fn remove(&mut self, path: &[u8]) -> Option<TreeNode> {
        let components = split_path(path).ok()?;
        if components.is_empty() {
            return None;
        }

        let (parent_components, file_name) = components.split_at(components.len() - 1);

        let mut current = &mut self.root;
        for component in parent_components {
            let key = OsStr::from_bytes(component);
            match current.entries.get_mut(key) {
                Some(TreeNode::Directory(dir)) => {
                    current = dir;
                }
                _ => return None,
            }
        }

        current.entries.remove(OsStr::from_bytes(file_name[0]))
    }

    pub fn node_count(&self) -> u64 {
        count_nodes_in_dir(&self.root)
    }

    pub fn total_data_size(&self) -> u64 {
        data_size_in_dir(&self.root)
    }

    pub(crate) fn regular_file_link_counts(&self) -> HashMap<RegularFileId, u32> {
        let mut counts = HashMap::new();
        count_regular_links_in_dir(&self.root, &mut counts);
        counts
    }

    pub(crate) fn refresh_regular_nlinks(&mut self) {
        let counts = self.regular_file_link_counts();
        refresh_regular_nlinks_in_dir(&mut self.root, &counts);
    }

    pub fn merge_layer(&mut self, layer: FileTree) {
        merge_directory(&mut self.root, layer.root);
        self.refresh_regular_nlinks();
    }

    /// Strip file data from this tree, keeping only directory structure and metadata.
    ///
    /// After calling this, all `RegularFile` nodes have empty `FileData::Memory(Vec::new())`.
    /// Used to reduce memory after writing a per-layer EROFS while retaining the tree
    /// for fsmeta merge.
    pub fn strip_file_data(&mut self) {
        strip_data_in_dir(&mut self.root);
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_total_size: DEFAULT_MAX_TOTAL_SIZE,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            max_entry_count: DEFAULT_MAX_ENTRY_COUNT,
            max_path_length: DEFAULT_MAX_PATH_LENGTH,
            max_path_depth: DEFAULT_MAX_PATH_DEPTH,
            max_symlink_target: DEFAULT_MAX_SYMLINK_TARGET,
        }
    }
}

impl Default for InodeMetadata {
    fn default() -> Self {
        Self {
            uid: 0,
            gid: 0,
            mode: DEFAULT_DIR_MODE,
            mtime: 0,
            mtime_nsec: 0,
        }
    }
}

impl fmt::Display for FileTreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileTreeError::PathEmpty => write!(f, "path is empty"),
            FileTreeError::PathTraversal(p) => {
                write!(f, "path traversal attempt: \"..\" in path \"{p}\"")
            }
            FileTreeError::NotADirectory(p) => {
                write!(f, "not a directory: \"{p}\"")
            }
            FileTreeError::EntryExists(p) => {
                write!(f, "entry already exists: \"{p}\"")
            }
        }
    }
}

impl std::error::Error for FileTreeError {}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

fn split_path(path: &[u8]) -> Result<Vec<&[u8]>, FileTreeError> {
    let components: Vec<&[u8]> = path
        .split(|&b| b == b'/')
        .filter(|c| !c.is_empty())
        .collect();

    if components.is_empty() {
        return Err(FileTreeError::PathEmpty);
    }

    for component in &components {
        if *component == b".." {
            let path_str = String::from_utf8_lossy(path).into_owned();
            return Err(FileTreeError::PathTraversal(path_str));
        }
    }

    Ok(components)
}

fn count_nodes_in_dir(dir: &DirectoryNode) -> u64 {
    let mut count = 0u64;
    for node in dir.entries.values() {
        count += 1;
        if let TreeNode::Directory(child_dir) = node {
            count += count_nodes_in_dir(child_dir);
        }
    }
    count
}

fn data_size_in_dir(dir: &DirectoryNode) -> u64 {
    let mut seen = HashSet::new();
    data_size_in_dir_once(dir, &mut seen)
}

fn data_size_in_dir_once(dir: &DirectoryNode, seen: &mut HashSet<RegularFileId>) -> u64 {
    let mut size = 0u64;
    for node in dir.entries.values() {
        match node {
            TreeNode::RegularFile(file) if seen.insert(file.id) => {
                size += file.data.len() as u64;
            }
            TreeNode::Directory(child_dir) => {
                size += data_size_in_dir_once(child_dir, seen);
            }
            _ => {}
        }
    }
    size
}

fn count_regular_links_in_dir(dir: &DirectoryNode, counts: &mut HashMap<RegularFileId, u32>) {
    for node in dir.entries.values() {
        match node {
            TreeNode::RegularFile(file) => {
                *counts.entry(file.id).or_insert(0) += 1;
            }
            TreeNode::Directory(child_dir) => {
                count_regular_links_in_dir(child_dir, counts);
            }
            _ => {}
        }
    }
}

fn refresh_regular_nlinks_in_dir(dir: &mut DirectoryNode, counts: &HashMap<RegularFileId, u32>) {
    for node in dir.entries.values_mut() {
        match node {
            TreeNode::RegularFile(file) => {
                file.nlink = counts.get(&file.id).copied().unwrap_or(1);
            }
            TreeNode::Directory(child_dir) => {
                refresh_regular_nlinks_in_dir(child_dir, counts);
            }
            _ => {}
        }
    }
}

fn strip_data_in_dir(dir: &mut DirectoryNode) {
    for node in dir.entries.values_mut() {
        match node {
            TreeNode::RegularFile(f) => {
                f.data = FileData::Memory(Vec::new());
            }
            TreeNode::Directory(d) => {
                strip_data_in_dir(d);
            }
            _ => {}
        }
    }
}

/// Merge multiple layer trees into a single tree with provenance tracking.
///
/// Applies layers bottom-to-top, handling whiteouts and opaque directories.
/// Unlike `merge_layer()`, whiteout entries and opaque xattrs are consumed
/// and NOT propagated — the output is the clean final state.
///
/// Returns the merged tree and a map from file path to source layer index
/// (0-based, bottom-to-top).
pub fn merge_layers_with_provenance(layers: Vec<FileTree>) -> (FileTree, HashMap<PathBuf, usize>) {
    let mut merged = FileTree::new();
    let mut provenance: HashMap<PathBuf, usize> = HashMap::new();

    for (layer_idx, layer) in layers.into_iter().enumerate() {
        let path = PathBuf::new();
        merge_directory_with_provenance(
            &mut merged.root,
            layer.root,
            layer_idx,
            &path,
            &mut provenance,
        );
    }

    // Strip opaque xattrs from the final merged tree — they are overlayfs
    // directives consumed by the merge, not meaningful in fsmeta.
    strip_opaque_xattrs(&mut merged.root);
    merged.refresh_regular_nlinks();

    (merged, provenance)
}

fn merge_directory_with_provenance(
    base: &mut DirectoryNode,
    layer: DirectoryNode,
    layer_idx: usize,
    current_path: &Path,
    provenance: &mut HashMap<PathBuf, usize>,
) {
    for (name, layer_node) in layer.entries {
        let child_path = current_path.join(&name);

        // Whiteout: remove target from merged tree and its provenance.
        if is_whiteout_device(&layer_node) {
            // Remove provenance entries for the deleted item and all its descendants.
            base.entries.remove(&name);
            provenance.retain(|k, _| !k.starts_with(&child_path));
            continue;
        }

        match layer_node {
            TreeNode::Directory(layer_dir) => {
                let opaque = has_opaque_xattr(&layer_dir);

                match base.entries.get_mut(&name) {
                    Some(TreeNode::Directory(base_dir)) => {
                        if opaque {
                            // Remove all provenance for entries under this directory.
                            provenance.retain(|k, _| !k.starts_with(&child_path));
                            base_dir.entries.clear();
                        }
                        base_dir.metadata = layer_dir.metadata;
                        base_dir.xattrs = layer_dir.xattrs;
                        merge_directory_with_provenance(
                            base_dir,
                            DirectoryNode {
                                metadata: InodeMetadata::default(),
                                xattrs: Vec::new(),
                                entries: layer_dir.entries,
                            },
                            layer_idx,
                            &child_path,
                            provenance,
                        );
                    }
                    _ => {
                        // New directory replaces whatever was there.
                        provenance.retain(|k, _| !k.starts_with(&child_path));
                        // Record provenance for all entries in the new directory.
                        record_provenance_recursive(&layer_dir, layer_idx, &child_path, provenance);
                        base.entries.insert(name, TreeNode::Directory(layer_dir));
                    }
                }
            }
            other => {
                // Non-directory entry: record provenance.
                provenance.insert(child_path, layer_idx);
                base.entries.insert(name, other);
            }
        }
    }
}

fn record_provenance_recursive(
    dir: &DirectoryNode,
    layer_idx: usize,
    current_path: &Path,
    provenance: &mut HashMap<PathBuf, usize>,
) {
    for (name, child) in &dir.entries {
        let child_path = current_path.join(name);
        match child {
            TreeNode::Directory(child_dir) => {
                record_provenance_recursive(child_dir, layer_idx, &child_path, provenance);
            }
            _ => {
                provenance.insert(child_path, layer_idx);
            }
        }
    }
}

fn strip_opaque_xattrs(dir: &mut DirectoryNode) {
    dir.xattrs
        .retain(|x| !(x.name == OPAQUE_XATTR_NAME && x.value == OPAQUE_XATTR_VALUE));
    for node in dir.entries.values_mut() {
        if let TreeNode::Directory(child_dir) = node {
            strip_opaque_xattrs(child_dir);
        }
    }
}

fn is_whiteout_device(node: &TreeNode) -> bool {
    matches!(node, TreeNode::CharDevice(dev) if dev.major == WHITEOUT_MAJOR && dev.minor == WHITEOUT_MINOR)
}

fn has_opaque_xattr(dir: &DirectoryNode) -> bool {
    dir.xattrs
        .iter()
        .any(|x| x.name == OPAQUE_XATTR_NAME && x.value == OPAQUE_XATTR_VALUE)
}

fn merge_directory(base: &mut DirectoryNode, layer: DirectoryNode) {
    for (name, layer_node) in layer.entries {
        if is_whiteout_device(&layer_node) {
            base.entries.remove(&name);
            continue;
        }

        match layer_node {
            TreeNode::Directory(layer_dir) => {
                let opaque = has_opaque_xattr(&layer_dir);

                match base.entries.get_mut(&name) {
                    Some(TreeNode::Directory(base_dir)) => {
                        if opaque {
                            base_dir.entries.clear();
                        }
                        base_dir.metadata = layer_dir.metadata;
                        base_dir.xattrs = layer_dir.xattrs;
                        merge_directory(
                            base_dir,
                            DirectoryNode {
                                metadata: InodeMetadata::default(),
                                xattrs: Vec::new(),
                                entries: layer_dir.entries,
                            },
                        );
                    }
                    _ => {
                        base.entries.insert(name, TreeNode::Directory(layer_dir));
                    }
                }
            }
            other => {
                base.entries.insert(name, other);
            }
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_regular_file(data: &[u8]) -> TreeNode {
        make_regular_file_with_id(data, RegularFileId::new())
    }

    fn make_regular_file_with_id(data: &[u8], id: RegularFileId) -> TreeNode {
        TreeNode::RegularFile(RegularFileNode {
            id,
            metadata: InodeMetadata::default(),
            xattrs: Vec::new(),
            data: FileData::Memory(data.to_vec()),
            nlink: 1,
        })
    }

    fn make_directory() -> TreeNode {
        TreeNode::Directory(DirectoryNode::new(InodeMetadata::default()))
    }

    fn make_whiteout() -> TreeNode {
        TreeNode::CharDevice(DeviceNode {
            metadata: InodeMetadata::default(),
            major: 0,
            minor: 0,
        })
    }

    fn make_opaque_directory() -> DirectoryNode {
        DirectoryNode {
            metadata: InodeMetadata::default(),
            xattrs: vec![Xattr {
                name: OPAQUE_XATTR_NAME.to_vec(),
                value: OPAQUE_XATTR_VALUE.to_vec(),
            }],
            entries: BTreeMap::new(),
        }
    }

    #[test]
    fn insert_and_get_file() {
        let mut tree = FileTree::new();
        tree.insert(b"hello.txt", make_regular_file(b"hello world"))
            .unwrap();

        let node = tree.get(b"hello.txt").unwrap();
        match node {
            TreeNode::RegularFile(f) => {
                assert_eq!(f.data, FileData::Memory(b"hello world".to_vec()))
            }
            _ => panic!("expected regular file"),
        }
    }

    #[test]
    fn insert_with_missing_parents_creates_them() {
        let mut tree = FileTree::new();
        tree.insert(b"a/b/c/file.txt", make_regular_file(b"deep"))
            .unwrap();

        // Intermediate directories should exist.
        let node = tree.get(b"a").unwrap();
        assert!(matches!(node, TreeNode::Directory(_)));

        let node = tree.get(b"a/b").unwrap();
        assert!(matches!(node, TreeNode::Directory(_)));

        let node = tree.get(b"a/b/c").unwrap();
        assert!(matches!(node, TreeNode::Directory(_)));

        let node = tree.get(b"a/b/c/file.txt").unwrap();
        assert!(matches!(node, TreeNode::RegularFile(_)));
    }

    #[test]
    fn reject_dotdot_in_path() {
        let mut tree = FileTree::new();
        let result = tree.insert(b"a/../etc/passwd", make_regular_file(b"bad"));
        assert!(matches!(result, Err(FileTreeError::PathTraversal(_))));
    }

    #[test]
    fn merge_layer_replaces_file() {
        let mut base = FileTree::new();
        base.insert(b"config.txt", make_regular_file(b"old"))
            .unwrap();

        let mut layer = FileTree::new();
        layer
            .insert(b"config.txt", make_regular_file(b"new"))
            .unwrap();

        base.merge_layer(layer);

        match base.get(b"config.txt").unwrap() {
            TreeNode::RegularFile(f) => assert_eq!(f.data, FileData::Memory(b"new".to_vec())),
            _ => panic!("expected regular file"),
        }
    }

    #[test]
    fn merge_layer_whiteout_removes_file() {
        let mut base = FileTree::new();
        base.insert(b"dir/secret.txt", make_regular_file(b"sensitive"))
            .unwrap();

        let mut layer = FileTree::new();
        layer.insert(b"dir", make_directory()).unwrap();
        layer.insert(b"dir/secret.txt", make_whiteout()).unwrap();

        base.merge_layer(layer);

        assert!(base.get(b"dir/secret.txt").is_none());
        // The parent directory should still exist.
        assert!(base.get(b"dir").is_some());
    }

    #[test]
    fn merge_layer_opaque_dir_clears_existing_entries() {
        let mut base = FileTree::new();
        base.insert(b"dir/a.txt", make_regular_file(b"a")).unwrap();
        base.insert(b"dir/b.txt", make_regular_file(b"b")).unwrap();

        let mut layer = FileTree::new();
        let mut opaque_dir = make_opaque_directory();
        opaque_dir
            .entries
            .insert(OsString::from("c.txt"), make_regular_file(b"c"));
        layer
            .root
            .entries
            .insert(OsString::from("dir"), TreeNode::Directory(opaque_dir));

        base.merge_layer(layer);

        // Old entries should be gone.
        assert!(base.get(b"dir/a.txt").is_none());
        assert!(base.get(b"dir/b.txt").is_none());
        // New entry should be present.
        match base.get(b"dir/c.txt").unwrap() {
            TreeNode::RegularFile(f) => assert_eq!(f.data, FileData::Memory(b"c".to_vec())),
            _ => panic!("expected regular file"),
        }
    }

    #[test]
    fn node_count_and_data_size() {
        let mut tree = FileTree::new();
        tree.insert(b"a/file1.txt", make_regular_file(b"hello"))
            .unwrap();
        tree.insert(b"a/file2.txt", make_regular_file(b"world!"))
            .unwrap();
        tree.insert(b"b/nested/file3.txt", make_regular_file(b"!"))
            .unwrap();

        // a, a/file1.txt, a/file2.txt, b, b/nested, b/nested/file3.txt = 6
        assert_eq!(tree.node_count(), 6);
        // 5 + 6 + 1 = 12
        assert_eq!(tree.total_data_size(), 12);
    }

    #[test]
    fn data_size_counts_hardlinked_regular_file_once() {
        let mut tree = FileTree::new();
        let file_id = RegularFileId::new();

        tree.insert(b"a.txt", make_regular_file_with_id(b"shared", file_id))
            .unwrap();
        tree.insert(b"b.txt", make_regular_file_with_id(b"shared", file_id))
            .unwrap();

        tree.refresh_regular_nlinks();

        assert_eq!(tree.total_data_size(), b"shared".len() as u64);
        for path in [b"a.txt".as_slice(), b"b.txt".as_slice()] {
            match tree.get(path).unwrap() {
                TreeNode::RegularFile(file) => assert_eq!(file.nlink, 2),
                _ => panic!("expected regular file"),
            }
        }
    }

    #[test]
    fn remove_node() {
        let mut tree = FileTree::new();
        tree.insert(b"a/b.txt", make_regular_file(b"data")).unwrap();
        assert!(tree.get(b"a/b.txt").is_some());

        let removed = tree.remove(b"a/b.txt");
        assert!(removed.is_some());
        assert!(tree.get(b"a/b.txt").is_none());
    }

    #[test]
    fn empty_path_is_rejected() {
        let mut tree = FileTree::new();
        let result = tree.insert(b"", make_regular_file(b"data"));
        assert!(matches!(result, Err(FileTreeError::PathEmpty)));
    }

    #[test]
    fn not_a_directory_error() {
        let mut tree = FileTree::new();
        tree.insert(b"a", make_regular_file(b"file")).unwrap();

        let result = tree.insert(b"a/b", make_regular_file(b"nested"));
        assert!(matches!(result, Err(FileTreeError::NotADirectory(_))));
    }

    #[test]
    fn resource_limits_default() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_total_size, 10 * 1024 * 1024 * 1024);
        assert_eq!(limits.max_file_size, 5 * 1024 * 1024 * 1024);
        assert_eq!(limits.max_entry_count, 1_000_000);
        assert_eq!(limits.max_path_length, 4096);
        assert_eq!(limits.max_path_depth, 128);
        assert_eq!(limits.max_symlink_target, 4096);
    }
}
