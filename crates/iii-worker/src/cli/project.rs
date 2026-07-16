// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Project auto-detection and manifest loading for worker dev sessions.

use colored::Colorize;
use std::collections::{BTreeMap, HashMap};

pub const WORKER_MANIFEST: &str = "iii.worker.yaml";

/// First-gate check on `runtime.base_image` — rejects shell
/// metacharacters, whitespace, NUL, etc. Does not replace
/// `Reference::parse`'s own grammar check.
pub(crate) fn is_plausible_image_ref(s: &str) -> bool {
    if s.is_empty() || s.len() > 512 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '@' | '+'))
}

/// Supported values for `runtime.kind` in `iii.worker.yaml`.
///
/// - `typescript` / `javascript`: node-based (needs `package_manager: npm|yarn|pnpm|bun`)
/// - `bun`: bun is runtime + package manager, no sub-field needed
/// - `python`: CPython + pip/venv
/// - `rust`: rustup + cargo
///
/// Keep in sync with the match arms in `infer_scripts()` and
/// `oci::oci_image_for_kind()`.
const SUPPORTED_KINDS: &[&str] = &["typescript", "javascript", "bun", "python", "rust"];

// ─────────────────────────────────────────────────────────────────────────────
// Manifest schema source of truth: `cli::worker_manifest::WorkerManifest`.
// The typed struct defines the supported fields (mirroring the formal sections
// in docs/creating-workers/worker-manifest.mdx), carries the deprecated legacy
// fields (runtime.kind/package_manager/entry/language, top-level config/
// language/entry — warned, still honored), and catches unknown keys via
// `#[serde(flatten)]` maps. `validate_manifest_keys` / `warn_deprecated_
// manifest_keys` below are thin wrappers over it for the add/start paths.
// `env` and `dependencies` are arbitrary user-defined maps — their inner keys
// are never allowlist-checked.
// ─────────────────────────────────────────────────────────────────────────────

/// Max size of a local `iii.worker.yaml` we will read, mirroring the bundle
/// path's `MAX_BUNDLE_MANIFEST_BYTES`. Guards against a hostile or accidental
/// multi-GB manifest in a cloned worker dir being slurped into host memory.
pub const MAX_LOCAL_MANIFEST_BYTES: u64 = 64 * 1024;

pub struct ProjectInfo {
    pub name: String,
    /// Runtime identifier from `runtime.kind` in the manifest (formerly
    /// `runtime.language`). Drives three things: default OCI image in
    /// `oci::oci_image_for_kind`, script inference in `infer_scripts`,
    /// and validation. `runtime.language` is still read for backwards
    /// compatibility with older manifests and mapped onto this field.
    pub kind: Option<String>,
    pub setup_cmd: String,
    pub install_cmd: String,
    pub run_cmd: String,
    pub env: HashMap<String, String>,
    /// Optional override for the base OCI image used as rootfs. When
    /// `None`, the default from `oci::oci_image_for_kind` is used. Set
    /// via `runtime.base_image` in `iii.worker.yaml` — e.g.
    /// `base_image: oven/bun:1` to pin a specific bun version instead
    /// of the default `oven/bun:latest`.
    pub base_image: Option<String>,
}

impl ProjectInfo {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(ref k) = self.kind
            && !k.is_empty()
            && !SUPPORTED_KINDS.contains(&k.as_str())
        {
            return Err(format!(
                "unrecognized runtime kind '{}' in {} — supported: {}",
                k,
                WORKER_MANIFEST,
                SUPPORTED_KINDS.join(", ")
            ));
        }
        if self.run_cmd.is_empty() {
            return Err(format!(
                "no run command could be determined — check {} for missing `scripts.start` or `runtime` section",
                WORKER_MANIFEST
            ));
        }
        Ok(())
    }
}

pub fn infer_scripts(kind: &str, package_manager: &str, entry: &str) -> (String, String, String) {
    match (kind, package_manager) {
        // `kind: bun` — no package_manager needed; bun IS the package
        // manager. The oven/bun:latest rootfs ships bun preinstalled so
        // setup is empty.
        ("bun", _) => (
            String::new(),
            "bun install".to_string(),
            format!("bun run {}", entry),
        ),
        // Legacy: `kind: typescript` + `package_manager: bun`. Same
        // bun commands, but without the oven rootfs the entrypoint may
        // need `$HOME/.bun/bin` on PATH — keep the bootstrap.
        ("typescript", "bun") => (
            "curl -fsSL https://bun.sh/install | bash".to_string(),
            "export PATH=$HOME/.bun/bin:$PATH && bun install".to_string(),
            format!("export PATH=$HOME/.bun/bin:$PATH && bun {}", entry),
        ),
        ("typescript" | "javascript", "npm") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "npm install".to_string(),
            format!("npx tsx {}", entry),
        ),
        // Note: pnpm exec and yarn exec require tsx in devDependencies (unlike npx which auto-installs)
        ("typescript" | "javascript", "pnpm") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "pnpm install".to_string(),
            format!("pnpm exec tsx {}", entry),
        ),
        ("typescript" | "javascript", "yarn") => (
            "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".to_string(),
            "yarn install".to_string(),
            format!("yarn exec tsx {}", entry),
        ),
        ("typescript" | "javascript", other) => {
            eprintln!(
                "{} unrecognized package_manager '{}' for {} — supported: npm, yarn, pnpm, bun (or use `kind: bun` directly)",
                "error:".red(),
                other,
                kind
            );
            (String::new(), String::new(), String::new())
        },
        ("python", _) => (
            "command -v python3 >/dev/null || (apt-get update && apt-get install -y python3-venv python3-pip)".to_string(),
            "python3 -m venv .venv && .venv/bin/pip install -e .".to_string(),
            format!(".venv/bin/python -m {}", entry),
        ),
        // ponytail: --release on purpose, for disk not speed. A worker's
        // per-VM `target` dir is never shared (no CARGO_TARGET_DIR), so debug's
        // full debuginfo (130 MB binaries) + `incremental=true` churn pile up
        // per worker (MOT-3459 Defect 1). Release defaults to no debuginfo and
        // `incremental=false`. Must match the auto-detect path below and keep
        // build/run on the same profile (mismatch = a second full target tree).
        ("rust", _) => (
            "command -v cargo >/dev/null || (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y)".to_string(),
            "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo build --release".to_string(),
            "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo run --release".to_string(),
        ),
        _ => (String::new(), String::new(), entry.to_string()),
    }
}

