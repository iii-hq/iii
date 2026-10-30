pub(crate) mod launch;

use std::{
    os::unix::io::{AsRawFd, RawFd},
    slice,
};

use crate::vstate::MeasuredRegion;
use arch::x86_64::layout::*;

use launch::{error::FirmwareError, firmware::Firmware, *};

use kvm_bindings::{kvm_enc_region, CpuId, KVM_CPUID_FLAG_SIGNIFCANT_INDEX};
use kvm_ioctls::VmFd;
use vm_memory::{
    Bytes, GuestAddress, GuestMemory, GuestMemoryMmap, GuestMemoryRegion, GuestRegionMmap,
};

#[derive(Debug)]
pub enum Error {
    CpuIdWrite,
    CpuIdFull,
    CreateLauncher(FirmwareError),
    GuestMemoryWrite(vm_memory::GuestMemoryError),
    GuestMemoryRead(vm_memory::GuestMemoryError),
    LaunchStart(FirmwareError),
    LaunchUpdate(FirmwareError),
    LaunchFinish(FirmwareError),
    MemoryEncryptRegion,
    OpenFirmware(std::io::Error),
}

const COUNT_MAX: usize = 80;

fn as_u32_le(array: &[u8; 4]) -> u32 {
    (array[0] as u32)
        + ((array[1] as u32) << 8)
        + ((array[2] as u32) << 16)
        + ((array[3] as u32) << 24)
}

