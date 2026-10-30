// Copyright 2020, Red Hat Inc. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter, Result};

/// Data structure holding the attributes read from the `libkrunfw` kernel config.
#[derive(Debug, Default)]
pub struct KernelBundle {
    pub host_addr: u64,
    pub guest_addr: u64,
    pub entry_addr: u64,
    pub size: usize,
}

/// Structure used to specify the parameters for the `libkrunfw` kernel bundle.
#[derive(Debug)]
pub enum KernelBundleError {
    /// Guest address is not page-aligned.
    InvalidGuestAddress,
    /// Host address is zero or not page-aligned.
    InvalidHostAddress,
    /// Kernel size is zero or not a multiple of the page size.
    InvalidSize,
}

impl Display for KernelBundleError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        use self::KernelBundleError::*;
        match *self {
            InvalidGuestAddress => write!(f, "Guest address is not page-aligned"),
            InvalidHostAddress => write!(f, "Host address is zero or not page-aligned"),
            InvalidSize => write!(f, "Kernel size is zero or not a multiple of the page size"),
        }
    }
}

/// Data structure holding the attributes read from the `libkrunfw` qboot config.
#[derive(Debug, Default)]
pub struct QbootBundle {
    pub host_addr: u64,
    pub size: usize,
}

/// Structure used to specify the parameters for the `libkrunfw` qboot bundle.
#[derive(Debug)]
pub enum QbootBundleError {
    /// Qboot binary is not 64K long.
    InvalidSize,
}

impl Display for QbootBundleError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        use self::QbootBundleError::*;
        match *self {
            InvalidSize => write!(f, "qboot binary is not 64K long."),
        }
    }
}

/// Data structure holding the attributes read from the `libkrunfw` initrd config.
#[derive(Debug, Default)]
pub struct InitrdBundle {
    pub host_addr: u64,
    pub size: usize,
}