pub fn load_project_info(path: &std::path::Path) -> Option<ProjectInfo> {
    let manifest_path = path.join(WORKER_MANIFEST);
    if manifest_path.exists() {
        return load_from_manifest(&manifest_path);
    }
    auto_detect_project(path)
}

/// Resolve `scripts.install` from a manifest's `scripts` section. Returns
/// the install command plus whether the key was *omitted* entirely.
/// An omitted key yields `("", true)` so the caller can warn that the
/// dependency install will be skipped; an explicit `install: ""`
/// is an intentional opt-out and reports `("", false)`.
fn resolve_install(scripts: &super::worker_manifest::ScriptsSection) -> (String, bool) {
    match &scripts.install {
        Some(v) => (v.trim().to_string(), false),
        None => (String::new(), true),
    }
}

/// The warning printed when `scripts.install` is omitted.
fn install_skipped_warning(manifest_path: &std::path::Path) -> String {
    format!(
        "{} {}: `scripts.install` omitted; dependency install will be skipped. \
         Add a `scripts.install` command (e.g. `npm install`, \
         `python3 -m venv .venv && .venv/bin/pip install -e .`, `cargo build`) \
         or your worker may fail at runtime with missing dependencies.",
        "warning:".yellow(),
        manifest_path.display()
    )
}

/// Read, size-cap, and parse an `iii.worker.yaml` exactly once.
///
/// Returns `Ok(None)` when the manifest is absent (the auto-detect path), so
/// callers can branch without a second `exists()` check. Returns `Err` when
/// the file exceeds [`MAX_LOCAL_MANIFEST_BYTES`] or fails to read/parse — the
/// size cap mirrors the bundle path and bounds every downstream read, since
/// callers thread the returned `Value` instead of re-reading.
pub fn read_manifest_doc(
    manifest_path: &std::path::Path,
) -> Result<Option<serde_yaml::Value>, String> {
    if !manifest_path.exists() {
        return Ok(None);
    }
    let len = std::fs::metadata(manifest_path)
        .map_err(|e| format!("cannot stat {}: {e}", manifest_path.display()))?
        .len();
    if len > MAX_LOCAL_MANIFEST_BYTES {
        return Err(format!(
            "{} is {len} bytes; iii.worker.yaml is capped at {MAX_LOCAL_MANIFEST_BYTES} bytes",
            manifest_path.display(),
        ));
    }
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let doc = serde_yaml::from_str(&content)
        .map_err(|e| format!("invalid YAML in {}: {e}", manifest_path.display()))?;
    Ok(Some(doc))
}

/// Strict key/shape validation, delegating to the typed
/// [`super::worker_manifest::WorkerManifest`] (the schema's single source of
/// truth). Returns `Err` listing every UNKNOWN key (typo / unsupported),
/// collected and sorted so a manifest with several mistakes surfaces them all
/// in one run, or an `invalid manifest shape` error when a section has the
/// wrong type (e.g. `runtime: node` where a mapping is required). Pure — emits
/// NO warnings — so it can run on both the `add` and the `start` paths (start
/// would otherwise re-warn about deprecated keys on every engine boot).
///
/// An empty manifest (`null`) or empty mapping returns `Ok(())`: there are no
/// keys to police. Missing `name` is a separate required-field rule —
/// [`require_manifest_name`], enforced on the add path. A non-mapping top
/// level (a list, a bare scalar) is a shape error.
pub fn validate_manifest_keys(
    doc: &serde_yaml::Value,
    manifest_path: &std::path::Path,
) -> Result<(), String> {
    if matches!(doc, serde_yaml::Value::Null) {
        return Ok(());
    }
    if doc.as_mapping().is_none() {
        return Err(format!(
            "{}: manifest top level must be a YAML mapping of fields, e.g. `name: my-worker`",
            manifest_path.display(),
        ));
    }
    let manifest = super::worker_manifest::WorkerManifest::from_value(doc)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let (unknown, _deprecated) = manifest.classify_keys();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(format!(
        "unknown key(s) in {}: [{}]. Supported fields are: name, description, \
         runtime.base_image, scripts.(setup|install|start), env, dependencies, \
         resources.(cpus|memory), plus the registry publish metadata keys \
         (iii, deploy, manifest, tags) which the engine accepts and ignores.",
        manifest_path.display(),
        unknown.join(", "),
    ))
}

/// Require a non-empty `name` on a parsed `iii.worker.yaml`. Add-path gate,
/// run right after [`validate_manifest_keys`] (which stays pure key/shape
/// policing). Without this, a nameless manifest fell through
/// `load_from_manifest → None` and was misreported as "No project manifest
/// detected" — a false diagnosis when the file plainly exists (caught by the
/// live DX audit). Deliberately NOT enforced on the start path: workers
/// added via auto-detect before this rule may sit next to a nameless
/// placeholder manifest, and start must not brick them.
pub fn require_manifest_name(
    doc: &serde_yaml::Value,
    manifest_path: &std::path::Path,
) -> Result<(), String> {
    if matches!(doc, serde_yaml::Value::Null) {
        return Err(format!(
            "{} is empty; at least `name` and `scripts.start` are required",
            manifest_path.display(),
        ));
    }
    let has_name = doc
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if has_name {
        return Ok(());
    }
    Err(format!(
        "{}: missing required field `name` (a single path segment, e.g. `name: my-worker`)",
        manifest_path.display(),
    ))
}