/// An entry in the SNP CPUID Page
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct CpuidFunctionEntry {
    /// function
    pub eax_in: u32,
    /// index
    pub ecx_in: u32,
    /// register state when cpuid is called
    pub xcr0_in: u64,
    /// register state when cpuid is called
    pub xss_in: u64,
    /// cpuid out
    pub eax: u32,
    /// cpuid out
    pub ebx: u32,
    /// cpuid out
    pub ecx: u32,
    /// cpuid out
    pub edx: u32,
    reserved: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct CpuidPageEntry {
    count: u32,
    reserved_1: u32,
    reserved_2: u64,
    functions: [CpuidFunctionEntry; COUNT_MAX],
}

pub struct AmdSnp {
    fw: Firmware,
}

impl AmdSnp {
    pub fn new() -> Result<Self, Error> {
        let fw = Firmware::open().map_err(Error::OpenFirmware)?;

        Ok(AmdSnp { fw })
    }

    pub fn vm_prepare(
        &self,
        vm_fd: &VmFd,
        guest_mem: &GuestMemoryMmap,
    ) -> Result<Launcher<Started, RawFd, RawFd>, Error> {
        let vm_rfd = vm_fd.as_raw_fd();
        let fw_rfd = self.fw.as_raw_fd();

        let launcher = Launcher::new(vm_rfd, fw_rfd).map_err(Error::CreateLauncher)?;

        for region in guest_mem.iter() {
            // It's safe to unwrap because the guest address is valid.
            let host_addr = guest_mem.get_host_address(region.start_addr()).unwrap();
            let enc_region = kvm_enc_region {
                addr: host_addr as u64,
                size: region.len(),
            };

            vm_fd
                .register_enc_memory_region(&enc_region)
                .map_err(|_| Error::MemoryEncryptRegion)?;
        }

        let mut policy = GuestPolicy(0);
        policy.set_smt_allowed(true);

        let start = Start::new(policy, [0; 16]);

        let launcher = launcher.start(start).map_err(Error::LaunchStart)?;

        Ok(launcher)
    }

    fn write_cpuid_page(
        &self,
        cpuid: CpuId,
        guest_mem: &GuestMemoryMmap,
    ) -> Result<CpuidPageEntry, Error> {
        let mut cpuid_entry = CpuidPageEntry {
            count: 0,
            reserved_1: 0,
            reserved_2: 0,
            functions: [CpuidFunctionEntry::default(); COUNT_MAX],
        };

        for (i, kvm_entry) in cpuid.as_slice().iter().enumerate() {
            // GET_CPUID2 returns bogus entries at the end with all zero set
            if kvm_entry.function == 0 && kvm_entry.index == 0 && i != 0 {
                continue;
            }

            if kvm_entry.function == 0xFFFFFFFF {
                break;
            }

            // range check, see:
            // SEV Secure Nested Paging Firmware ABI Specification
            // 8.14.2.6 PAGE_TYPE_CPUID
            if !((0..0xFFFF).contains(&kvm_entry.function)
                || (0x8000_0000..0x8000_FFFF).contains(&kvm_entry.function))
            {
                continue;
            }

            let mut snp_cpuid_entry = CpuidFunctionEntry {
                eax_in: kvm_entry.function,
                ecx_in: {
                    if (kvm_entry.flags & KVM_CPUID_FLAG_SIGNIFCANT_INDEX) != 0 {
                        kvm_entry.index
                    } else {
                        0
                    }
                },
                xcr0_in: 0,
                xss_in: 0,
                eax: kvm_entry.eax,
                ebx: kvm_entry.ebx,
                ecx: kvm_entry.ecx,
                edx: kvm_entry.edx,
                ..Default::default()
            };

            // Expose HYPERVISOR.
            if snp_cpuid_entry.eax_in == 0x1 {
                snp_cpuid_entry.ecx |= 1 << 31;
            }

            // Disable extended features, not supported by SNP guests.
            if snp_cpuid_entry.eax_in == 0x7 {
                snp_cpuid_entry.ebx &= !(1 << 1);
                snp_cpuid_entry.edx = 0;
            }

            // Disable virt_ssbd, not supported by SNP guests.
            if snp_cpuid_entry.eax_in == 0x8000_0008 {
                snp_cpuid_entry.ebx &= !(1 << 25);
            }

            // Fix XSAVE entry.
            if snp_cpuid_entry.eax_in == 0xD {
                if snp_cpuid_entry.ecx_in == 0x1 {
                    snp_cpuid_entry.xcr0_in = 0x1;
                }
                if snp_cpuid_entry.ecx_in == 0x0 || snp_cpuid_entry.ecx_in == 0x1 {
                    snp_cpuid_entry.ebx = 576;
                }
            }

            // Indicate the guest is running with SNP enabled.
            if snp_cpuid_entry.eax_in == 0x8000_001F {
                snp_cpuid_entry.eax = 0x1a;
                snp_cpuid_entry.ebx = 51 | (1 << 6);
                snp_cpuid_entry.ecx = 0;
                snp_cpuid_entry.edx = 0;
            }

            if cpuid_entry.count as usize >= COUNT_MAX {
                return Err(Error::CpuIdFull);
            }

            cpuid_entry.functions[cpuid_entry.count as usize] = snp_cpuid_entry;
            cpuid_entry.count += 1;
        }

        // Expose the KVM hypervisor signature.
        let snp_cpuid_entry = CpuidFunctionEntry {
            eax_in: 0x40000000,
            ecx_in: 0,
            xcr0_in: 0,
            xss_in: 0,
            eax: 0x40000001,
            ebx: as_u32_le(b"KVMK"),
            ecx: as_u32_le(b"VMKV"),
            edx: as_u32_le(b"M\0\0\0"),
            ..Default::default()
        };

        cpuid_entry.functions[cpuid_entry.count as usize] = snp_cpuid_entry;
        cpuid_entry.count += 1;

        // Expose the KVM hypervisor flags.
        let snp_cpuid_entry = CpuidFunctionEntry {
            eax_in: 0x40000001,
            ecx_in: 0,
            xcr0_in: 0,
            xss_in: 0,
            eax: 0xff,
            ebx: 0,
            ecx: 0,
            edx: 0,
            ..Default::default()
        };

        cpuid_entry.functions[cpuid_entry.count as usize] = snp_cpuid_entry;
        cpuid_entry.count += 1;

        let data = unsafe {
            std::slice::from_raw_parts(
                &cpuid_entry as *const _ as *const u8,
                std::mem::size_of::<CpuidPageEntry>(),
            )
        };
        guest_mem
            .write(data, GuestAddress(0x6000))
            .map_err(Error::GuestMemoryWrite)?;

        Ok(cpuid_entry)
    }

    fn check_cpuid_page(
        &self,
        guest_mem: &GuestMemoryMmap,
        old_cpuid: CpuidPageEntry,
    ) -> Result<(), Error> {
        let mut data: [u8; 4096] = [0; 4096];
        guest_mem
            .read(&mut data, GuestAddress(0x6000))
            .map_err(Error::GuestMemoryRead)?;

        let new_cpuid_p = data.as_ptr() as *const CpuidPageEntry;
        let new_cpuid = unsafe { *new_cpuid_p };

        for (i, entry) in old_cpuid.functions.iter().enumerate() {
            if *entry != new_cpuid.functions[i] {
                debug!("cpuid entry: {i} differs");
                debug!("provided {entry:?}");
                debug!("expected: {:?}", new_cpuid.functions[i]);
            }
        }

        Ok(())
    }

    fn add_region(
        &self,
        guest_mem: &GuestMemoryMmap,
        region: MeasuredRegion,
        launcher: &mut Launcher<Started, RawFd, RawFd>,
        page_type: PageType,
    ) -> Result<(), Error> {
        let ga = GuestAddress(region.guest_addr);

        /*
         * Use the guest's address to obtain its GuestRegionMmap, and then
         * convert this region to a slice. Basically, we are taking an
         * entire slice of a guest memory region.
         */
        let gr: &GuestRegionMmap = guest_mem.find_region(ga).unwrap();

        let region_addr = gr.to_region_addr(ga).unwrap();
        let bytes = gr.get_slice(region_addr, region.size).unwrap();
        let ptr = bytes.ptr_guard().as_ptr();
        let slice: &[u8] = unsafe { slice::from_raw_parts(ptr, region.size) };

        let update = Update::new(region.guest_addr >> 12, slice, page_type);

        launcher
            .update_data(update, region.guest_addr, region.size as u64)
            .map_err(Error::LaunchUpdate)
    }

    pub fn vm_measure(
        &self,
        cpuid: CpuId,
        guest_mem: &GuestMemoryMmap,
        measured_regions: Vec<MeasuredRegion>,
        mut launcher: Launcher<Started, RawFd, RawFd>,
    ) -> Result<(), Error> {
        for region in measured_regions {
            self.add_region(guest_mem, region, &mut launcher, PageType::Normal)?;
        }

        // Inital LIDT
        self.add_region(
            guest_mem,
            MeasuredRegion {
                guest_addr: SNP_LIDT_START,
                host_addr: guest_mem
                    .get_host_address(GuestAddress(SNP_LIDT_START))
                    .unwrap() as u64,
                size: 0x1000,
            },
            &mut launcher,
            PageType::Zero,
        )?;

        // Secrets page
        self.add_region(
            guest_mem,
            MeasuredRegion {
                guest_addr: SNP_SECRETS_START,
                host_addr: guest_mem
                    .get_host_address(GuestAddress(SNP_SECRETS_START))
                    .unwrap() as u64,
                size: 0x1000,
            },
            &mut launcher,
            PageType::Secrets,
        )?;

        // CPUID page
        let old_cpuid = self.write_cpuid_page(cpuid, guest_mem)?;
        if let Err(e) = self.add_region(
            guest_mem,
            MeasuredRegion {
                guest_addr: SNP_CPUID_START,
                host_addr: guest_mem
                    .get_host_address(GuestAddress(SNP_CPUID_START))
                    .unwrap() as u64,
                size: 0x1000,
            },
            &mut launcher,
            PageType::Cpuid,
        ) {
            // The PSP fixes the tables itself, so a second attempt should succeed.
            warn!("PSP rejected the CPUID page ({e:?}). Trying again.");

            self.check_cpuid_page(guest_mem, old_cpuid)?;
            if let Err(e) = self.add_region(
                guest_mem,
                MeasuredRegion {
                    guest_addr: SNP_CPUID_START,
                    host_addr: guest_mem
                        .get_host_address(GuestAddress(SNP_CPUID_START))
                        .unwrap() as u64,
                    size: 0x1000,
                },
                &mut launcher,
                PageType::Cpuid,
            ) {
                error!("PSP rejected the CPUID page fixed by itself: {e:?}");
            }
        }

        // FW stack and initial page tables
        self.add_region(
            guest_mem,
            MeasuredRegion {
                guest_addr: SNP_FWDATA_START,
                host_addr: guest_mem
                    .get_host_address(GuestAddress(SNP_FWDATA_START))
                    .unwrap() as u64,
                size: SNP_FWDATA_SIZE,
            },
            &mut launcher,
            PageType::Zero,
        )?;

        let finish = Finish::new(None, None, [0; 32]);

        let (_vmfd, _fwfd) = launcher.finish(finish).map_err(Error::LaunchFinish)?;

        Ok(())
    }
}
