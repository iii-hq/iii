//! iii-observability: shared OTel + Logger primitives for the iii Rust SDK.
pub const VERSION: &str = "0.13.0-next.1";

pub mod telemetry;

pub use telemetry::types::{OtelConfig, ReconnectionConfig};