/// Emit a single batched DEPRECATION warning (gated by
/// `III_NO_DEPRECATION_WARN`) for keys the parser still honors but the docs no
/// longer define. Add-path only — the start path stays silent so engine boots
/// don't spam the warning. Side-effect only; never fails (shape errors are
/// `validate_manifest_keys`' job and have already gated the add by the time
/// this runs).
pub fn warn_deprecated_manifest_keys(doc: &serde_yaml::Value, manifest_path: &std::path::Path) {
    if std::env::var_os("III_NO_DEPRECATION_WARN").is_some() {
        return;
    }
    let Ok(manifest) = super::worker_manifest::WorkerManifest::from_value(doc) else {
        return;
    };
    let (_unknown, deprecated) = manifest.classify_keys();
    if deprecated.is_empty() {
        return;
    }
    // Only print the remediation bullet for a group that is actually present,
    // so a `config:`-only manifest isn't told how to migrate keys it lacks.
    let has_inference = deprecated.iter().any(|d| {
        matches!(
            d.as_str(),
            "runtime.kind"
                | "runtime.package_manager"
                | "runtime.entry"
                | "runtime.language"
                | "language"
                | "entry"
        )
    });
    let has_config = deprecated.iter().any(|d| d == "config");
    let mut bullets = String::new();
    if has_inference {
        bullets.push_str(
            "\n  - `runtime.kind`/`package_manager`/`entry`/`language` (and the legacy \
             top-level `language`/`entry`): define `scripts.setup`/`install`/`start` \
             explicitly instead.",
        );
    }
    if has_config {
        bullets.push_str(
            "\n  - `config`: set per-worker config directly in your project's \
             `config.yaml` entry for this worker.",
        );
    }
    eprintln!(
        "{} {}: deprecated manifest key(s) [{}]. Still honored for now but no \
         longer part of the documented schema; they will be removed in a \
         future version.{}\n  (set III_NO_DEPRECATION_WARN=1 to silence.)",
        "warning:".yellow(),
        manifest_path.display(),
        deprecated.join(", "),
        bullets,
    );
}

pub fn load_from_manifest(manifest_path: &std::path::Path) -> Option<ProjectInfo> {
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    // Typed view — the manifest schema's single source of truth. A shape
    // mismatch (e.g. `runtime: node`) yields None here; on the add/start
    // paths `validate_manifest_keys` has already surfaced the precise error.
    let manifest = super::worker_manifest::WorkerManifest::from_value(&doc).ok()?;
    let name = manifest.name.clone()?;

    let rt = manifest.runtime.clone().unwrap_or_default();
    // Prefer `runtime.kind`; fall back to the legacy `runtime.language` so
    // older manifests keep working. Deprecation warnings come from
    // `warn_deprecated_manifest_keys` on the `worker add` path.
    let kind = rt.kind.as_deref().or(rt.language.as_deref()).unwrap_or("");
    let package_manager = rt.package_manager.as_deref().unwrap_or("");
    let entry = rt.entry.as_deref().unwrap_or("");
    let base_image = rt
        .base_image
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| {
            let trimmed = s.trim();
            if !is_plausible_image_ref(trimmed) {
                eprintln!(
                    "{} {}: `runtime.base_image: {:?}` contains characters that don't \
                     belong in an OCI image reference (allowed: alphanumerics, `._-/:@`). \
                     Ignoring the override and falling back to the `kind` default.",
                    "warning:".yellow(),
                    manifest_path.display(),
                    trimmed
                );
                None
            } else {
                Some(trimmed.to_string())
            }
        });

    let (setup_cmd, install_cmd, run_cmd) = if let Some(scripts) = &manifest.scripts {
        let setup = scripts.setup.as_deref().unwrap_or("").trim().to_string();
        // `scripts.install` omitted means no dependency install runs.
        // That silently breaks workers whose code imports those deps
        // (e.g. ModuleNotFoundError at boot), so warn loudly.
        // An explicit `install: ""` is an intentional opt-out — no warning.
        let (install, install_omitted) = resolve_install(scripts);
        if install_omitted {
            eprintln!("{}", install_skipped_warning(manifest_path));
        }
        let start = scripts.start.as_deref().unwrap_or("").trim().to_string();
        (setup, install, start)
    } else {
        // `package_manager` is required for typescript/javascript
        // (we need to know whether to use npm/yarn/pnpm/bun); `bun`,
        // `python`, `rust` don't need it.
        let needs_pm = matches!(kind, "typescript" | "javascript");
        if needs_pm && package_manager.is_empty() {
            eprintln!(
                "{} cannot infer install/start for `{}` in {} without `runtime.package_manager`.\n\nhint: prefer defining explicit lifecycle scripts (these are the documented, supported fields):\n\n  scripts:\n    install: \"npm install\"\n    start: \"npx tsx {}\"\n\n(the legacy inference path via `runtime.package_manager: npm|yarn|pnpm|bun` is deprecated.)",
                "error:".red(),
                kind,
                manifest_path.display(),
                if entry.is_empty() {
                    "src/index.ts"
                } else {
                    entry
                },
            );
            return None;
        }
        infer_scripts(kind, package_manager, entry)
    };

    let mut env = HashMap::new();
    if let Some(env_map) = &manifest.env {
        for (key, val) in env_map {
            if key != "III_URL" && key != "III_ENGINE_URL" {
                env.insert(key.clone(), val.clone());
            }
        }
    }

    Some(ProjectInfo {
        name,
        kind: Some(kind.to_string()),
        setup_cmd,
        install_cmd,
        run_cmd,
        env,
        base_image,
    })
}

