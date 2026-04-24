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
}
