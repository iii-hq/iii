// Copyright 2018 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
//
// Portions Copyright 2017 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the THIRD-PARTY file.

/// Legacy Device Manager.
pub mod legacy;

/// Device Shared Memory Region Manager.
pub mod shm;

/// Memory Mapped I/O Manager.
#[cfg(target_os = "linux")]
pub mod kvm;
#[cfg(target_os = "linux")]
pub use self::kvm::mmio;
#[cfg(target_os = "macos")]
pub mod hvf;
#[cfg(target_os = "macos")]
pub use self::hvf::mmio;
