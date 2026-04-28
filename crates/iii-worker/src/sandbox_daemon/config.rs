//! Fail-closed behavior:
//!   - `image_allowlist` empty denies every `sandbox::create` (see
//!     `catalog::check_allowlist`).
//!   - `load_config` returns an error on missing / malformed YAML;
//!     the daemon exits non-zero instead of falling back to defaults.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PerImageCap {
    pub max_cpus: u32,
    pub max_memory_mb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    #[serde(default = "default_sandbox_auto_install")]
    pub auto_install: bool,
    #[serde(default)]
    pub image_allowlist: Vec<String>,
    #[serde(default = "default_sandbox_idle_timeout")]
    pub default_idle_timeout_secs: u64,
    #[serde(default = "default_sandbox_max_concurrent")]
    pub max_concurrent_sandboxes: u32,
    #[serde(default = "default_sandbox_cpus")]
    pub default_cpus: u32,
    #[serde(default = "default_sandbox_memory")]
    pub default_memory_mb: u32,
    #[serde(default)]
    pub per_image_caps: std::collections::HashMap<String, PerImageCap>,
    /// Deployment-specific images beyond the built-in presets. Map key is
    /// the name used in `image_allowlist` and the `image` field on
    /// `sandbox::create`; value is a fully-qualified OCI reference
    /// (e.g. `ghcr.io/acme/my-app:1.2.3`).
    ///
    /// Preset names (`python`, `node`, `bash`, `alpine`) are reserved —
    /// declaring one here is rejected by `load_config` so nothing can
    /// silently shadow the trusted catalog. An image named here still
    /// must appear in `image_allowlist` to be bootable.
    #[serde(default)]
    pub custom_images: std::collections::HashMap<String, String>,
}

fn default_sandbox_auto_install() -> bool {
    true
}
fn default_sandbox_idle_timeout() -> u64 {
    300
}
fn default_sandbox_max_concurrent() -> u32 {
    32
}
fn default_sandbox_cpus() -> u32 {
    1
}
fn default_sandbox_memory() -> u32 {
    512
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            auto_install: default_sandbox_auto_install(),
            image_allowlist: Vec::new(),
            default_idle_timeout_secs: default_sandbox_idle_timeout(),
            max_concurrent_sandboxes: default_sandbox_max_concurrent(),
            default_cpus: default_sandbox_cpus(),
            default_memory_mb: default_sandbox_memory(),
            per_image_caps: std::collections::HashMap::new(),
            custom_images: std::collections::HashMap::new(),
        }
    }
}

/// Load + parse config YAML. **Fail-closed**: any I/O or parse error
/// propagates — the daemon exits instead of falling back to defaults.
///
/// Rejects the legacy wrapped shape (`sandbox: {...}`) with a clear
/// migration message. That shape was the vm-worker wrapper around
/// `SandboxConfig` and went away with `vm::exec`.
pub fn load_config(path: &str) -> Result<SandboxConfig> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path))?;
    let raw: serde_yaml::Value =
        serde_yaml::from_str(&content).with_context(|| format!("parse {}", path))?;
    reject_legacy_wrapped_shape(&raw).with_context(|| format!("iii-sandbox config in {}", path))?;
    let cfg: SandboxConfig =
        serde_yaml::from_value(raw).with_context(|| format!("parse {}", path))?;
    validate_custom_images(&cfg).with_context(|| format!("custom_images in {}", path))?;
    Ok(cfg)
}

fn reject_legacy_wrapped_shape(raw: &serde_yaml::Value) -> Result<()> {
    let Some(map) = raw.as_mapping() else {
        return Ok(());
    };
    let sandbox_key = serde_yaml::Value::String("sandbox".into());
    if map.contains_key(&sandbox_key) {
        anyhow::bail!(
            "legacy wrapped shape detected: move fields from `sandbox:` to the top level. \
             The vm-worker wrapper (`VmWorkerConfig`) and `vm::exec` were removed; \
             `SandboxConfig` is the top-level config now. \
             Example: `image_allowlist: [python, node]` at the top level, not under `sandbox:`."
        );
    }
    Ok(())
}

