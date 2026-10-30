// SPDX-License-Identifier: Apache-2.0

//! Everything one needs to launch an AMD SEV encrypted virtual machine.
//!
//! This module contains types for establishing a secure channel with the
//! AMD Secure Processor for purposes of attestation as well as abstractions
//! for navigating the AMD SEV launch process for a virtual machine.

#[cfg(target_os = "linux")]
mod linux;

pub mod error;
pub mod firmware;
pub(crate) mod util;

use super::error::FirmwareError;

#[cfg(target_os = "linux")]
use linux::{ioctl::*, snp::*};

use std::{fmt::Display, marker::PhantomData, os::unix::io::AsRawFd, result::Result};

use bitfield::bitfield;
use bitflags::bitflags;

/// Launcher type-state that indicates a brand new launch.
pub struct New;

/// Launcher type-state that indicates a SNP in-progress.
pub struct Started;

/// Facilitates the correct execution of the SEV launch process.
pub struct Launcher<T, U: AsRawFd, V: AsRawFd> {
    vm_fd: U,
    sev: V,
    state: PhantomData<T>,
}

impl<T, U: AsRawFd, V: AsRawFd> AsRef<U> for Launcher<T, U, V> {
    /// Give access to the vm fd to create vCPUs or such.
    fn as_ref(&self) -> &U {
        &self.vm_fd
    }
}

impl<T, U: AsRawFd, V: AsRawFd> AsMut<U> for Launcher<T, U, V> {
    /// Give access to the vm fd to create vCPUs or such.
    fn as_mut(&mut self) -> &mut U {
        &mut self.vm_fd
    }
}

impl<U: AsRawFd, V: AsRawFd> Launcher<New, U, V> {
    /// Begin the SEV-SNP launch process by creating a Launcher and issuing the
    /// KVM_SNP_INIT ioctl.
    pub fn new(vm_fd: U, sev: V) -> Result<Self, FirmwareError> {
        let mut launcher = Launcher {
            vm_fd,
            sev,
            state: PhantomData,
        };

        let init = Init2::init_default_snp();

        let mut cmd = Command::from(&launcher.sev, &init);

        INIT2
            .ioctl(&mut launcher.vm_fd, &mut cmd)
            .map_err(|_| cmd.encapsulate())?;

        Ok(launcher)
    }

    /// Initialize the flow to launch a guest.
    pub fn start(mut self, start: Start) -> Result<Launcher<Started, U, V>, FirmwareError> {
        let launch_start = LaunchStart::from(start);
        let mut cmd = Command::from(&self.sev, &launch_start);

        SNP_LAUNCH_START
            .ioctl(&mut self.vm_fd, &mut cmd)
            .map_err(|_| cmd.encapsulate())?;

        let launcher = Launcher {
            vm_fd: self.vm_fd,
            sev: self.sev,
            state: PhantomData,
        };

        Ok(launcher)
    }
}

impl<U: AsRawFd, V: AsRawFd> Launcher<Started, U, V> {
    /// Encrypt guest SNP data.
    pub fn update_data(
        &mut self,
        mut update: Update,
        gpa: u64,
        gpa_len: u64,
    ) -> Result<(), FirmwareError> {
        loop {
            let launch_update_data = LaunchUpdate::from(update);
            let mut cmd = Command::from(&self.sev, &launch_update_data);

            // Register the encryption region
            KvmEncRegion::new(update.uaddr).register(&mut self.vm_fd)?;

            // Set memory attributes to private
            KvmSetMemoryAttributes::new(gpa, gpa_len, KVM_MEMORY_ATTRIBUTE_PRIVATE)
                .set_attributes(&mut self.vm_fd)?;

            // Perform the SNP_LAUNCH_UPDATE ioctl call
            match SNP_LAUNCH_UPDATE.ioctl(&mut self.vm_fd, &mut cmd) {
                Ok(_) => {
                    // Check if the entire range has been processed
                    if launch_update_data.len == 0 {
                        break;
                    }

                    // Update the `update` object with the remaining range
                    update.start_gfn = launch_update_data.start_gfn;
                    update.uaddr = unsafe {
                        std::slice::from_raw_parts(
                            launch_update_data.uaddr as *const u8,
                            launch_update_data.len as usize,
                        )
                    };
                }
                Err(e) if e.raw_os_error() == Some(libc::EAGAIN) => {
                    // Retry the operation if `-EAGAIN` is returned
                    continue;
                }
                Err(_) => {
                    // Handle other errors
                    return Err(cmd.encapsulate());
                }
            }
        }

        Ok(())
    }

