//! OCI refs are stored in canonical form (`docker.io/<ns>/<repo>:<tag>`)
//! so they hash to the same rootfs-cache slug as the managed-worker side
//! (see `oci_image_for_kind`). Shorthand would pull the same image to a
//! second cache slug under `~/.iii/cache/`.

use crate::sandbox_daemon::errors::SandboxError;
use std::collections::HashMap;

static PRESETS: &[(&str, &str)] = &[
    ("python", "docker.io/iiidev/python:latest"),
    ("node", "docker.io/iiidev/node:latest"),
];

pub fn resolve_preset(image: &str) -> Option<&'static str> {
    PRESETS
        .iter()
        .find(|(name, _)| *name == image)
        .map(|(_, oci)| *oci)
}

pub fn is_preset(image: &str) -> bool {
    PRESETS.iter().any(|(name, _)| *name == image)
}

/// Names of the built-in presets. Used by `SandboxConfig` validation to
/// reject `custom_images` entries that would otherwise silently be
/// ignored (presets always win in `resolve_image`).
pub fn preset_names() -> impl Iterator<Item = &'static str> {
    PRESETS.iter().map(|(name, _)| *name)
}

/// Resolve an image name to its OCI reference. Presets shadow
/// `custom_images` — a malicious or mistaken config cannot redirect
/// `python` to an attacker-controlled ref. Returns `None` when the
/// name is unknown to both catalogs.
pub fn resolve_image(image: &str, custom_images: &HashMap<String, String>) -> Option<String> {
    if let Some(preset) = resolve_preset(image) {
        return Some(preset.to_string());
    }
    custom_images.get(image).cloned()
}

/// `true` when `image` is either a catalog preset OR a key in
/// `custom_images`. Used by `check_allowlist` to decide whether an
/// image is known to the daemon at all, separate from whether it has
/// been explicitly permitted by `image_allowlist`.
pub fn is_known_image(image: &str, custom_images: &HashMap<String, String>) -> bool {
    is_preset(image) || custom_images.contains_key(image)
}

/// Two-stage fail-closed check: the image must be known (preset or
/// custom) AND explicitly allowlisted. An empty `allowlist` denies
/// everything — there is no "open by default" mode.
pub fn check_allowlist(
    image: &str,
    allowlist: &[String],
    custom_images: &HashMap<String, String>,
) -> Result<(), SandboxError> {
    if !is_known_image(image, custom_images) {
        return Err(SandboxError::image_not_in_catalog(image));
    }
    if allowlist.iter().any(|a| a == image) {
        Ok(())
    } else {
        Err(SandboxError::image_not_in_catalog(image))
    }
}

/// `sandbox::catalog::list` request. No fields — kept as a struct (not
/// `()`) so adding optional filters (e.g. `include_oci_refs: bool`) later
/// is a non-breaking serde change.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct CatalogListRequest {}

