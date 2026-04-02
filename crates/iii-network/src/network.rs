//! `SmoltcpNetwork` — orchestration type that ties [`NetworkConfig`] to the
//! smoltcp engine.
//!
//! The single type the runtime creates from config, wires into the VM builder,
//! and starts the networking stack.

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::thread::JoinHandle;

use msb_krun::backends::net::NetBackend;

use crate::backend::SmoltcpBackend;
use crate::config::NetworkConfig;
use crate::shared::{DEFAULT_QUEUE_CAPACITY, SharedState};
use crate::stack::{self, PollLoopConfig};

/// Maximum sandbox slot value. Limited by MAC encoding (16 bits = 65535).
const MAX_SLOT: u64 = u16::MAX as u64;

/// The networking engine. Created from [`NetworkConfig`] by the runtime.
///
/// Owns the smoltcp poll thread and provides:
/// - [`take_backend()`](Self::take_backend) — the `NetBackend` for `VmBuilder::net()`
/// - [`guest_mac()`](Self::guest_mac) — MAC for `VmBuilder::net().mac()`
pub struct SmoltcpNetwork {
    #[allow(dead_code)]
    config: NetworkConfig,
    shared: Arc<SharedState>,
    backend: Option<SmoltcpBackend>,
    poll_handle: Option<JoinHandle<()>>,
    guest_mac: [u8; 6],
    gateway_mac: [u8; 6],
    mtu: u16,
    guest_ipv4: Ipv4Addr,
    gateway_ipv4: Ipv4Addr,
}

impl SmoltcpNetwork {
    /// Create from user config + sandbox slot (for IP/MAC derivation).
    ///
    /// # Panics
    ///
    /// Panics if `slot` exceeds the address pool capacity (65535).
    pub fn new(config: NetworkConfig, slot: u64) -> Self {
        assert!(
            slot <= MAX_SLOT,
            "sandbox slot {slot} exceeds address pool capacity (max {MAX_SLOT})"
        );

        let guest_mac = derive_guest_mac(slot);
        let gateway_mac = derive_gateway_mac(slot);
        let mtu = config.mtu;
        let guest_ipv4 = derive_guest_ipv4(slot);
        let gateway_ipv4 = gateway_from_guest_ipv4(guest_ipv4);

        let shared = Arc::new(SharedState::new(DEFAULT_QUEUE_CAPACITY));
        let backend = SmoltcpBackend::new(shared.clone());

        Self {
            config,
            shared,
            backend: Some(backend),
            poll_handle: None,
            guest_mac,
            gateway_mac,
            mtu,
            guest_ipv4,
            gateway_ipv4,
        }
    }

    /// Start the smoltcp poll thread.
    ///
    /// Must be called before VM boot. The tokio handle is stored for Phase 7
    /// proxy task spawning.
    pub fn start(&mut self, tokio_handle: tokio::runtime::Handle) {
        let shared = self.shared.clone();
        let poll_config = PollLoopConfig {
            gateway_mac: self.gateway_mac,
            guest_mac: self.guest_mac,
            gateway_ipv4: self.gateway_ipv4,
            guest_ipv4: self.guest_ipv4,
            mtu: self.mtu as usize,
        };

        self.poll_handle = Some(
            std::thread::Builder::new()
                .name("smoltcp-poll".into())
                .spawn(move || {
                    stack::smoltcp_poll_loop(shared, poll_config, tokio_handle);
                })
                .expect("failed to spawn smoltcp poll thread"),
        );
    }

    /// Take the `NetBackend` for `VmBuilder::net()`. One-shot.
    pub fn take_backend(&mut self) -> Box<dyn NetBackend + Send> {
        Box::new(self.backend.take().expect("backend already taken"))
    }

    /// Guest MAC address for `VmBuilder::net().mac()`.
    pub fn guest_mac(&self) -> [u8; 6] {
        self.guest_mac
    }

    /// Gateway IPv4 address.
    pub fn gateway_ipv4(&self) -> Ipv4Addr {
        self.gateway_ipv4
    }

    /// Guest IPv4 address.
    pub fn guest_ipv4(&self) -> Ipv4Addr {
        self.guest_ipv4
    }
}

/// Derive a guest MAC address from the sandbox slot.
///
/// Format: `02:6d:73:SS:SS:02` where SS:SS encodes the slot.
fn derive_guest_mac(slot: u64) -> [u8; 6] {
    let s = slot.to_be_bytes();
    [0x02, 0x6d, 0x73, s[6], s[7], 0x02]
}

/// Derive a gateway MAC address from the sandbox slot.
///
/// Format: `02:6d:73:SS:SS:01`.
fn derive_gateway_mac(slot: u64) -> [u8; 6] {
    let s = slot.to_be_bytes();
    [0x02, 0x6d, 0x73, s[6], s[7], 0x01]
}

/// Derive a guest IPv4 address from the sandbox slot.
///
/// Pool: `100.96.0.0/11`. Each slot gets a `/30` block (4 IPs).
/// Guest is at offset +2 in the block.
fn derive_guest_ipv4(slot: u64) -> Ipv4Addr {
    let base: u32 = u32::from(Ipv4Addr::new(100, 96, 0, 0));
    let offset = (slot as u32) * 4 + 2;
    Ipv4Addr::from(base + offset)
}

/// Gateway IPv4 from guest IPv4: guest - 1 (offset +1 in the /30 block).
fn gateway_from_guest_ipv4(guest: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(guest) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_addresses_slot_0() {
        assert_eq!(derive_guest_mac(0), [0x02, 0x6d, 0x73, 0x00, 0x00, 0x02]);
        assert_eq!(derive_gateway_mac(0), [0x02, 0x6d, 0x73, 0x00, 0x00, 0x01]);
        assert_eq!(derive_guest_ipv4(0), Ipv4Addr::new(100, 96, 0, 2));
        assert_eq!(
            gateway_from_guest_ipv4(Ipv4Addr::new(100, 96, 0, 2)),
            Ipv4Addr::new(100, 96, 0, 1)
        );
    }

    #[test]
    fn derive_addresses_slot_1() {
        assert_eq!(derive_guest_ipv4(1), Ipv4Addr::new(100, 96, 0, 6));
        assert_eq!(
            gateway_from_guest_ipv4(Ipv4Addr::new(100, 96, 0, 6)),
            Ipv4Addr::new(100, 96, 0, 5)
        );
    }
}
