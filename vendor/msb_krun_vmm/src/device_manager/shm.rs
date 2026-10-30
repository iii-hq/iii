use std::collections::BTreeMap;

use arch::ArchMemoryInfo;
use vm_memory::GuestAddress;
use vmm_sys_util::align_upwards;

#[derive(Debug)]
pub enum Error {
    DuplicatedGpuRegion,
    OutOfSpace,
}

#[derive(Clone)]
pub struct ShmRegion {
    pub guest_addr: GuestAddress,
    pub size: usize,
}

pub struct ShmManager {
    next_guest_addr: u64,
    page_size: usize,
    fs_regions: BTreeMap<usize, ShmRegion>,
    gpu_region: Option<ShmRegion>,
}

impl ShmManager {
    pub fn new(info: &ArchMemoryInfo) -> ShmManager {
        Self {
            next_guest_addr: info.shm_start_addr,
            page_size: info.page_size,
            fs_regions: BTreeMap::new(),
            gpu_region: None,
        }
    }

    pub fn regions(&self) -> Vec<(GuestAddress, usize)> {
        let mut regions: Vec<(GuestAddress, usize)> = Vec::new();

        for region in self.fs_regions.iter() {
            regions.push((region.1.guest_addr, region.1.size));
        }

        if let Some(region) = &self.gpu_region {
            regions.push((region.guest_addr, region.size));
        }

        regions
    }

    #[cfg(not(any(feature = "tee", feature = "aws-nitro")))]
    pub fn fs_region(&self, index: usize) -> Option<&ShmRegion> {
        self.fs_regions.get(&index)
    }

    #[cfg(feature = "gpu")]
    pub fn gpu_region(&self) -> Option<&ShmRegion> {
        self.gpu_region.as_ref()
    }

    fn create_region(&mut self, size: usize) -> Result<ShmRegion, Error> {
        let size = align_upwards!(size, self.page_size);

        let region = ShmRegion {
            guest_addr: GuestAddress(self.next_guest_addr),
            size,
        };

        if let Some(addr) = self.next_guest_addr.checked_add(size as u64) {
            self.next_guest_addr = addr;
            Ok(region)
        } else {
            Err(Error::OutOfSpace)
        }
    }

    pub fn create_gpu_region(&mut self, size: usize) -> Result<(), Error> {
        if self.gpu_region.is_some() {
            Err(Error::DuplicatedGpuRegion)
        } else {
            self.gpu_region = Some(self.create_region(size)?);
            Ok(())
        }
    }

    #[cfg(not(feature = "tee"))]
    pub fn create_fs_region(&mut self, index: usize, size: usize) -> Result<(), Error> {
        let region = self.create_region(size)?;
        self.fs_regions.insert(index, region);
        Ok(())
    }
}