/// Read only the `dependencies:` block from an `iii.worker.yaml` manifest.
/// Returns an empty map when the file is missing (authors without deps work
/// unchanged) or the field is absent/null. Returns `Err` only when the file
/// exists but is malformed YAML or the `dependencies:` block is invalid.
pub fn load_manifest_dependencies(
    manifest_path: &std::path::Path,
) -> Result<BTreeMap<String, String>, String> {
    if !manifest_path.exists() {
        return Ok(BTreeMap::new());
    }
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("failed to read {}: {e}", manifest_path.display()))?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&content)
        .map_err(|e| format!("failed to parse {}: {e}", manifest_path.display()))?;
    let self_name = doc.get("name").and_then(|v| v.as_str());
    super::worker_manifest_deps::parse_dependencies(&doc, self_name)
}

/// Same as [`load_manifest_dependencies`] but operates on an already-parsed
/// document, so callers that read the manifest once can thread it through
/// instead of re-reading from disk.
pub fn manifest_dependencies_from_doc(
    doc: &serde_yaml::Value,
) -> Result<BTreeMap<String, String>, String> {
    let self_name = doc.get("name").and_then(|v| v.as_str());
    super::worker_manifest_deps::parse_dependencies(doc, self_name)
}

pub fn auto_detect_project(path: &std::path::Path) -> Option<ProjectInfo> {
    let info = if path.join("package.json").exists() {
        if path.join("bun.lock").exists() || path.join("bun.lockb").exists() {
            // A bun lockfile on disk means the user is committed to
            // bun. Pick `kind: bun` so the oven/bun:latest rootfs gets
            // used — no bun bootstrap script needed.
            ProjectInfo {
                name: "bun".into(),
                kind: Some("bun".into()),
                setup_cmd: String::new(),
                install_cmd: "bun install".into(),
                run_cmd: "bun run dev".into(),
                env: HashMap::new(),
                base_image: None,
            }
        } else {
            ProjectInfo {
                name: "node (npm)".into(),
                kind: Some("typescript".into()),
                setup_cmd: "command -v node >/dev/null || (curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs)".into(),
                install_cmd: "npm install".into(),
                run_cmd: "npm run dev".into(),
                env: HashMap::new(),
                base_image: None,
            }
        }
    } else if path.join("Cargo.toml").exists() {
        ProjectInfo {
            name: "rust".into(),
            kind: Some("rust".into()),
            setup_cmd: "command -v cargo >/dev/null || (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y)".into(),
            install_cmd: "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo build --release".into(),
            run_cmd: "[ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"; cargo run --release".into(),
            env: HashMap::new(),
            base_image: None,
        }
    } else if path.join("pyproject.toml").exists() || path.join("requirements.txt").exists() {
        ProjectInfo {
            name: "python".into(),
            kind: Some("python".into()),
            setup_cmd: "command -v python3 >/dev/null || (apt-get update && apt-get install -y python3 python3-pip python3-venv)".into(),
            install_cmd: "python3 -m pip install -e .".into(),
            run_cmd: "python3 -m iii".into(),
            env: HashMap::new(),
            base_image: None,
        }
    } else {
        return None;
    };
    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_manifest_with_explicit_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: my-worker
scripts:
  setup: "apt-get update"
  install: "npm install"
  start: "node server.js"
env:
  FOO: bar
  III_URL: skip
  III_ENGINE_URL: skip
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.name, "my-worker");
        assert_eq!(info.setup_cmd, "apt-get update");
        assert_eq!(info.install_cmd, "npm install");
        assert_eq!(info.run_cmd, "node server.js");
        assert_eq!(info.env.get("FOO").unwrap(), "bar");
        assert!(!info.env.contains_key("III_URL"));
        assert!(!info.env.contains_key("III_ENGINE_URL"));
    }

    // `scripts.start` without `scripts.install` leaves install
    // empty (deps skipped). A warning is emitted to stderr; install_cmd
    // stays empty so the author's explicit `scripts` block is honored.
    #[test]
    fn omitted_install_skips_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: my-worker
runtime:
  kind: python
  entry: worker
scripts:
  start: "python3 worker.py"
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.run_cmd, "python3 worker.py");
        assert_eq!(info.install_cmd, "");
    }

    // The omitted/opt-out/explicit decision that drives the warning.
    #[test]
    fn resolve_install_reports_omission() {
        use super::super::worker_manifest::ScriptsSection;
        fn scripts(yaml: &str) -> ScriptsSection {
            serde_yaml::from_str(yaml).unwrap()
        }

        assert_eq!(
            resolve_install(&scripts("start: run")),
            (String::new(), true)
        );
        assert_eq!(
            resolve_install(&scripts("install: \"\"")),
            (String::new(), false)
        );
        assert_eq!(
            resolve_install(&scripts("install: npm install")),
            ("npm install".to_string(), false)
        );
    }

    // The skipped-install warning text is generic (not python-specific)
    // and names the manifest path.
    #[test]
    fn install_skipped_warning_is_generic() {
        let msg = install_skipped_warning(std::path::Path::new("/tmp/iii.worker.yaml"));
        assert!(msg.contains("warning:"));
        assert!(msg.contains("`scripts.install` omitted"));
        assert!(msg.contains("dependency install will be skipped"));
        assert!(msg.contains("/tmp/iii.worker.yaml"));
        // mentions all three runtimes, not just python
        assert!(msg.contains("npm install"));
        assert!(msg.contains("pip install"));
        assert!(msg.contains("cargo build"));
    }

    // An explicit `install: ""` is an intentional opt-out (no warning),
    // and resolves to empty just like before.
    #[test]
    fn explicit_empty_install_opts_out() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: my-worker