    /// Complete the SNP launch process.
    pub fn finish(mut self, finish: Finish) -> Result<(U, V), FirmwareError> {
        let launch_finish = LaunchFinish::from(finish);
        let mut cmd = Command::from(&self.sev, &launch_finish);

        SNP_LAUNCH_FINISH
            .ioctl(&mut self.vm_fd, &mut cmd)
            .map_err(|_| cmd.encapsulate())?;

        Ok((self.vm_fd, self.sev))
    }
}

/// Encapsulates the various data needed to begin the launch process.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Start {
    /// Describes a policy that the AMD Secure Processor will enforce.
    pub(crate) policy: GuestPolicy,

    /// Hypervisor provided value to indicate guest OS visible workarounds.The format is hypervisor defined.
    pub(crate) gosvw: [u8; 16],

    /// Indicates that this launch flow is launching an IMI for the purpose of guest-assisted migration.
    pub(crate) flags: u16,
}

impl Start {
    /// Encapsulate all data needed for the SNP_LAUNCH_START ioctl.
    pub fn new(policy: GuestPolicy, gosvw: [u8; 16]) -> Self {
        Self {
            policy,
            gosvw,
            flags: 0,
        }
    }
}

/// Encoded page types for a launch update. See Table 58 of the SNP Firmware
/// specification for further details.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
#[non_exhaustive]
pub enum PageType {
    /// A normal data page.
    Normal = 0x1,

    /// A VMSA page.
    Vmsa = 0x2,

    /// A page full of zeroes.
    Zero = 0x3,

    /// A page that is encrypted but not measured
    Unmeasured = 0x4,

    /// A page for the firmware to store secrets for the guest.
    Secrets = 0x5,

    /// A page for the hypervisor to provide CPUID function values.
    Cpuid = 0x6,
}

/// Encapsulates the various data needed to begin the update process.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Update<'a> {
    /// guest start frame number.
    pub(crate) start_gfn: u64,

    /// The userspace of address of the encrypted region.
    pub(crate) uaddr: &'a [u8],

    /// Encoded page type.
    pub(crate) page_type: PageType,
}

impl<'a> Update<'a> {
    /// Encapsulate all data needed for the SNP_LAUNCH_UPDATE ioctl.
    pub fn new(start_gfn: u64, uaddr: &'a [u8], page_type: PageType) -> Self {
        Self {
            start_gfn,
            uaddr,
            page_type,
        }
    }
}

bitflags! {
    #[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
    /// VMPL permission masks.
    pub struct VmplPerms: u8 {
        /// Page is readable by the VMPL.
        const READ = 1;

        /// Page is writeable by the VMPL.
        const WRITE = 1 << 1;

        /// Page is executable by the VMPL in CPL3.
        const EXECUTE_USER = 1 << 2;

        /// Page is executable by the VMPL in CPL2, CPL1, and CPL0.
        const EXECUTE_SUPERVISOR = 1 << 3;
    }
}

/// Encapsulates the data needed to complete a guest launch.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Finish<'a, 'b> {
    /// The userspace address of the encrypted region.
    pub(crate) id_block: Option<&'a [u8]>,

    /// The userspace address of the authentication information of the ID block.
    pub(crate) id_auth: Option<&'b [u8]>,

    /// Opaque host-supplied data to describe the guest. The firmware does not interpret this
    /// value.
    pub(crate) host_data: [u8; KVM_SEV_SNP_FINISH_DATA_SIZE],
}

impl<'a, 'b> Finish<'a, 'b> {
    /// Encapsulate all data needed for the SNP_LAUNCH_FINISH ioctl.
    pub fn new(
        id_block: Option<&'a [u8]>,
        id_auth: Option<&'b [u8]>,
        host_data: [u8; KVM_SEV_SNP_FINISH_DATA_SIZE],
    ) -> Self {
        Self {
            id_block,
            id_auth,
            host_data,
        }
    }
}

