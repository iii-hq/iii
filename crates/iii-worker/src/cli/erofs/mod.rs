// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Host-side erofs base-image builder for the shared read-only overlay lower.
//!
//! Why erofs and not squashfs: the libkrunfw guest kernel ships erofs (with lz4
//! decompression) on every arch, but `CONFIG_SQUASHFS` is compiled in only on
//! aarch64 — on x86_64 a `mount(2)` of a squashfs lower returns ENODEV and the
//! VM never boots. erofs is the one read-only filesystem the guest can mount
//! everywhere, so it is the lower for the shared-rootfs overlay model.
//!
//! The `format`, `tree`, `crc32c`, `writer`, and `reader` submodules are a
//! pure-Rust, std-only erofs implementation whose on-disk layout matches what
//! the libkrunfw guest kernel mounts. The writer emits UNCOMPRESSED erofs
//! (FLAT_PLAIN / FLAT_INLINE), which the guest mounts with no decompressor;
//! `builder` adds the extracted-rootfs-directory → image packing iii needs.

// These modules carry some capability iii does not exercise (layer merge, full
// reader walk); kept intact rather than trimmed to a minimal subset.
#[allow(dead_code)]
pub(crate) mod crc32c;
#[allow(dead_code)]
pub(crate) mod format;
#[allow(dead_code)]
pub(crate) mod reader;
#[allow(dead_code)]
pub(crate) mod tree;
#[allow(dead_code)]
pub(crate) mod writer;

mod builder;

pub use builder::{base_erofs_path, build_erofs, ensure_base_erofs, remove_base_erofs};
#[cfg(test)]
pub(crate) use reader::ErofsReader;
pub(crate) use writer::write_erofs;