runtime:
  kind: python
  entry: worker
scripts:
  install: ""
  start: "python3 worker.py"
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.install_cmd, "");
        assert_eq!(info.run_cmd, "python3 worker.py");
    }

    #[test]
    fn load_manifest_auto_detects_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: my-bun-worker
runtime:
  kind: bun
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.name, "my-bun-worker");
        assert_eq!(info.kind.as_deref(), Some("bun"));
        assert!(info.install_cmd.contains("bun install"));
        assert!(info.run_cmd.contains("bun run src/index.ts"));
    }

    #[test]
    fn load_manifest_parses_base_image_override() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: bun-worker
runtime:
  kind: bun
  base_image: oven/bun:1
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.base_image.as_deref(), Some("oven/bun:1"));
    }

    #[test]
    fn load_manifest_base_image_absent_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: default-worker
runtime:
  kind: typescript
  package_manager: npm
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert!(info.base_image.is_none());
    }

    #[test]
    fn load_manifest_base_image_blank_is_none() {
        // Empty / whitespace-only values are treated as "not set" so a
        // stray `base_image: ""` doesn't get slugified into an empty
        // rootfs dir name downstream.
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: blank-worker
runtime:
  kind: typescript
  package_manager: npm
  base_image: "   "
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert!(info.base_image.is_none());
    }

    #[test]
    fn load_manifest_filters_engine_url_env() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: env-test
env:
  FOO: bar
  III_URL: skip
  III_ENGINE_URL: skip
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.env.get("FOO").unwrap(), "bar");
        assert!(!info.env.contains_key("III_URL"));
        assert!(!info.env.contains_key("III_ENGINE_URL"));
    }

    #[test]
    fn load_manifest_accepts_legacy_language_field() {
        // Backwards compat: old manifests with `runtime.language`
        // keep working and parse equivalently. The deprecation warning
        // now comes from `validate_manifest_keys` on the `worker add`
        // path, not from `load_from_manifest`.
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: legacy-worker
runtime:
  language: typescript
  package_manager: bun
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.kind.as_deref(), Some("typescript"));
        assert!(info.install_cmd.contains("bun install"));
    }

    #[test]
    fn load_manifest_kind_wins_over_legacy_language() {
        // If both are set, `kind` wins — lets a user migrate
        // incrementally by adding `kind` before removing `language`.
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: migrating-worker
runtime:
  kind: bun
  language: typescript
  package_manager: npm
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.kind.as_deref(), Some("bun"));
    }

    #[test]
    fn infer_scripts_python() {
        let (setup, install, run) = infer_scripts("python", "pip", "my_module");
        assert!(setup.contains("python3-venv") || setup.contains("python3"));
        assert!(install.contains(".venv/bin/pip"));
        assert!(run.contains(".venv/bin/python -m my_module"));
    }

    #[test]
    fn infer_scripts_rust() {
        let (setup, install, run) = infer_scripts("rust", "cargo", "src/main.rs");
        assert!(setup.contains("rustup"));
        // MOT-3459 Defect 1: the inferred build must be --release (no debuginfo,
        // no incremental) so per-worker `target` dirs don't bloat, and build/run
        // must share the profile or `cargo run` rebuilds a second debug tree.
        assert!(install.contains("cargo build --release"));
        assert!(run.contains("cargo run --release"));
    }

    #[test]
    fn infer_scripts_bun_as_kind() {
        // New path: `kind: bun` produces native bun commands with no
        // bootstrap (the oven/bun rootfs ships bun preinstalled).
        let (setup, install, run) = infer_scripts("bun", "", "src/index.ts");
        assert!(setup.is_empty(), "bun rootfs has bun preinstalled");
        assert!(install.contains("bun install"));
        assert!(run.contains("bun run src/index.ts"));
    }

    #[test]
    fn infer_scripts_typescript_plus_bun_pm() {
        // Legacy path: `kind: typescript` + `package_manager: bun`
        // still works, bootstraps bun via curl since the default
        // node rootfs doesn't ship it.
        let (setup, install, run) = infer_scripts("typescript", "bun", "src/index.ts");
        assert!(setup.contains("bun.sh/install"));
        assert!(install.contains("bun install"));
        assert!(run.contains("bun src/index.ts"));
    }

    #[test]
    fn infer_scripts_npm() {
        let (setup, install, run) = infer_scripts("typescript", "npm", "src/index.ts");
        assert!(setup.contains("nodejs") || setup.contains("nodesource"));
        assert!(install.contains("npm install"));
        assert!(run.contains("npx tsx src/index.ts"));
    }

    #[test]
    fn auto_detect_project_node_npm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "node (npm)");
        assert_eq!(info.kind.as_deref(), Some("typescript"));
        assert!(info.install_cmd.contains("npm"));
    }

    #[test]
    fn auto_detect_project_bun_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("bun.lock"), "").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "bun");
        assert_eq!(info.kind.as_deref(), Some("bun"));
        assert!(info.run_cmd.contains("bun"));
    }

    #[test]
    fn auto_detect_project_rust() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "rust");
        assert_eq!(info.kind.as_deref(), Some("rust"));
    }

    #[test]
    fn auto_detect_project_python() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]").unwrap();
        let info = auto_detect_project(dir.path()).unwrap();
        assert_eq!(info.name, "python");
        assert_eq!(info.kind.as_deref(), Some("python"));
    }

    #[test]
    fn auto_detect_project_unknown_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(auto_detect_project(dir.path()).is_none());
    }

    #[test]
    fn load_project_info_prefers_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let yaml = r#"
