// Copyright 2018 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Wrapper for configuring the Block devices attached to the microVM.
#[cfg(feature = "blk")]
pub mod block;

/// Wrapper for configuring the firmware.
pub mod firmware;

/// Wrapper for configuring the kernel command line.
pub mod kernel_cmdline;

/// Wrapper for configuring an external kernel to be loaded in the microVM.
pub mod external_kernel;

/// Wrapper for configuring the Fs devices attached to the microVM.
#[cfg(not(feature = "tee"))]
pub mod fs;

/// Wrapper over the microVM general information attached to the microVM.
pub mod instance_info;

/// Wrapper for configuring the kernel bundle to be loaded in the microVM.
pub mod kernel_bundle;

/// Wrapper for configuring the memory and CPU of the microVM.
pub mod machine_config;

/// Wrapper for configuring the vsock devices attached to the microVM.
pub mod vsock;

/// Wrapper for configuring the network devices attached to the microVM.
#[cfg(feature = "net")]
pub mod net;
