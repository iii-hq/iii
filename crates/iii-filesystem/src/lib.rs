//! `iii-filesystem` provides filesystem backends for iii worker VM sandboxes.
//!
//! The primary backend is `PassthroughFs`, which exposes a host directory to the
//! guest VM via virtio-fs with optional init binary injection.

pub mod backends;
pub mod init;

// Re-export DynFileSystem types from msb_krun (per D-08)
pub use msb_krun::backends::fs::{
    Context, DirEntry, DynFileSystem, Entry, Extensions, FsOptions, GetxattrReply, ListxattrReply,
    OpenOptions, RemovemappingOne, SetattrValid, ZeroCopyReader, ZeroCopyWriter, stat64, statvfs64,
};

// Re-export PassthroughFs backend types
pub use backends::passthroughfs::{
    CachePolicy, PassthroughConfig, PassthroughFs, PassthroughFsBuilder,
};