name: manifest-worker
runtime:
  kind: typescript
  package_manager: npm
  entry: src/index.ts
"#;
        std::fs::write(dir.path().join("iii.worker.yaml"), yaml).unwrap();
        let info = load_project_info(dir.path()).unwrap();
        assert_eq!(info.name, "manifest-worker");
    }

    #[test]
    fn infer_scripts_pnpm() {
        let (setup, install, run) = infer_scripts("typescript", "pnpm", "src/index.ts");
        assert!(setup.contains("nodejs") || setup.contains("nodesource"));
        assert!(install.contains("pnpm install"));
        assert!(run.contains("pnpm exec tsx src/index.ts"));
    }

    #[test]
    fn infer_scripts_yarn() {
        let (setup, install, run) = infer_scripts("typescript", "yarn", "src/index.ts");
        assert!(setup.contains("nodejs") || setup.contains("nodesource"));
        assert!(install.contains("yarn install"));
        assert!(run.contains("yarn exec tsx src/index.ts"));
    }

    #[test]
    fn infer_scripts_typescript_unknown_pm() {
        let (setup, install, run) = infer_scripts("typescript", "deno", "src/index.ts");
        assert!(setup.is_empty());
        assert!(install.is_empty());
        assert!(run.is_empty());
    }

    #[test]
    fn load_manifest_missing_package_manager_for_typescript() {
        // typescript/javascript require package_manager; bun/python/
        // rust don't. Verify the error only fires where it should.
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: broken-worker
runtime:
  kind: typescript
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        assert!(load_from_manifest(&manifest_path).is_none());
    }

    #[test]
    fn load_manifest_bun_kind_without_package_manager_works() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: bun-only
runtime:
  kind: bun
  entry: src/index.ts
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let info = load_from_manifest(&manifest_path).unwrap();
        assert_eq!(info.kind.as_deref(), Some("bun"));
        assert!(info.run_cmd.contains("bun run"));
    }

    #[test]
    fn validate_empty_run_cmd() {
        let project = ProjectInfo {
            name: "test".into(),
            kind: Some("typescript".into()),
            setup_cmd: String::new(),
            install_cmd: String::new(),
            run_cmd: String::new(),
            env: HashMap::new(),
            base_image: None,
        };
        assert!(project.validate().is_err());
    }

    #[test]
    fn validate_unrecognized_kind() {
        let project = ProjectInfo {
            name: "test".into(),
            kind: Some("typescirpt".into()),
            setup_cmd: String::new(),
            install_cmd: String::new(),
            run_cmd: "node index.js".into(),
            env: HashMap::new(),
            base_image: None,
        };
        let err = project.validate().unwrap_err();
        assert!(err.contains("unrecognized runtime kind"));
        assert!(err.contains("typescirpt"));
    }

    #[test]
    fn validate_accepts_bun_kind() {
        let project = ProjectInfo {
            name: "test".into(),
            kind: Some("bun".into()),
            setup_cmd: String::new(),
            install_cmd: "bun install".into(),
            run_cmd: "bun run src/index.ts".into(),
            env: HashMap::new(),
            base_image: None,
        };
        assert!(project.validate().is_ok());
    }

    #[test]
    fn load_manifest_dependencies_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let yaml = r#"
name: caller
runtime:
  kind: bun
  entry: src/index.ts
dependencies:
  math-worker: "^0.1.0"