bitfield! {
    /// The firmware associates each guest with a guest policy that the guest owner provides. The
    /// firmware restricts what actions the hypervisor can take on this guest according to the guest policy.
    /// The policy also indicates the minimum firmware version to for the guest.
    ///
    /// The guest owner provides the guest policy to the firmware during launch. The firmware then binds
    /// the policy to the guest. The policy cannot be changed throughout the lifetime of the guest. The
    /// policy is also migrated with the guest and enforced by the destination platform firmware.
    ///
    /// | Bit(s) | Name              | Description                                                                                                        >
    /// |--------|-------------------|-------------------------------------------------------------------------------------------------------------------->
    /// | 7:0    | ABI_MINOR         | The minimum ABI minor version required for this guest to run.                                                      >
    /// | 15:8   | ABI_MAJOR         | The minimum ABI major version required for this guest to run.                                                      >
    /// | 16     | SMT               | 0: Host SMT usage is disallowed.<br>1: Host SMT usage is allowed.                                                  >
    /// | 17     | -                 | Reserved. Must be one.                                                                                             >
    /// | 18     | MIGRATE_MA        | 0: Association with a migration agent is disallowed.<br>1: Association with a migration agent is allowed           >
    /// | 19     | DEBUG             | 0: Debugging is disallowed.<br>1: Debugging is allowed.                                                            >
    /// | 20     | SINGLE_SOCKET     | 0: Guest can be activated on multiple sockets.<br>1: Guest can only be activated on one socket.                    >
    /// | 21     | CXL_ALLOW         | 0: CXL cannot be populated with devices or memory.<br>1: CXL can be populated with devices or memory.              >
    /// | 22     | MEM_AES_256_XTS   | 0: Allow either AES 128 XEX or AES 256 XTS for memory encryption.<br>1: Require AES 256 XTS for memory encryption. >
    /// | 23     | RAPL_DIS          | 0: Allow Running Average Power Limit (RAPL).<br>1: RAPL must be disabled.                                          >
    /// | 24     | CIPHERTEXT_HIDING | 0: Ciphertext hiding may be enabled or disabled.<br>1: Ciphertext hiding must be enabled.                          >
    /// | 25     | PAGE_SWAP_DISABLE | 0: Disable Guest access to SNP_PAGE_MOVE, SNP_SWAP_OUT and SNP_SWAP_IN commands.                                   >
    /// | 63:25  | -                 | Reserved. MBZ.                                                                                                     >
    ///
    #[repr(C)]
    #[derive(Default, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
    pub struct GuestPolicy(u64);
    impl Debug;
    /// ABI_MINOR field: Indicates the minor API version.
    pub abi_minor, set_abi_minor: 7, 0;
    /// ABI_MAJOR field: Indicates the minor API version.
    pub abi_major, set_abi_major: 15, 8;
    /// SMT_ALLOWED field: Indicates the if SMT should be permitted.
    pub smt_allowed, set_smt_allowed: 16;
    /// MIGRATE_MA_ALLOWED field: Indicates the if migration is permitted with
    /// the migration agent.
    pub migrate_ma_allowed, set_migrate_ma_allowed: 18;
    /// DEBUG_ALLOWED field: Indicates the if debugging should is permitted.
    pub debug_allowed, set_debug_allowed: 19;
    /// SINGLE_SOCKET_REQUIRED field: Indicates the if a single socket is required.
    pub single_socket_required, set_single_socket_required: 20;
    /// CXL_ALLOW field: (1) can populate CXL devices/memory, (0) cannot populate CXL devices/memory
    pub cxl_allowed, set_cxl_allowed: 21;
    /// MEM_AES_256_XTS field: (1) require AES 256 XTS encryption, (0) allows either AES 128 XEX or AES 256 XTS encryption
    pub mem_aes_256_xts, set_mem_aes_256_xts: 22;
    /// RAPL_DIS field: (1) RAPL must be disabled, (0) allow RAPL
    pub rapl_dis, set_rapl_dis: 23;
    /// CIPHERTEXT_HIDING field: (1) ciphertext hiding must be enabled, (0) ciphertext hiding may be enabled/disabled
    pub ciphertext_hiding, set_ciphertext_hiding: 24;
    /// Guest policy to disable Guest access to SNP_PAGE_MOVE, SNP_SWAP_OUT, and SNP_SWAP_IN commands. If this policy
    /// option is selected to disable these Page Move commands, then these commands will return POLICY_FAILURE.
    /// 0: Do not disable Guest support for the commands.
    /// 1: Disable Guest support for the commands.
    pub page_swap_disabled, set_page_swap_disabled: 25;
}

impl Display for GuestPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#"Guest Policy (0x{:x}):
  ABI Major:     {}
  ABI Minor:     {}
  SMT Allowed:   {}
  Migrate MA:    {}
  Debug Allowed: {}
  Single Socket: {}
  CXL Allowed:   {}
  AEX 256 XTS:   {}
  RAPL Allowed:  {}
  Ciphertext hiding: {}
  Page Swap Disable: {}"#,
            self.0,
            self.abi_major(),
            self.abi_minor(),
            self.smt_allowed(),
            self.migrate_ma_allowed(),
            self.debug_allowed(),
            self.single_socket_required(),
            self.cxl_allowed(),
            self.mem_aes_256_xts(),
            self.rapl_dis(),
            self.ciphertext_hiding(),
            self.page_swap_disabled()
        )
    }
}

impl From<GuestPolicy> for u64 {
    fn from(value: GuestPolicy) -> Self {
        // Bit 17 of the guest policy is reserved and must always be set to 1.
        let reserved: u64 = 1 << 17;

        value.0 | reserved
    }
}

impl From<u64> for GuestPolicy {
    fn from(value: u64) -> Self {
        // Bit 17 of the guest policy is reserved and must always be set to 1.
        let reserved: u64 = 1 << 17;

        GuestPolicy(value | reserved)
    }
}
