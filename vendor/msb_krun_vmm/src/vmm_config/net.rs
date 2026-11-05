// Copyright 2018 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;
use std::fmt;
use std::result;
use std::sync::{Arc, Mutex};

use devices::virtio::net::device::VirtioNetBackend;
use devices::virtio::Net;

pub struct NetworkInterfaceConfig {
    /// ID of the guest network interface.
    pub iface_id: String,
    /// Backend to transport data to/from the host.
    pub backend: VirtioNetBackend,
    /// MAC address.
    pub mac: [u8; 6],
    /// virtio-net features for the network interface.
    pub features: u32,
}

/// Errors associated with `NetworkInterfaceConfig`.
#[derive(Debug)]
pub enum NetworkInterfaceError {
    /// Could not create Network Device.
    CreateNetworkDevice(devices::virtio::net::Error),
    /// Couldn't find the interface to update (patch).
    DeviceIdNotFound,
}

impl fmt::Display for NetworkInterfaceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::NetworkInterfaceError::*;
        match *self {
            CreateNetworkDevice(ref e) => write!(f, "Could not create Network Device: {e:?}"),
            DeviceIdNotFound => write!(f, "Invalid interface ID - not found."),
        }
    }
}

type Result<T> = result::Result<T, NetworkInterfaceError>;

/// Builder for a list of network devices.
#[derive(Default)]
pub struct NetBuilder {
    pub list: VecDeque<Arc<Mutex<Net>>>,
}

impl NetBuilder {
    /// Creates an empty list of Network Devices.
    pub fn new() -> Self {
        NetBuilder {
            // List of built network devices.
            list: VecDeque::new(),
        }
    }

    pub fn insert(&mut self, config: NetworkInterfaceConfig) -> Result<()> {
        let net_dev = Arc::new(Mutex::new(Self::create_net(config)?));
        self.list.push_back(net_dev);
        Ok(())
    }

    /// Creates a Net device from a NetworkInterfaceConfig.
    pub fn create_net(cfg: NetworkInterfaceConfig) -> Result<Net> {
        // Create and return the Net device
        Net::new(cfg.iface_id, cfg.backend, cfg.mac, cfg.features)
            .map_err(NetworkInterfaceError::CreateNetworkDevice)
    }
}
