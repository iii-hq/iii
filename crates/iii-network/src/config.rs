//! Network configuration for iii worker VM sandboxes.

/// Network configuration controlling the smoltcp stack behavior.
///
/// Phase 7 extends this with DNS config, proxy settings, etc.
pub struct NetworkConfig {
    pub enabled: bool,
    pub mtu: u16,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mtu: 1500,
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