/// Reject `custom_images` entries that collide with a catalog preset
/// name. Shadowing is prevented at the `resolve_image` layer too, but
/// failing at load-time gives the operator a clear error instead of
/// letting a silently-inert key sit in their config.
fn validate_custom_images(sandbox: &SandboxConfig) -> Result<()> {
    use crate::sandbox_daemon::catalog;
    let presets: std::collections::HashSet<&str> = catalog::preset_names().collect();
    for key in sandbox.custom_images.keys() {
        if presets.contains(key.as_str()) {
            anyhow::bail!(
                "custom_images key '{key}' shadows a built-in preset — rename it \
                 (presets: {})",
                catalog::preset_names().collect::<Vec<_>>().join(", ")
            );
        }
        if key.is_empty() {
            anyhow::bail!("custom_images key must not be empty");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_shape() {
        let yaml = r#"
auto_install: true
image_allowlist:
  - python
  - node
default_idle_timeout_secs: 300
max_concurrent_sandboxes: 32
default_cpus: 1
default_memory_mb: 512
"#;
        let cfg: SandboxConfig = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(cfg.auto_install, true);
        assert_eq!(
            cfg.image_allowlist,
            vec!["python", "node"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(cfg.default_idle_timeout_secs, 300);
        assert_eq!(cfg.max_concurrent_sandboxes, 32);
        assert_eq!(cfg.default_cpus, 1);
        assert_eq!(cfg.default_memory_mb, 512);
    }

    #[test]
    fn defaults_fill_in_when_fields_absent() {
        let cfg: SandboxConfig = serde_yaml::from_str("{}").expect("parse");
        assert_eq!(cfg.auto_install, true);
        assert!(cfg.image_allowlist.is_empty());
        assert_eq!(cfg.default_idle_timeout_secs, 300);
        assert_eq!(cfg.max_concurrent_sandboxes, 32);
        assert_eq!(cfg.default_cpus, 1);
        assert_eq!(cfg.default_memory_mb, 512);
        assert!(cfg.per_image_caps.is_empty());
        assert!(cfg.custom_images.is_empty());
    }

    #[test]
    fn parses_custom_images_map() {
        let yaml = r#"
auto_install: true
image_allowlist: [my-app]
custom_images:
  my-app: ghcr.io/acme/my-app:1.2.3
  other: docker.io/tenant/other:latest
"#;
        let cfg: SandboxConfig = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(
            cfg.custom_images.get("my-app").unwrap(),
            "ghcr.io/acme/my-app:1.2.3"
        );
        assert_eq!(
            cfg.custom_images.get("other").unwrap(),
            "docker.io/tenant/other:latest"
        );
    }

    #[test]
    fn parses_per_image_caps() {
        let yaml = r#"
image_allowlist: [python]
per_image_caps:
  python: { max_cpus: 4, max_memory_mb: 2048 }
"#;
        let cfg: SandboxConfig = serde_yaml::from_str(yaml).expect("parse");
        let cap = cfg.per_image_caps.get("python").expect("python cap");
        assert_eq!(cap.max_cpus, 4);
        assert_eq!(cap.max_memory_mb, 2048);
    }

    #[test]
    fn load_config_rejects_legacy_wrapped_shape() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "sandbox:\n  image_allowlist: [python]\n").unwrap();
        let err = load_config(tmp.path().to_str().unwrap()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("legacy wrapped shape"),
            "expected migration error, got: {msg}"
        );
    }

    #[test]
    fn load_config_rejects_custom_image_shadowing_preset() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "custom_images:\n  python: docker.io/evil/python:latest\n",
        )
        .unwrap();
        let err = load_config(tmp.path().to_str().unwrap()).unwrap_err();
        assert!(format!("{err:#}").contains("shadows a built-in preset"));
    }

    #[test]
    fn load_config_rejects_empty_custom_image_key() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            "custom_images:\n  \"\": docker.io/whatever:latest\n",
        )
        .unwrap();
        let err = load_config(tmp.path().to_str().unwrap()).unwrap_err();
        assert!(format!("{err:#}").contains("must not be empty"));
    }

    #[test]
    fn load_config_missing_file_errors() {
        let err = load_config("/this/path/does/not/exist/sandbox.yaml");
        assert!(err.is_err());
    }
}