"#;
        std::fs::write(&manifest_path, yaml).unwrap();
        let deps = super::load_manifest_dependencies(&manifest_path).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps.get("math-worker").unwrap(), "^0.1.0");
    }

    #[test]
    fn load_manifest_dependencies_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("iii.worker.yaml");
        let deps = super::load_manifest_dependencies(&manifest_path).unwrap();
        assert!(deps.is_empty());
    }

    // ── validate_manifest_keys ──────────────────────────────────────────────

    fn doc(s: &str) -> serde_yaml::Value {
        serde_yaml::from_str(s).unwrap()
    }

    fn dummy_manifest_path() -> std::path::PathBuf {
        std::path::PathBuf::from("/tmp/iii.worker.yaml")
    }

    #[test]
    fn validate_keys_accepts_fully_documented_manifest() {
        let d = doc(r#"
name: my-worker
description: A worker that does things.
runtime:
  base_image: oven/bun:1
scripts:
  setup: "apt-get update"
  install: "npm install"
  start: "node server.js"
env:
  FOO: bar
dependencies:
  iii-http: "^0.19"
tags:
  - http
  - api
resources:
  cpus: 2
  memory: 2048
"#);
        assert!(validate_manifest_keys(&d, &dummy_manifest_path()).is_ok());
    }

    #[test]
    fn validate_keys_supports_description() {
        // `description` is read by managed.rs and documented in workers.mdx;
        // it must be a supported top-level field, not an unknown key.
        let d = doc("name: w\ndescription: does things\n");
        assert!(validate_manifest_keys(&d, &dummy_manifest_path()).is_ok());
        let m = super::super::worker_manifest::WorkerManifest::from_value(&d).unwrap();
        let (unknown, deprecated) = m.classify_keys();
        assert!(
            unknown.is_empty(),
            "description must not be unknown: {unknown:?}"
        );
        assert!(
            !deprecated.iter().any(|d| d == "description"),
            "description must not be deprecated: {deprecated:?}"
        );
    }

    #[test]
    fn validate_keys_accepts_publish_metadata_keys() {
        // iii/deploy/manifest/tags are release-CI metadata carried by workers
        // in the workers repo; `worker add` must accept them (neither unknown
        // nor deprecated) so one manifest satisfies both validators.
        let d = doc("iii: v1\nname: w\ndeploy: binary\nmanifest: Cargo.toml\ntags:\n  - api\n");
        assert!(validate_manifest_keys(&d, &dummy_manifest_path()).is_ok());
        let m = super::super::worker_manifest::WorkerManifest::from_value(&d).unwrap();
        let (unknown, deprecated) = m.classify_keys();
        assert!(unknown.is_empty(), "got: {unknown:?}");
        for key in ["iii", "deploy", "manifest", "tags"] {
            assert!(
                !deprecated.iter().any(|x| x == key),
                "`{key}` must not be deprecated: {deprecated:?}"
            );
        }
    }

    #[test]
    fn validate_keys_rejects_unknown_top_level() {
        let d = doc("name: w\nruntimee:\n  kind: bun\n");
        let err = validate_manifest_keys(&d, &dummy_manifest_path()).unwrap_err();
        assert!(err.contains("runtimee"), "got: {err}");
    }

    #[test]
    fn validate_keys_rejects_unknown_nested_keys() {
        let scripts = doc("name: w\nscripts:\n  instal: \"npm i\"\n  start: \"x\"\n");
        let err = validate_manifest_keys(&scripts, &dummy_manifest_path()).unwrap_err();
        assert!(err.contains("scripts.instal"), "got: {err}");

        let runtime = doc("name: w\nruntime:\n  foo: bar\n");
        let err = validate_manifest_keys(&runtime, &dummy_manifest_path()).unwrap_err();
        assert!(err.contains("runtime.foo"), "got: {err}");

        let resources = doc("name: w\nresources:\n  disk: 10\n");
        let err = validate_manifest_keys(&resources, &dummy_manifest_path()).unwrap_err();
        assert!(err.contains("resources.disk"), "got: {err}");
    }

    #[test]
    fn validate_keys_collects_multiple_unknowns_sorted() {
        // Document order is zzz_bad then aaa_bad; the error must list BOTH and
        // sort them (aaa_bad before zzz_bad), proving collect-all + sort.
        let d = doc("name: w\nzzz_bad:\n  x: 1\naaa_bad:\n  y: 2\n");
        let err = validate_manifest_keys(&d, &dummy_manifest_path()).unwrap_err();
        let first = err.find("aaa_bad").expect("aaa_bad listed");
        let second = err.find("zzz_bad").expect("zzz_bad listed");
        assert!(first < second, "expected sorted order, got: {err}");
    }

    #[test]
    fn validate_keys_deprecated_keys_are_not_errors() {
        // Deprecated keys (runtime.* inference keys, top-level config, and the
        // legacy top-level language/entry) classify as deprecated, never
        // unknown, so strict validation passes. The warning is a separate
        // concern (warn_deprecated_manifest_keys).
        for (manifest, expect_dep) in [
            ("name: w\nruntime:\n  kind: bun\n", "runtime.kind"),
            (
                "name: w\nruntime:\n  package_manager: npm\n",
                "runtime.package_manager",
            ),
            (
                "name: w\nruntime:\n  entry: src/index.ts\n",
                "runtime.entry",
            ),
            (
                "name: w\nruntime:\n  language: typescript\n",
                "runtime.language",
            ),
            ("name: w\nconfig:\n  port: 3000\n", "config"),
            ("name: w\nlanguage: typescript\n", "language"),
            ("name: w\nentry: src/index.ts\n", "entry"),
        ] {
            let d = doc(manifest);
            assert!(
                validate_manifest_keys(&d, &dummy_manifest_path()).is_ok(),
                "deprecated key must not error: {manifest}"
            );
            let m = super::super::worker_manifest::WorkerManifest::from_value(&d).unwrap();
            let (unknown, deprecated) = m.classify_keys();
            assert!(
                unknown.is_empty(),
                "no unknowns expected for {manifest}: {unknown:?}"
            );
            assert!(
                deprecated.iter().any(|x| x == expect_dep),
                "expected `{expect_dep}` deprecated for {manifest}: {deprecated:?}"
            );
            // warn path must not panic regardless of which keys are present.
            warn_deprecated_manifest_keys(&d, &dummy_manifest_path());
        }
    }

    #[test]
    fn validate_keys_treats_env_and_dependencies_as_opaque() {
        let d = doc("name: w\nenv:\n  MY_WEIRD_KEY: v\n  ANYTHING_GOES: w\n\
             dependencies:\n  some-worker: \"^1\"\n  another-one: \"~2\"\n");
        assert!(validate_manifest_keys(&d, &dummy_manifest_path()).is_ok());
    }

    #[test]
    fn require_manifest_name_enforces_the_required_field() {
        let path = dummy_manifest_path();
        assert!(require_manifest_name(&doc("name: w\n"), &path).is_ok());

        for (manifest, label) in [
            ("scripts:\n  start: x\n", "missing name"),
            ("name: \"\"\nscripts:\n  start: x\n", "empty name"),
            ("name: \"   \"\n", "whitespace name"),
        ] {
            let err = require_manifest_name(&doc(manifest), &path).unwrap_err();
            assert!(
                err.contains("missing required field `name`"),
                "{label}: got {err}"
            );
        }

        // Empty file: the message says the manifest is empty rather than
        // pretending the file was never found.
        let err = require_manifest_name(&doc(""), &path).unwrap_err();
        assert!(err.contains("is empty"), "got: {err}");
    }

    #[test]
    fn validate_keys_empty_is_ok_non_mapping_is_shape_error() {
        // Empty file (null) and empty mapping: nothing to police; missing
        // `name` is require_manifest_name's job on the add path.
        assert!(validate_manifest_keys(&doc("{}"), &dummy_manifest_path()).is_ok());
        assert!(validate_manifest_keys(&doc(""), &dummy_manifest_path()).is_ok());
        // A list or bare scalar was never a functional manifest — with the
        // typed WorkerManifest it is now a clear shape error instead of being
        // silently waved through to a confusing downstream failure.
        let err = validate_manifest_keys(&doc("- a\n- b\n"), &dummy_manifest_path()).unwrap_err();
        assert!(err.contains("mapping"), "got: {err}");
        let err =
            validate_manifest_keys(&doc("\"just a string\""), &dummy_manifest_path()).unwrap_err();
        assert!(err.contains("mapping"), "got: {err}");
    }

    #[test]
    fn validate_keys_runtime_scalar_is_shape_error() {
        // `runtime: node` used to be silently skipped (the old key-walker
        // couldn't express type errors). The typed manifest reports it as a
        // clear shape error instead of letting the add fail later with the
        // confusing "no run command could be determined".
        let d = doc("name: w\nruntime: node\n");
        let err = validate_manifest_keys(&d, &dummy_manifest_path()).unwrap_err();
        assert!(err.contains("invalid manifest shape"), "got: {err}");
    }

    #[test]
    fn validate_keys_unknown_errors_even_with_deprecated_present() {
        // The deprecation-collection path must not swallow the hard error when
        // both a deprecated and an unknown key are present. (The interaction
        // with III_NO_DEPRECATION_WARN is covered separately below.)
        let d = doc("name: w\nconfig:\n  port: 3000\nruntimee:\n  kind: bun\n");
        let err = validate_manifest_keys(&d, &dummy_manifest_path()).unwrap_err();
        assert!(err.contains("runtimee"), "got: {err}");
    }

    /// Serializes the two tests that mutate the process-global
    /// `III_NO_DEPRECATION_WARN` env var so they don't race each other (or
    /// other tests that read it) under cargo's parallel test runner.
    static DEPRECATION_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn env_gate_silences_warning_but_validation_still_errors() {
        let _g = DEPRECATION_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // SAFETY: serialized via DEPRECATION_ENV_LOCK and restored before any
        // assertion (so a panic can't leak the var into other tests).
        unsafe { std::env::set_var("III_NO_DEPRECATION_WARN", "1") };
        // warn path is a no-op under the gate (can't observe stderr in-process,
        // but it must run without panicking).
        warn_deprecated_manifest_keys(
            &doc("name: w\nconfig:\n  port: 1\n"),
            &dummy_manifest_path(),
        );
        // The hard error is independent of the env var.
        let unknown_err =
            validate_manifest_keys(&doc("name: w\nruntimee:\n  x: 1\n"), &dummy_manifest_path())
                .is_err();
        unsafe { std::env::remove_var("III_NO_DEPRECATION_WARN") };

        assert!(
            unknown_err,
            "III_NO_DEPRECATION_WARN must never suppress the unknown-key error"
        );
    }

    #[test]
    fn warn_returns_silently_when_manifest_shape_is_invalid() {
        let _g = DEPRECATION_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // SAFETY: serialized via DEPRECATION_ENV_LOCK; the env gate must be
        // open or the function returns before reaching the typed-parse branch.
        unsafe { std::env::remove_var("III_NO_DEPRECATION_WARN") };
        let d = doc("name: w\nruntime: node\n");
        assert!(
            crate::cli::worker_manifest::WorkerManifest::from_value(&d).is_err(),
            "precondition: `runtime: node` must fail the typed parse"
        );

        // Shape errors are validate_manifest_keys' job: the warn helper must
        // bail out of the let-else without panicking or classifying keys.
        warn_deprecated_manifest_keys(&d, &dummy_manifest_path());
    }

    #[test]
    fn validate_keys_sanitizes_control_chars_in_echoed_keys() {
        // A hostile third-party manifest must not inject terminal escapes
        // through an unknown key name. Build the key programmatically so the
        // ESC byte is exact regardless of YAML escaping rules.
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::from("name"),
            serde_yaml::Value::from("w"),
        );
        m.insert(
            serde_yaml::Value::from("ev\u{1b}\u{7}il"),
            serde_yaml::Value::from(1),
        );
        let d = serde_yaml::Value::Mapping(m);
        let err = validate_manifest_keys(&d, &dummy_manifest_path()).unwrap_err();
        assert!(
            !err.contains('\u{1b}') && !err.contains('\u{7}'),
            "control bytes must be stripped from echoed key: {err:?}"
        );
        assert!(
            err.contains("evil"),
            "printable key text should survive sanitization: {err}"
        );
    }

    #[test]
    fn read_manifest_doc_absent_is_ok_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(WORKER_MANIFEST);
        assert!(matches!(read_manifest_doc(&path), Ok(None)));
    }

    #[test]
    fn read_manifest_doc_valid_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(WORKER_MANIFEST);
        std::fs::write(&path, "name: w\nscripts:\n  start: x\n").unwrap();
        let doc = read_manifest_doc(&path).unwrap().expect("Some");
        assert_eq!(doc.get("name").and_then(|v| v.as_str()), Some("w"));
    }

    #[test]
    fn read_manifest_doc_malformed_is_err() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(WORKER_MANIFEST);
        std::fs::write(&path, "name: w\n  : : bad\n:\n").unwrap();
        let err = read_manifest_doc(&path).unwrap_err();
        assert!(err.contains("invalid YAML"), "got: {err}");
    }

    #[test]
    fn read_manifest_doc_oversize_is_err() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(WORKER_MANIFEST);
        // Valid YAML, but larger than the cap.
        let big = format!(
            "name: w\nbloat: \"{}\"\n",
            "a".repeat(MAX_LOCAL_MANIFEST_BYTES as usize)
        );
        std::fs::write(&path, big).unwrap();
        let err = read_manifest_doc(&path).unwrap_err();
        assert!(err.contains("capped at"), "got: {err}");
    }
}
