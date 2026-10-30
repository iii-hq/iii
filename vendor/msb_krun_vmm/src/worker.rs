use std::io;
use std::sync::{Arc, Mutex};

#[cfg(feature = "tee")]
use utils::worker_message::MemoryProperties;
use utils::worker_message::WorkerMessage;

use crossbeam_channel::Receiver;
#[cfg(feature = "tee")]
use crossbeam_channel::Sender;
#[cfg(feature = "tee")]
use kvm_bindings::{kvm_memory_attributes, KVM_MEMORY_ATTRIBUTE_PRIVATE};
#[cfg(feature = "tee")]
use libc::{fallocate, madvise, FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE, MADV_DONTNEED};
#[cfg(feature = "tee")]
use std::ffi::c_void;
#[cfg(feature = "tee")]
use vm_memory::{
    guest_memory::GuestMemory, Address, GuestAddress, GuestMemoryRegion, MemoryRegionAddress,
};

pub fn start_worker_thread(
    vmm: Arc<Mutex<super::Vmm>>,
    receiver: Receiver<WorkerMessage>,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("vmm worker".into())
        .spawn(move || loop {
            match receiver.recv() {
                Err(e) => error!("error receiving message from vmm worker thread: {e:?}"),
                #[cfg(target_os = "macos")]
                Ok(message) => vmm.lock().unwrap().match_worker_message(message),
                #[cfg(target_os = "linux")]
                Ok(message) => vmm.lock().unwrap().match_worker_message(message),
            }
        })?;
    Ok(())
}

impl super::Vmm {
    fn match_worker_message(&self, msg: WorkerMessage) {
        match msg {
            #[cfg(target_os = "macos")]
            WorkerMessage::GpuAddMapping(s, h, g, l) => self.add_mapping(s, h, g, l),
            #[cfg(target_os = "macos")]
            WorkerMessage::GpuRemoveMapping(s, g, l) => self.remove_mapping(s, g, l),
            #[cfg(target_arch = "x86_64")]
            WorkerMessage::GsiRoute(sender, entries) => {
                let mut routing = kvm_bindings::KvmIrqRouting::new(entries.len()).unwrap();
                let routing_entries = routing.as_mut_slice();
                routing_entries.copy_from_slice(&entries);
                sender
                    .send(self.vm.fd().set_gsi_routing(&routing).is_ok())
                    .unwrap();
            }
            #[cfg(target_arch = "x86_64")]
            WorkerMessage::IrqLine(sender, irq, active) => {
                sender
                    .send(self.vm.fd().set_irq_line(irq, active).is_ok())
                    .unwrap();
            }
            WorkerMessage::ConvertMemory(_sender, _properties) =>
            {
                #[cfg(feature = "tee")]
                self.convert_memory(_sender, _properties)
            }
        }
    }

    #[cfg(feature = "tee")]
    fn convert_memory(&self, sender: Sender<bool>, properties: MemoryProperties) {
        let Some((guest_memfd, region_start)) = self.kvm_vm().guest_memfd_get(properties.gpa)
        else {
            error!(
                "unable to find KVM guest_memfd for memory region corresponding to GPA 0x{:x}",
                properties.gpa
            );
            sender.send(false).unwrap();
            return;
        };

        let attributes: u64 = if properties.private {
            KVM_MEMORY_ATTRIBUTE_PRIVATE as u64
        } else {
            0
        };

        let attr = kvm_memory_attributes {
            address: properties.gpa,
            size: properties.size,
            attributes,
            flags: 0,
        };

        if self.kvm_vm().fd().set_memory_attributes(attr).is_err() {
            error!("unable to set memory attributes for memory region corresponding to guest address 0x{:x}", properties.gpa);
            sender.send(false).unwrap();
            return;
        }

        let region = self
            .guest_memory()
            .find_region(GuestAddress(properties.gpa));
        if region.is_none() {
            error!(
                "guest memory region corresponding to GPA 0x{:x} not found",
                properties.gpa
            );
            sender.send(false).unwrap();
            return;
        }

        let offset = properties.gpa - region_start;

        if properties.private {
            let region_addr = MemoryRegionAddress(offset);

            let Ok(host_startaddr) = region.unwrap().get_host_address(region_addr) else {
                error!(
                    "host address corresponding to memory region address 0x{:x} not found",
                    region_addr.raw_value()
                );
                sender.send(false).unwrap();
                return;
            };

            let ret = unsafe {
                madvise(
                    host_startaddr as *mut c_void,
                    properties.size.try_into().unwrap(),
                    MADV_DONTNEED,
                )
            };

            if ret < 0 {
                error!("unable to advise kernel that memory region corresponding to GPA 0x{:x} will likely not be needed (madvise)", properties.gpa);
                sender.send(false).unwrap();
            }
        } else {
            let ret = unsafe {
                fallocate(
                    guest_memfd,
                    FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE,
                    offset as i64,
                    properties.size as i64,
                )
            };

            if ret < 0 {
                error!("unable to allocate space in guest_memfd for shared memory (fallocate)");
                sender.send(false).unwrap();
            }
        }

        sender.send(true).unwrap();
    }
}
