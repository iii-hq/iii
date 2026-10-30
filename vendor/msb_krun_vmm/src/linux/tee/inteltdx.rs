use kvm_ioctls::VmFd;
use tdx::launch::{self, Launcher};

use std::os::unix::io::AsRawFd;

#[derive(Debug)]
pub enum Error {
    GetCapabilities(launch::Error),
    InitVm(launch::Error),
    InitMemoryRegions(launch::Error),
    FinalizeVm(launch::Error),
}

type Result<T> = std::result::Result<T, Error>;

pub struct IntelTdx {}

impl IntelTdx {
    pub fn new() -> Self {
        Self {}
    }

    pub fn vm_prepare(&self, vm_fd: &VmFd, cpuid: kvm_bindings::CpuId) -> Result<Launcher> {
        let mut launcher = Launcher::new(vm_fd.as_raw_fd());
        let caps = launcher
            .get_capabilities()
            .map_err(Error::GetCapabilities)?;
        launcher.init_vm(&caps, cpuid).map_err(Error::InitVm)?;
        Ok(launcher)
    }

    pub fn configure_td_memory(
        &self,
        launcher: &mut Launcher,
        regions: &Vec<crate::vstate::MeasuredRegion>,
    ) -> Result<()> {
        for region in regions {
            let mem_region = tdx::launch::MemRegion::new(
                region.guest_addr,
                (region.size / 4096) as u64,
                (arch::FIRMWARE_START == region.guest_addr).into(),
                region.host_addr,
            );
            launcher
                .init_mem_region(mem_region)
                .map_err(Error::InitMemoryRegions)?;
        }

        Ok(())
    }

    pub fn finalize_vm(&self, mut launcher: Launcher) -> Result<()> {
        launcher.finalize().map_err(Error::FinalizeVm)
    }
}
