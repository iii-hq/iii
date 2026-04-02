//! Userspace TCP/IP networking for iii worker VM sandboxes.
//!
//! Provides the shared-memory bridge between libkrun's NetWorker thread and the
//! smoltcp poll thread. Every guest ethernet frame flows through these types.

pub mod backend;
pub mod config;
pub mod conn;
pub mod device;
pub mod dns;
pub mod network;
pub mod proxy;
pub mod shared;
pub mod stack;
pub mod udp_relay;
pub mod wake_pipe;

pub use backend::SmoltcpBackend;
pub use config::NetworkConfig;
pub use conn::{ConnectionTracker, NewConnection};
pub use device::SmoltcpDevice;
pub use dns::DnsInterceptor;
pub use network::SmoltcpNetwork;
pub use proxy::spawn_tcp_proxy;
pub use shared::{DEFAULT_QUEUE_CAPACITY, SharedState};
pub use stack::{classify_frame, create_interface, smoltcp_poll_loop, FrameAction, PollLoopConfig};
pub use udp_relay::UdpRelay;
pub use wake_pipe::WakePipe;
