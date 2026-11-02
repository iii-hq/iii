//! The running iii engine version and the `min_iii_version` floor check.

use std::sync::OnceLock;

use scaffolder_core::runtime::tool::iii_tool;
use scaffolder_core::templates::version::{base_version, parse_version};
use semver::Version;

use super::builtin_defaults::ENGINE_BUILTIN_VERSION;

/// Version of the engine running this worker: the spawning engine's `III_ENGINE_VERSION`,
/// else the `iii` on PATH, else the version this binary shipped with.
pub fn running_iii_version() -> &'static Version {
    static RUNNING: OnceLock<Version> = OnceLock::new();
    RUNNING.get_or_init(|| {
        resolve_running_from(
            std::env::var("III_ENGINE_VERSION").ok(),
            iii_tool().get_version(),
        )
    })
}

fn resolve_running_from(env: Option<String>, cli: Option<String>) -> Version {
    [env, cli]
        .into_iter()
        .flatten()
        .find_map(|r| parse_version(&r).ok())
        .unwrap_or_else(|| Version::parse(ENGINE_BUILTIN_VERSION).expect("crate version is semver"))
}

/// True only for a plain `MAJOR.MINOR.PATCH` (no `v`, pre-release or build metadata).
pub fn valid_min_iii_version(min: &str) -> bool {
    Version::parse(min).is_ok_and(|v| v.pre.is_empty() && v.build.is_empty())
}

/// Compatible when `base(running) >= min`, so `0.26.0-rc.2` satisfies `0.26.0`.
pub fn check_min_iii_version(worker: &str, min: &str, running: &Version) -> Result<(), String> {
    if !valid_min_iii_version(min) {
        return Err(format!(
            "{worker}: min_iii_version \"{min}\" must be MAJOR.MINOR.PATCH (e.g. 0.26.0)"
        ));
    }
    let floor = Version::parse(min).expect("validated above");
    if base_version(running) >= floor {
        Ok(())
    } else {
        Err(format!(
            "{worker} requires iii >= {min}, running {running}; run: iii update"
        ))
    }
}

/// Checks the manifest's `min_iii_version`, if it declares one; `running` is only
/// evaluated then, so manifests without the key never probe the engine.
pub fn enforce_manifest_min_iii_version(
    worker: &str,
    doc: &serde_yaml::Value,
    running: impl FnOnce() -> Version,
) -> Result<(), String> {
    match doc.get("min_iii_version") {
        None | Some(serde_yaml::Value::Null) => Ok(()),
        Some(serde_yaml::Value::String(min)) => check_min_iii_version(worker, min, &running()),
        Some(other) => Err(format!(
            "{worker}: min_iii_version {other:?} must be a MAJOR.MINOR.PATCH string (e.g. 0.26.0)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn rc_engine_satisfies_release_floor() {
        assert!(check_min_iii_version("http", "0.26.0", &v("0.26.0-rc.2")).is_ok());
    }

    #[test]
    fn older_engine_is_refused_with_update_hint() {
        assert_eq!(
            check_min_iii_version("http", "0.26.0", &v("0.25.9")).unwrap_err(),
            "http requires iii >= 0.26.0, running 0.25.9; run: iii update"
        );
    }

    #[test]
    fn malformed_floor_is_refused() {
        assert_eq!(
            check_min_iii_version("http", "0.26", &v("0.26.0")).unwrap_err(),
            "http: min_iii_version \"0.26\" must be MAJOR.MINOR.PATCH (e.g. 0.26.0)"
        );
    }

    #[test]
    fn valid_min_iii_version_accepts_only_major_minor_patch() {
        assert!(valid_min_iii_version("0.26.0"));
        for bad in ["0.26", "v0.26.0", "0.26.0-rc.2", "0.26.0+b"] {
            assert!(!valid_min_iii_version(bad), "{bad}");
        }
    }

    #[test]
    fn running_version_prefers_env_then_cli_then_build_version() {
        let builtin = v(ENGINE_BUILTIN_VERSION);
        let s = |x: &str| Some(x.to_string());
        assert_eq!(resolve_running_from(s("0.26.1"), s("0.25.0")), v("0.26.1"));
        assert_eq!(resolve_running_from(s("garbage"), s("0.25.0")), v("0.25.0"));
        assert_eq!(resolve_running_from(None, s("0.25.0")), v("0.25.0"));
        assert_eq!(resolve_running_from(None, None), builtin);
        assert_eq!(resolve_running_from(s("garbage"), s("junk")), builtin);
    }

    #[test]
    fn absent_key_never_resolves_running_version() {
        let doc: serde_yaml::Value = serde_yaml::from_str("name: http\n").unwrap();
        assert!(enforce_manifest_min_iii_version("http", &doc, || panic!("probed")).is_ok());
    }

    #[test]
    fn absent_key_is_ok() {
        let doc: serde_yaml::Value = serde_yaml::from_str("name: http\n").unwrap();
        assert!(enforce_manifest_min_iii_version("http", &doc, || v("0.1.0")).is_ok());
    }

    #[test]
    fn declared_key_is_enforced() {
        let doc: serde_yaml::Value = serde_yaml::from_str("min_iii_version: 0.26.0\n").unwrap();
        assert!(enforce_manifest_min_iii_version("http", &doc, || v("0.25.0")).is_err());
        assert!(enforce_manifest_min_iii_version("http", &doc, || v("0.26.0")).is_ok());
    }
}