/// One row in the catalog response. `name` is the catalog key that
/// callers pass to `sandbox::create` (or `sandbox::run`); `oci_ref` is the
/// fully-qualified OCI reference the daemon will pull. `kind` lets agents
/// distinguish bundled presets from operator-registered custom images
/// without having to maintain their own preset allowlist.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct CatalogEntry {
    pub name: String,
    pub oci_ref: String,
    pub kind: CatalogEntryKind,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogEntryKind {
    /// Bundled with the daemon binary. Stable identifier across releases.
    Preset,
    /// Operator-registered under `sandbox.custom_images` in
    /// `iii.config.yaml`. Specific to this deployment.
    Custom,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct CatalogListResponse {
    /// Every image the daemon will accept on `sandbox::create.image`.
    /// Presets come first in stable order; custom entries follow,
    /// sorted by `name` for determinism so an agent that diffs two
    /// `catalog::list` responses sees stable output.
    pub images: Vec<CatalogEntry>,
}

/// Handler for `sandbox::catalog::list`. Infallible — returns the union of
/// built-in presets and the operator-registered `custom_images` map. An
/// empty catalog means presets only (custom_images map is empty).
///
/// Agents call this BEFORE `sandbox::create` when they don't already
/// know what's available on the deployment. Closes the "ask the operator
/// what custom images exist" gap that was the last unattended path on
/// the create-time S100 (image not in catalog) failure mode.
pub fn handle_catalog_list(
    _req: CatalogListRequest,
    cfg: &crate::sandbox_daemon::config::SandboxConfig,
) -> CatalogListResponse {
    let mut images: Vec<CatalogEntry> = PRESETS
        .iter()
        .map(|(name, oci_ref)| CatalogEntry {
            name: (*name).to_string(),
            oci_ref: (*oci_ref).to_string(),
            kind: CatalogEntryKind::Preset,
        })
        .collect();

    let mut custom: Vec<(&String, &String)> = cfg.custom_images.iter().collect();
    custom.sort_by(|a, b| a.0.cmp(b.0));
    for (name, oci_ref) in custom {
        images.push(CatalogEntry {
            name: name.clone(),
            oci_ref: oci_ref.clone(),
            kind: CatalogEntryKind::Custom,
        });
    }

    CatalogListResponse { images }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_preset_known() {
        assert_eq!(
            resolve_preset("python"),
            Some("docker.io/iiidev/python:latest")
        );
        assert_eq!(resolve_preset("node"), Some("docker.io/iiidev/node:latest"));
    }

    #[test]
    fn resolve_preset_unknown_is_none() {
        assert_eq!(resolve_preset("malicious"), None);
    }

    fn empty() -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    #[test]
    fn check_allowlist_allows_listed_preset() {
        let allow = vec!["python".into()];
        assert!(check_allowlist("python", &allow, &empty()).is_ok());
    }

    #[test]
    fn check_allowlist_rejects_unlisted_preset() {
        let allow = vec!["python".into()];
        let err = check_allowlist("node", &allow, &empty()).unwrap_err();
        assert_eq!(err.code().as_str(), "S100");
    }

    #[test]
    fn check_allowlist_rejects_non_preset_when_no_custom_images() {
        let allow = vec!["anything".into()];
        let err = check_allowlist("anything", &allow, &empty()).unwrap_err();
        assert_eq!(err.code().as_str(), "S100");
    }

    #[test]
    fn empty_allowlist_denies_all_even_presets() {
        let allow: Vec<String> = vec![];
        let err = check_allowlist("python", &allow, &empty()).unwrap_err();
        assert_eq!(err.code().as_str(), "S100");
    }

    #[test]
    fn resolve_image_presets_shadow_custom() {
        // If someone tries to redirect `python` via custom_images, the
        // preset must still win. Stops a misconfigured (or malicious)
        // config from silently swapping the trusted python rootfs for
        // an attacker-controlled ref.
        let mut custom = std::collections::HashMap::new();
        custom.insert("python".into(), "docker.io/evil/python:latest".into());
        let resolved = resolve_image("python", &custom).unwrap();
        assert_eq!(resolved, "docker.io/iiidev/python:latest");
    }

    #[test]
    fn check_allowlist_accepts_custom_image_when_listed() {
        let allow = vec!["my-app".into()];
        let mut custom = std::collections::HashMap::new();
        custom.insert("my-app".into(), "ghcr.io/acme/my-app:1.2.3".into());
        assert!(check_allowlist("my-app", &allow, &custom).is_ok());
    }

    #[test]
    fn check_allowlist_rejects_custom_image_when_not_in_allowlist() {
        // `my-app` is defined in custom_images but missing from the
        // allowlist. Presence in the catalog alone is not permission.
        let allow: Vec<String> = vec![];
        let mut custom = std::collections::HashMap::new();
        custom.insert("my-app".into(), "ghcr.io/acme/my-app:1".into());
        let err = check_allowlist("my-app", &allow, &custom).unwrap_err();
        assert_eq!(err.code().as_str(), "S100");
    }

    #[test]
    fn resolve_image_returns_custom_ref_for_non_preset() {
        let mut custom = std::collections::HashMap::new();
        custom.insert("my-app".into(), "ghcr.io/acme/my-app:1.2.3".into());
        let resolved = resolve_image("my-app", &custom).unwrap();
        assert_eq!(resolved, "ghcr.io/acme/my-app:1.2.3");
    }

    #[test]
    fn resolve_image_returns_none_for_unknown() {
        assert!(resolve_image("does-not-exist", &empty()).is_none());
    }

    fn make_cfg(
        custom: std::collections::HashMap<String, String>,
    ) -> crate::sandbox_daemon::config::SandboxConfig {
        // Build a SandboxConfig with safe defaults; only custom_images
        // matters for the catalog::list shape.
        let mut yaml = String::from("{}");
        if !custom.is_empty() {
            yaml.clear();
            yaml.push_str("custom_images:\n");
            let mut entries: Vec<(&String, &String)> = custom.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            for (k, v) in entries {
                yaml.push_str(&format!("  {k}: {v}\n"));
            }
        }
        serde_yaml::from_str(&yaml).expect("parse SandboxConfig")
    }

    #[test]
    fn catalog_list_with_empty_custom_returns_presets_only() {
        let cfg = make_cfg(empty());
        let resp = handle_catalog_list(CatalogListRequest::default(), &cfg);
        assert_eq!(resp.images.len(), 2);
        assert_eq!(resp.images[0].name, "python");
        assert!(matches!(resp.images[0].kind, CatalogEntryKind::Preset));
        assert_eq!(resp.images[1].name, "node");
        assert!(matches!(resp.images[1].kind, CatalogEntryKind::Preset));
    }

    #[test]
    fn catalog_list_appends_custom_after_presets_sorted_by_name() {
        let mut custom = std::collections::HashMap::new();
        custom.insert("rust-stable".into(), "ghcr.io/iii-hq/rust:1.85".into());
        custom.insert("pg17".into(), "docker.io/library/postgres:17".into());
        let cfg = make_cfg(custom);
        let resp = handle_catalog_list(CatalogListRequest::default(), &cfg);
        // 2 presets + 2 customs
        assert_eq!(resp.images.len(), 4);
        // Presets keep their order
        assert_eq!(resp.images[0].name, "python");
        assert_eq!(resp.images[1].name, "node");
        // Customs are sorted alphabetically for deterministic diffing
        assert_eq!(resp.images[2].name, "pg17");
        assert_eq!(resp.images[2].oci_ref, "docker.io/library/postgres:17");
        assert!(matches!(resp.images[2].kind, CatalogEntryKind::Custom));
        assert_eq!(resp.images[3].name, "rust-stable");
        assert!(matches!(resp.images[3].kind, CatalogEntryKind::Custom));
    }

    #[test]
    fn catalog_list_request_ignores_unknown_fields() {
        // Wire-compat: older callers that send {filter: "..."} or other
        // forward-compat fields must continue to deserialize.
        let json = serde_json::json!({ "filter": "anything", "page": 3 });
        let _: CatalogListRequest = serde_json::from_value(json).expect("parse");
    }
}
