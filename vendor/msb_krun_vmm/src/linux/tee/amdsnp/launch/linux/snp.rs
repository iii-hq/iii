// SPDX-License-Identifier: Apache-2.0

//! Types for interacting with the KVM SEV-SNP guest management API.

use crate::linux::tee::amdsnp::*;

use std::marker::PhantomData;

/// Structure passed into KVM_SEV_INIT2 command.
#[derive(Default)]
#[repr(C, packed)]
pub struct Init2 {
    /// Initial value of features field in VMSA. (Must be 0 for SEV)
    vmsa_features: u64,

    /// Always set to 0
    flags: u32,

    /// Maximum guest GHCB version allowed. (Currently 0 for SEV and 1 for SEV-ES and SEV-SNP)
    ghcb_version: u16,

    pad1: u16,

    pad2: [u32; 8],
}

impl Init2 {
    /// Default INIT2 values for SEV-SNP
    pub fn init_default_snp() -> Self {
        Self {
            vmsa_features: 0,
            flags: 0,
            ghcb_version: 2,
            pad1: Default::default(),
            pad2: Default::default(),
        }
    }
}

#[repr(C)]
pub struct LaunchStart {
    /// Guest policy. See Table 7 of the AMD SEV-SNP Firmware
    /// specification for a description of the guest policy structure.
    policy: u64,

    /// Hypervisor provided value to indicate guest OS visible workarounds.
    /// The format is hypervisor defined.
    gosvw: [u8; 16],

    flags: u16,

    pad0: [u8; 6],

    pad1: [u64; 4],
}

impl From<Start> for LaunchStart {
    fn from(start: Start) -> Self {
        Self {
            policy: start.policy.into(),
            gosvw: start.gosvw,
            flags: 0,
            pad0: [0u8; 6],
            pad1: [0u64; 4],
        }
    }
}

/// Insert pages into the guest physical address space.
#[repr(C)]
pub struct LaunchUpdate<'a> {
    /// guest start frame number.
    pub start_gfn: u64,

    /// Userspace address of the page needed to be encrypted.
    pub uaddr: u64,

    /// Length of the page needed to be encrypted:
    /// (end encryption uaddr = uaddr + len).
    pub len: u64,

    /// Encoded page type. See Table 58 if the SNP Firmware specification.
    pub page_type: u8,

    pad0: u8,

    flags: u16,

    pad1: u32,

    pad2: [u64; 4],

    _phantom: PhantomData<&'a [u8]>,
}

impl From<Update<'_>> for LaunchUpdate<'_> {
    fn from(update: Update) -> Self {
        Self {
            start_gfn: update.start_gfn,
            uaddr: update.uaddr.as_ptr() as _,
            len: update.uaddr.len() as _,
            page_type: update.page_type as _,
            pad0: 0,
            flags: 0,
            pad1: 0,
            pad2: [0u64; 4],
            _phantom: PhantomData,
        }
    }
}

pub const KVM_SEV_SNP_FINISH_DATA_SIZE: usize = 32;

/// Complete the guest launch flow.
#[repr(C)]
pub struct LaunchFinish<'a> {
    /// Userspace address of the ID block. Ignored if ID_BLOCK_EN is 0.
    id_block_uaddr: u64,

    /// Userspace address of the authentication information of the ID block. Ignored if ID_BLOCK_EN is 0.
    id_auth_uaddr: u64,

    /// Indicates that the ID block is present.
    id_block_en: u8,

    /// Indicates that the author key is present in the ID authentication information structure.
    /// Ignored if ID_BLOCK_EN is 0.
    auth_key_en: u8,

    /// Opaque host-supplied data to describe the guest. The firmware does not interpret this value.
    host_data: [u8; KVM_SEV_SNP_FINISH_DATA_SIZE],

    pad: [u8; 6],

    _phantom: PhantomData<&'a [u8]>,
}

impl From<Finish<'_, '_>> for LaunchFinish<'_> {
    fn from(finish: Finish) -> Self {
        let id_block = if let Some(addr) = finish.id_block {
            addr.as_ptr() as u64
        } else {
            0
        };

        let id_auth = if let Some(addr) = finish.id_auth {
            addr.as_ptr() as u64
        } else {
            0
        };

        Self {
            id_block_uaddr: id_block,
            id_auth_uaddr: id_auth,
            id_block_en: u8::from(finish.id_block.is_some()),
            auth_key_en: u8::from(finish.id_auth.is_some()),
            host_data: finish.host_data,
            pad: [0u8; 6],
            _phantom: PhantomData,
        }
    }
}
