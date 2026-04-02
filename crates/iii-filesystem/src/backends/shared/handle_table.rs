//! Handle table for open file descriptors.

use std::{fs::File, sync::RwLock};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Data associated with an open file handle.
///
/// Wrapped in `RwLock` because `preadv64`/`pwritev64` only need a shared
/// lock (they take an explicit offset), while `lseek`, `fsync`, and
/// `ftruncate` need exclusive access.
pub(crate) struct HandleData {
    pub file: RwLock<File>,
}
