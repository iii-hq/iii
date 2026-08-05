//! Network configuration for iii worker VM sandboxes.

/// Network configuration controlling the smoltcp stack behavior.
///
/// Phase 7 extends this with DNS config, proxy settings, etc.
pub struct NetworkConfig {
    pub enabled: bool,
    pub mtu: u16,
    /// Path of the network-activity beacon file (see
    /// [`crate::shared::ActivityStamp`]). When set, the stack refreshes this
    /// file's mtime whenever guest payload is relayed, so an external idle
    /// reaper can tell a network-serving sandbox from a dead one. `None`
    /// (the default) disables the beacon entirely.
    pub activity_file: Option<std::path::PathBuf>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mtu: 1500,
            activity_file: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_enabled() {
        let cfg = NetworkConfig::default();
        assert!(cfg.enabled);
    }

    #[test]
    fn default_config_mtu_1500() {
        let cfg = NetworkConfig::default();
        assert_eq!(cfg.mtu, 1500);
    }
}
