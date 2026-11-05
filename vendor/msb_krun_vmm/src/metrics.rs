// Copyright 2026 Super Rad Company
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::sync::Mutex;

use arch::ArchMemoryInfo;
use utils::metrics::MetricsWriter;
use vm_memory::{Address, GuestMemory, GuestMemoryMmap, GuestMemoryRegion};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GuestMemoryRange {
    start: u64,
    end: u64,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

pub(crate) fn install_host_resident_memory_sampler(
    metrics: &MetricsWriter,
    guest_memory: &GuestMemoryMmap,
    arch_memory_info: &ArchMemoryInfo,
) {
    let guest_memory = guest_memory.clone();
    let ranges = guest_memory_ranges(arch_memory_info);
    let page_size = page_size();
    let residency = Mutex::new(Vec::new());

    metrics.set_memory_host_resident_sampler(move || {
        match host_resident_memory_bytes(&guest_memory, &ranges, page_size, &residency) {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                debug!("failed to sample host-resident guest memory: {err}");
                None
            }
        }
    });
}

fn host_resident_memory_bytes(
    guest_memory: &GuestMemoryMmap,
    ranges: &[GuestMemoryRange],
    page_size: usize,
    residency: &Mutex<Vec<u8>>,
) -> io::Result<u64> {
    let mut total = 0u64;
    let mut residency = residency.lock().unwrap();
    for region in guest_memory.iter() {
        for range in ranges {
            let Some((offset, len)) =
                inspect_region_range(region.start_addr().raw_value(), region.len(), *range)
            else {
                continue;
            };
            total = total.saturating_add(region_resident_bytes(
                region.as_ptr().wrapping_add(offset),
                len,
                page_size,
                &mut residency,
            )?);
        }
    }
    Ok(total)
}

fn guest_memory_ranges(info: &ArchMemoryInfo) -> Vec<GuestMemoryRange> {
    #[cfg(target_arch = "x86_64")]
    {
        let mut ranges = Vec::new();
        if info.ram_below_gap > 0 {
            ranges.push(GuestMemoryRange {
                start: 0,
                end: info.ram_below_gap,
            });
        }
        if info.ram_above_gap > 0 {
            ranges.push(GuestMemoryRange {
                start: info.ram_last_addr.saturating_sub(info.ram_above_gap),
                end: info.ram_last_addr,
            });
        }
        ranges
    }

    #[cfg(target_arch = "aarch64")]
    {
        vec![GuestMemoryRange {
            start: info.ram_start_addr,
            end: info.ram_last_addr,
        }]
    }

    #[cfg(target_arch = "riscv64")]
    {
        vec![GuestMemoryRange {
            start: 0,
            end: info.ram_last_addr,
        }]
    }
}

fn inspect_region_range(
    region_start: u64,
    region_len: u64,
    range: GuestMemoryRange,
) -> Option<(usize, usize)> {
    if region_len == 0 || range.start >= range.end {
        return None;
    }

    let region_end = region_start.saturating_add(region_len);
    let overlap_start = region_start.max(range.start);
    let overlap_end = region_end.min(range.end);
    if overlap_start >= overlap_end {
        return None;
    }

    Some((
        usize::try_from(overlap_start - region_start).ok()?,
        usize::try_from(overlap_end - overlap_start).ok()?,
    ))
}

fn region_resident_bytes(
    host_addr: *mut u8,
    len: usize,
    page_size: usize,
    residency: &mut Vec<u8>,
) -> io::Result<u64> {
    if len == 0 {
        return Ok(0);
    }

    let start = host_addr as usize;
    let aligned_start = start - (start % page_size);
    let page_offset = start - aligned_start;
    let inspected_len = len
        .checked_add(page_offset)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "mincore range overflow"))?;
    let page_count = inspected_len.div_ceil(page_size);
    residency.resize(page_count, 0);

    mincore(aligned_start as *mut u8, inspected_len, residency)?;

    Ok(resident_bytes_from_mincore(
        residency,
        page_offset,
        len,
        page_size,
    ))
}

fn resident_bytes_from_mincore(
    residency: &[u8],
    page_offset: usize,
    len: usize,
    page_size: usize,
) -> u64 {
    let region_start = page_offset;
    let region_end = page_offset + len;
    let mut bytes = 0usize;

    for (idx, entry) in residency.iter().enumerate() {
        if entry & 1 == 0 {
            continue;
        }

        let page_start = idx * page_size;
        let page_end = page_start + page_size;
        let overlap_start = page_start.max(region_start);
        let overlap_end = page_end.min(region_end);
        bytes = bytes.saturating_add(overlap_end.saturating_sub(overlap_start));
    }

    bytes as u64
}

#[cfg(target_os = "linux")]
fn mincore(addr: *mut u8, len: usize, residency: &mut [u8]) -> io::Result<()> {
    let rc = unsafe {
        libc::mincore(
            addr.cast::<libc::c_void>(),
            len,
            residency.as_mut_ptr().cast::<libc::c_uchar>(),
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn mincore(addr: *mut u8, len: usize, residency: &mut [u8]) -> io::Result<()> {
    let rc = unsafe {
        libc::mincore(
            addr.cast::<libc::c_void>(),
            len,
            residency.as_mut_ptr().cast::<libc::c_char>(),
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn page_size() -> usize {
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(4096)
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_region_range_skips_non_overlapping_regions() {
        let range = GuestMemoryRange {
            start: 4096,
            end: 8192,
        };

        assert_eq!(inspect_region_range(0, 4096, range), None);
        assert_eq!(inspect_region_range(8192, 4096, range), None);
    }

    #[test]
    fn inspect_region_range_clamps_to_range_overlap() {
        let range = GuestMemoryRange {
            start: 4096,
            end: 8192,
        };

        assert_eq!(inspect_region_range(0, 8192, range), Some((4096, 4096)));
        assert_eq!(inspect_region_range(6144, 4096, range), Some((0, 2048)));
    }

    #[test]
    fn resident_bytes_accounts_for_partial_pages() {
        assert_eq!(
            resident_bytes_from_mincore(&[1, 0, 1], 1024, 6144, 4096),
            3072
        );
        assert_eq!(resident_bytes_from_mincore(&[1, 1], 1024, 2048, 4096), 2048);
    }
}
