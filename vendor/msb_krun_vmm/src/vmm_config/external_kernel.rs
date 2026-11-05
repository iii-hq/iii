// Copyright 2024, Red Hat Inc. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub enum KernelFormat {
    // Raw image, ready to be loaded into the VM.
    #[default]
    Raw,
    // ELF image, need to locale sections be loaded.
    Elf,
    // Raw image compressed with GZIP, embedded into a PE file.
    PeGz,
    // ELF image compressed with BZIP2, embedded into an Image file.
    ImageBz2,
    // ELF image compressed with GZIP, embedded into an Image file.
    ImageGz,
    // ELF image compressed with ZSTD, embedded into an Image file.
    ImageZstd,
}

/// Data structure holding the attributes read from the `libkrunfw` kernel config.
#[derive(Clone, Debug, Default)]
pub struct ExternalKernel {
    pub path: PathBuf,
    pub format: KernelFormat,
    pub initramfs_path: Option<PathBuf>,
    pub initramfs_size: u64,
    pub cmdline: Option<String>,
}
