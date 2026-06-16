// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Typed `iii.worker.yaml` manifest.
//!
//! Single source of truth for the manifest schema: the [`WorkerManifest`]
//! struct drives (a) key classification for `worker add` validation
//! (`project::validate_manifest_keys` / `warn_deprecated_manifest_keys`
//! delegate here), (b) the machine-readable JSON Schema served by
//! `worker::schema { function_id: "iii.worker.yaml" }`, and (c) the dry-run
//! `worker::validate` trigger so an LLM can author → check → `worker::add`
//! without a failed install.
//!
//! Design notes:
//! - Deprecated fields are REAL typed fields (so readers stay clean and the
//!   schema can describe them) while every section carries a
//!   `#[serde(flatten)]` catch-all map — non-empty catch-all = unknown keys =
//!   hard error at add time. This preserves the warn-vs-reject split that
//!   `#[serde(deny_unknown_fields)]` cannot express.
//! - Parsing goes Value-first then `from_value`: serde_yaml 0.9 detects
//!   duplicate mapping keys only at the `Value` layer (typed maps silently
//!   last-win), so the two-step keeps duplicate keys a hard error.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

use super::project::{MAX_LOCAL_MANIFEST_BYTES, is_plausible_image_ref};

/// Strip control characters from a manifest key before it is echoed into a
/// terminal-bound error or warning. Manifest keys are third-party (users run
/// `iii worker add ./cloned-worker`), so an unknown key like `"\x1b]2;…\x07"`
/// must not reach the user's terminal raw. Mirrors the established
/// terminal-sanitization pattern in `engine/src/cli_trigger/help.rs`.
/// `is_control` covers only Cc, so the invisible Cf-category spoofing chars
/// (bidi overrides, zero-widths) are filtered explicitly — U+202E alone can
/// visually reverse an echoed key and disguise what the error points at.
pub(crate) fn sanitize_key(s: &str) -> String {
    fn is_invisible_format_char(c: char) -> bool {
        matches!(c,
            '\u{200b}'..='\u{200f}' // zero-widths + LRM/RLM
            | '\u{202a}'..='\u{202e}' // bidi embeddings/overrides
            | '\u{2066}'..='\u{2069}' // bidi isolates
            | '\u{061c}' // Arabic letter mark
            | '\u{feff}' // BOM / zero-width no-break space
        )
    }
    s.chars()
        .filter(|c| !c.is_control() && !is_invisible_format_char(*c))
        .collect()
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[schemars(
    description = "The `iii.worker.yaml` manifest at a worker project's root. Tells iii how to provision, install, and start the worker. Required for local-path `worker::add`. Unknown keys are rejected at add time; deprecated keys warn but still work."
)]
pub struct WorkerManifest {
    /// Required. Worker id; a single path segment (alphanumerics, '-', '_', '.').
    #[schemars(
        description = "Required. Worker id; must be a single path segment (alphanumerics, '-', '_', '.')."
    )]
    pub name: Option<String>,

    #[schemars(
        description = "Optional one-line human/LLM-readable summary; shown when installing or inspecting the worker."
    )]
    pub description: Option<String>,

    #[schemars(
        description = "Optional runtime details. The only supported sub-field is `base_image`; the rest are deprecated legacy inference knobs."
    )]
    pub runtime: Option<RuntimeSection>,

    #[schemars(
        description = "Lifecycle scripts: `start` runs the worker (required in practice), `install` installs dependencies, `setup` provisions the sandbox once."
    )]
    pub scripts: Option<ScriptsSection>,

    #[schemars(
        description = "Optional string->string env vars injected into the worker process. III_URL / III_ENGINE_URL are set by the engine and ignored here."
    )]
    pub env: Option<BTreeMap<String, String>>,

    #[schemars(
        description = "Optional map of other-worker-name -> semver range, resolved against the registry and installed before this worker, e.g. { iii-state: \"^0.19\" }. No self-references."
    )]
    pub dependencies: Option<BTreeMap<String, String>>,

    #[schemars(
        description = "Optional sandbox sizing: `cpus` (default 2) and `memory` in MiB (default 2048)."
    )]
    pub resources: Option<ResourcesSection>,

    #[schemars(
        with = "Option<JsonValue>",
        description = "DEPRECATED — set per-worker config directly in your project's config.yaml entry instead. Copied verbatim into config.yaml at add time."
    )]
    pub config: Option<YamlValue>,

    #[schemars(
        description = "DEPRECATED legacy scaffolder field — use explicit `scripts.start` instead."
    )]
    pub language: Option<String>,

    #[schemars(
        description = "DEPRECATED legacy scaffolder field — use explicit `scripts.start` instead."
    )]
    pub entry: Option<String>,

    #[serde(flatten)]
    #[schemars(skip)]
    pub unknown: BTreeMap<String, YamlValue>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[schemars(description = "Runtime environment details.")]
pub struct RuntimeSection {
    #[schemars(
        description = "Optional OCI rootfs override, e.g. \"oven/bun:1\" or \"ghcr.io/astral-sh/uv:bookworm-slim\". Alphanumerics plus `._-/:@+`, max 512 chars."
    )]
    pub base_image: Option<String>,

    #[schemars(
        description = "DEPRECATED — define `scripts.setup`/`install`/`start` explicitly instead. Legacy script inference: typescript|javascript|bun|python|rust."
    )]
    pub kind: Option<String>,

    #[schemars(
        description = "DEPRECATED — define explicit `scripts` instead. Legacy inference: npm|yarn|pnpm|bun (only meaningful with kind typescript/javascript)."
    )]
    pub package_manager: Option<String>,

    #[schemars(
        description = "DEPRECATED — put the entrypoint in `scripts.start` instead (e.g. start: \"npx tsx src/index.ts\")."
    )]
    pub entry: Option<String>,

    #[schemars(
        description = "DEPRECATED — rename to `runtime.kind` (itself deprecated; prefer explicit `scripts`)."
    )]
    pub language: Option<String>,

    #[serde(flatten)]
    #[schemars(skip)]
    pub unknown: BTreeMap<String, YamlValue>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[schemars(
    description = "Explicit lifecycle scripts (the documented, supported way to run a worker). When omitted, legacy inference from runtime.kind/package_manager applies (deprecated)."
)]
pub struct ScriptsSection {
    #[schemars(
        description = "Optional one-time sandbox provisioning, e.g. \"apt-get update && apt-get install -y build-essential\". Runs once when the sandbox is created."
    )]
    pub setup: Option<String>,

    #[schemars(
        description = "Optional dependency install, e.g. \"npm install\". Omitting it skips dependency install (warned); an explicit \"\" opts out silently."
    )]
    pub install: Option<String>,

    #[schemars(
        description = "Command that runs the worker process, e.g. \"node src/index.js\" or \"npx tsx watch src/index.ts\". Required unless legacy runtime.kind inference applies."
    )]
    pub start: Option<String>,

    #[serde(flatten)]
    #[schemars(skip)]
    pub unknown: BTreeMap<String, YamlValue>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[schemars(description = "Optional CPU/memory requests for the worker's sandbox.")]
pub struct ResourcesSection {
    #[schemars(description = "Optional integer vCPUs. Default 2; bundle workers are capped at 4.")]
    pub cpus: Option<u32>,

    #[schemars(
        description = "Optional memory in MiB. Default 2048; bundle workers are capped at 4096."
    )]
    pub memory: Option<u32>,

    #[serde(flatten)]
    #[schemars(skip)]
    pub unknown: BTreeMap<String, YamlValue>,
}

/// Human-readable YAML type label for shape-error messages. Numbers echo
/// their value (safe — serde_yaml renders them numerically); everything else
/// gets a type name so hostile string content never reaches the terminal.
fn yaml_value_desc(v: &YamlValue) -> String {
    match v {
        YamlValue::Null => "null".into(),
        YamlValue::Bool(b) => format!("the boolean {b}"),
        YamlValue::Number(n) => format!("the number {n}"),
        YamlValue::String(_) => "a string".into(),
        YamlValue::Sequence(_) => "a list".into(),
        YamlValue::Mapping(_) => "a mapping".into(),
        YamlValue::Tagged(_) => "a tagged value".into(),
    }
}

/// Treat YAML `null` like an absent field (serde maps it to `None`), so the
/// shape checks below only fire on values serde would actually reject.
fn present(v: Option<&YamlValue>) -> Option<&YamlValue> {
    v.filter(|v| !v.is_null())
}

/// Value-layer shape checks for every documented field, collected with dotted
/// paths so type mistakes read as "`runtime` must be a mapping ..., got a
/// string" instead of serde's "invalid type: string \"node\", expected struct
/// RuntimeSection" (Rust internals were leaking into user errors — caught by
/// the live DX audit). Must stay consistent with the typed struct: flag only
/// what serde would reject anyway; serde remains the backstop for anything
/// not covered here.
fn shape_problems(doc: &YamlValue) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(map) = doc.as_mapping() else {
        return problems;
    };

    for key in map.keys() {
        if key.as_str().is_none() {
            problems.push(format!(
                "manifest keys must be strings, got {}",
                yaml_value_desc(key)
            ));
        }
    }

    for field in ["name", "description", "language", "entry"] {
        if let Some(v) = present(doc.get(field))
            && v.as_str().is_none()
        {
            problems.push(format!(
                "`{field}` must be a string, got {}",
                yaml_value_desc(v)
            ));
        }
    }

    for section in ["runtime", "scripts", "resources"] {
        if let Some(v) = present(doc.get(section))
            && v.as_mapping().is_none()
        {
            problems.push(format!(
                "`{section}` must be a mapping of indented `key: value` fields, got {}",
                yaml_value_desc(v)
            ));
        }
    }

    if let Some(rt) = doc.get("runtime").and_then(YamlValue::as_mapping) {
        for field in ["base_image", "kind", "package_manager", "entry", "language"] {
            if let Some(v) = present(rt.get(field))
                && v.as_str().is_none()
            {
                problems.push(format!(
                    "`runtime.{field}` must be a string, got {}",
                    yaml_value_desc(v)
                ));
            }
        }
    }

    if let Some(scripts) = doc.get("scripts").and_then(YamlValue::as_mapping) {
        for field in ["setup", "install", "start"] {
            if let Some(v) = present(scripts.get(field))
                && v.as_str().is_none()
            {
                let hint = if v.is_sequence() {
                    " — use one command string, chaining with `&&` if needed"
                } else {
                    ""
                };
                problems.push(format!(
                    "`scripts.{field}` must be a command string, got {}{hint}",
                    yaml_value_desc(v)
                ));
            }
        }
    }

    if let Some(res) = doc.get("resources").and_then(YamlValue::as_mapping) {
        for field in ["cpus", "memory"] {
            if let Some(v) = present(res.get(field)) {
                match v.as_u64() {
                    Some(n) if n <= u64::from(u32::MAX) => {}
                    // A non-negative integer that's just too big needs the
                    // bound named, not a self-contradictory "must be a
                    // non-negative integer, got 4294967296".
                    Some(n) => problems.push(format!(
                        "`resources.{field}` must be at most {}, got {n}",
                        u32::MAX
                    )),
                    None => problems.push(format!(
                        "`resources.{field}` must be a non-negative integer, got {}",
                        yaml_value_desc(v)
                    )),
                }
            }
        }
    }

    // env/dependencies are opaque maps key-wise, but their VALUES must be
    // strings — the classic trap is `PORT: 8080` (a YAML number), so the
    // message says how to quote it.
    for section in ["env", "dependencies"] {
        let Some(v) = present(doc.get(section)) else {
            continue;
        };
        let Some(entries) = v.as_mapping() else {
            problems.push(format!(
                "`{section}` must be a mapping of string values, got {}",
                yaml_value_desc(v)
            ));
            continue;
        };
        for (key, val) in entries {
            let Some(key_name) = key.as_str().map(sanitize_key) else {
                problems.push(format!(
                    "`{section}` keys must be strings, got {}",
                    yaml_value_desc(key)
                ));
                continue;
            };
            if val.as_str().is_none() {
                let hint = match val {
                    YamlValue::Null => format!(" — write `{key_name}: \"\"` for an empty value"),
                    YamlValue::Bool(_) | YamlValue::Number(_) => {
                        format!(" — quote it (`{key_name}: \"...\"`)")
                    }
                    _ => String::new(),
                };
                problems.push(format!(
                    "`{section}.{key_name}` must be a string, got {}{hint}",
                    yaml_value_desc(val)
                ));
            }
        }
    }

    problems
}

impl WorkerManifest {
    /// Deserialize from an already-parsed YAML document. Returns `Err` with a
    /// human-readable message when the document's shape doesn't match (e.g.
    /// `runtime: node` where a mapping is required, or `resources.cpus: "four"`).
    /// Friendly [`shape_problems`] run first so the common type mistakes get
    /// dotted-path messages; raw serde errors are the backstop only.
    pub fn from_value(doc: &YamlValue) -> Result<Self, String> {
        let problems = shape_problems(doc);
        if !problems.is_empty() {
            return Err(format!("invalid manifest shape: {}", problems.join("; ")));
        }
        serde_yaml::from_value(doc.clone()).map_err(|e| format!("invalid manifest shape: {e}"))
    }

    /// Classify every manifest key into `(unknown, deprecated)` dotted paths,
    /// sanitized for terminal display and sorted. The single source of truth
    /// for add-time key validation and the `worker::validate` report.
    pub fn classify_keys(&self) -> (Vec<String>, Vec<String>) {
        let mut unknown: Vec<String> = Vec::new();
        let mut deprecated: Vec<String> = Vec::new();

        for k in self.unknown.keys() {
            unknown.push(sanitize_key(k));
        }
        if self.config.is_some() {
            deprecated.push("config".into());
        }
        if self.language.is_some() {
            deprecated.push("language".into());
        }
        if self.entry.is_some() {
            deprecated.push("entry".into());
        }
        if let Some(rt) = &self.runtime {
            for k in rt.unknown.keys() {
                unknown.push(format!("runtime.{}", sanitize_key(k)));
            }
            if rt.kind.is_some() {
                deprecated.push("runtime.kind".into());
            }
            if rt.package_manager.is_some() {
                deprecated.push("runtime.package_manager".into());
            }
            if rt.entry.is_some() {
                deprecated.push("runtime.entry".into());
            }
            if rt.language.is_some() {
                deprecated.push("runtime.language".into());
            }
        }
        if let Some(s) = &self.scripts {
            for k in s.unknown.keys() {
                unknown.push(format!("scripts.{}", sanitize_key(k)));
            }
        }
        if let Some(r) = &self.resources {
            for k in r.unknown.keys() {
                unknown.push(format!("resources.{}", sanitize_key(k)));
            }
        }
        unknown.sort();
        deprecated.sort();
        (unknown, deprecated)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// worker::validate — dry-run manifest validation for LLM/automation callers.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[schemars(
    description = "Validate an iii.worker.yaml WITHOUT installing anything. Pass exactly one of `manifest` (inline YAML text — preferred for authoring) or `path` (a file or worker directory on the engine/daemon host)."
)]
pub struct ValidateOptions {
    #[serde(default)]
    #[schemars(
        description = "Inline iii.worker.yaml content to validate. Preferred: lets a caller check a manifest before writing it anywhere."
    )]
    pub manifest: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "Host path to an iii.worker.yaml, or to a worker project directory containing one (same resolution as worker::add kind=local; resolved on the engine/daemon host, not the caller)."
    )]
    pub path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Default, serde::Serialize, JsonSchema)]
#[schemars(
    description = "Dry-run validation result. `valid: true` means worker::add would accept this manifest's keys and required fields. Fetch the full manifest schema via worker::schema { function_id: \"iii.worker.yaml\" }."
)]
pub struct ManifestReport {
    #[schemars(
        description = "True when there are no errors and no unknown keys — worker::add would accept the manifest."
    )]
    pub valid: bool,
    #[schemars(description = "Worker name parsed from the manifest, when present.")]
    pub name: Option<String>,
    #[schemars(
        description = "Blocking problems: YAML syntax/shape errors, missing required fields, invalid dependencies. Fix all of these."
    )]
    pub errors: Vec<String>,
    #[schemars(
        description = "Keys not in the documented schema (typos or unsupported). worker::add rejects the manifest while any are present."
    )]
    pub unknown_keys: Vec<String>,
    #[schemars(
        description = "Deprecated keys: still honored, warned at add time, scheduled for removal in a future version. Prefer migrating to explicit `scripts`."
    )]
    pub deprecated_keys: Vec<String>,
    #[schemars(
        description = "Non-blocking advice (e.g. omitted scripts.install, implausible base_image)."
    )]
    pub warnings: Vec<String>,
}

/// Build a full validation report from raw manifest text. Never panics; every
/// failure mode lands in `errors`. This is the `worker::validate` engine and
/// mirrors what the `worker add` gate enforces (size cap, YAML/duplicate
/// keys, shape, unknown keys, required `name`). Known gaps where `add` is
/// stricter, all on the deprecated legacy-inference path: it rejects
/// `runtime.kind` values outside its allowlist and ts/js kinds without
/// `runtime.package_manager` — checks that live in script inference, which a
/// dry run can't execute. Manifests on the documented schema (explicit
/// `scripts.*`) have no known divergence.
pub fn report_from_str(content: &str) -> ManifestReport {
    let mut report = ManifestReport::default();

    if content.len() as u64 > MAX_LOCAL_MANIFEST_BYTES {
        report.errors.push(format!(
            "manifest is {} bytes; iii.worker.yaml is capped at {MAX_LOCAL_MANIFEST_BYTES} bytes",
            content.len()
        ));
        return report;
    }

    // Value-first: catches YAML syntax errors AND duplicate mapping keys.
    let doc: YamlValue = match serde_yaml::from_str(content) {
        Ok(d) => d,
        Err(e) => {
            report.errors.push(format!("invalid YAML: {e}"));
            return report;
        }
    };

    if matches!(doc, YamlValue::Null) {
        report
            .errors
            .push("manifest is empty; at least `name` and `scripts.start` are required".into());
        return report;
    }
    if doc.as_mapping().is_none() {
        report.errors.push(
            "manifest top level must be a YAML mapping of fields, e.g. `name: my-worker`".into(),
        );
        return report;
    }

    // Dependency rules run on the Value (better messages than serde's), and
    // independently of typed-shape success.
    let self_name = doc.get("name").and_then(|v| v.as_str());
    if let Err(e) = super::worker_manifest_deps::parse_dependencies(&doc, self_name) {
        report.errors.push(e);
    }

    let manifest = match WorkerManifest::from_value(&doc) {
        Ok(m) => m,
        Err(e) => {
            report.errors.push(e);
            // Without the typed view we can still report the name.
            report.name = self_name.map(String::from);
            return report;
        }
    };

    let (unknown, deprecated) = manifest.classify_keys();
    report.unknown_keys = unknown;
    report.deprecated_keys = deprecated;

    match manifest
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(name) => {
            report.name = Some(name.to_string());
            if let Err(e) = crate::core::types::validate_worker_name(name) {
                report.errors.push(e);
            }
        }
        None => report.errors.push(
            "missing required field `name` (a single path segment, e.g. `name: my-worker`)".into(),
        ),
    }

    let has_explicit_start = manifest
        .scripts
        .as_ref()
        .and_then(|s| s.start.as_deref())
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    let has_legacy_inference = manifest.runtime.as_ref().is_some_and(|r| {
        r.kind.as_deref().is_some_and(|k| !k.trim().is_empty())
            || r.language.as_deref().is_some_and(|l| !l.trim().is_empty())
    });
    if !has_explicit_start && !has_legacy_inference {
        report.errors.push(
            "no way to start the worker: add `scripts.start` (e.g. start: \"node src/index.js\")"
                .into(),
        );
    }

    if let Some(s) = &manifest.scripts
        && s.start.is_some()
        && s.install.is_none()
    {
        report.warnings.push(
            "`scripts.install` omitted; dependency install will be skipped at provision time"
                .into(),
        );
    }
    if let Some(img) = manifest
        .runtime
        .as_ref()
        .and_then(|r| r.base_image.as_deref())
        && !is_plausible_image_ref(img.trim())
    {
        report.warnings.push(format!(
            "`runtime.base_image` {:?} does not look like an OCI reference (allowed: alphanumerics and `._-/:@+`); the default image would be used",
            sanitize_key(img),
        ));
    }
    if !report.deprecated_keys.is_empty() {
        report.warnings.push(
            "deprecated key(s) present — still honored but will be removed in a future version; \
             migrate to explicit `scripts.*` (and move `config` into the project config.yaml entry)"
                .into(),
        );
    }

    report.valid = report.errors.is_empty() && report.unknown_keys.is_empty();
    report
}

/// JSON Schema for `iii.worker.yaml`, post-processed so machines see the real
/// contract: unknown keys rejected (`additionalProperties: false` on every
/// section) and `name` required. schemars can't express either directly here —
/// the flatten catch-all forbids `deny_unknown_fields`, and `name` is `Option`
/// in Rust only so partial manifests still classify during validation.
pub fn manifest_schema_json() -> JsonValue {
    let schema = schemars::schema_for!(WorkerManifest);
    let mut v = serde_json::to_value(schema).unwrap_or(JsonValue::Null);

    if let Some(root) = v.as_object_mut() {
        root.insert("additionalProperties".into(), JsonValue::Bool(false));
        root.insert("required".into(), serde_json::json!(["name"]));
        if let Some(defs) = root.get_mut("definitions").and_then(|d| d.as_object_mut()) {
            for section in ["RuntimeSection", "ScriptsSection", "ResourcesSection"] {
                if let Some(s) = defs.get_mut(section).and_then(|s| s.as_object_mut()) {
                    s.insert("additionalProperties".into(), JsonValue::Bool(false));
                }
            }
        }
    }
    v
}

/// A complete, ready-to-write minimal Node worker as `{ path → contents }`,
/// served as the `iii.worker.yaml` pseudo-entry's response in
/// `worker::schema`. Exists because a real harness session showed an LLM
/// hallucinating the SDK package name (`@iii-hq/sdk` — npm 404) and then
/// reverse-engineering `registerWorker` out of `node_modules`: the manifest
/// schema alone doesn't teach what the worker CODE must look like. Contents
/// mirror docs/next/creating-workers/workers.mdx (the canonical example).
pub fn hello_world_example_json() -> JsonValue {
    serde_json::json!({
        "iii.worker.yaml": "name: hello-world\ndescription: Minimal Node worker exposing hello-world::greet.\nscripts:\n  install: \"npm install\"\n  start: \"node src/index.js\"\n",
        "package.json": "{\n  \"name\": \"hello-world\",\n  \"type\": \"module\",\n  \"private\": true,\n  \"dependencies\": {\n    \"iii-sdk\": \"latest\"\n  }\n}\n",
        "src/index.js": "import { registerWorker } from \"iii-sdk\";\n\nconst url = process.env.III_URL;\nif (!url) throw new Error(\"III_URL must be set\");\nconst worker = registerWorker(url, { workerName: \"hello-world\" });\n\nworker.registerFunction(\"hello-world::greet\", async (payload) => {\n  const name = payload?.name ?? \"World\";\n  return { message: `Hello, ${name}!` };\n});\n",
        "_usage": "Write these files into a directory on the engine/daemon host, then worker::add { source: { kind: \"local\", path: \"<dir>\" }, wait: false } and poll worker::status. The npm package is `iii-sdk` (NOT @iii-hq/sdk). The engine sets III_URL/III_ENGINE_URL automatically."
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(s: &str) -> ManifestReport {
        report_from_str(s)
    }

    #[test]
    fn report_accepts_fully_documented_manifest() {
        let r = report(
            "name: my-worker\ndescription: does things\nruntime:\n  base_image: oven/bun:1\n\
             scripts:\n  setup: \"apt-get update\"\n  install: \"npm install\"\n  start: \"node server.js\"\n\
             env:\n  FOO: bar\ndependencies:\n  iii-http: \"^0.19\"\nresources:\n  cpus: 2\n  memory: 2048\n",
        );
        assert!(r.valid, "expected valid, got: {r:?}");
        assert_eq!(r.name.as_deref(), Some("my-worker"));
        assert!(r.errors.is_empty() && r.unknown_keys.is_empty());
    }

    #[test]
    fn report_minimal_manifest_is_valid() {
        let r = report("name: w\nscripts:\n  start: \"node i.js\"\n");
        assert!(r.valid, "got: {r:?}");
        // install omitted → warned, not an error
        assert!(r.warnings.iter().any(|w| w.contains("scripts.install")));
    }

    #[test]
    fn report_flags_unknown_keys_sorted() {
        let r = report("name: w\nscripts:\n  start: x\n  instal: y\nzzz_bad: 1\naaa_bad: 2\n");
        assert!(!r.valid);
        assert_eq!(r.unknown_keys, vec!["aaa_bad", "scripts.instal", "zzz_bad"]);
    }

    #[test]
    fn report_flags_deprecated_keys_but_stays_valid() {
        let r = report("name: w\nruntime:\n  kind: bun\n  entry: src/i.ts\nconfig:\n  port: 1\n");
        assert!(r.valid, "deprecated keys must not invalidate: {r:?}");
        assert_eq!(
            r.deprecated_keys,
            vec!["config", "runtime.entry", "runtime.kind"]
        );
        assert!(r.warnings.iter().any(|w| w.contains("deprecated")));
    }

    #[test]
    fn report_requires_name_and_start() {
        let r = report("scripts:\n  setup: \"true\"\n");
        assert!(!r.valid);
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("missing required field `name`"))
        );
        assert!(r.errors.iter().any(|e| e.contains("scripts.start")));
    }

    #[test]
    fn report_rejects_bad_dependencies() {
        let r = report("name: w\nscripts:\n  start: x\ndependencies:\n  w: \"^1\"\n");
        assert!(r.errors.iter().any(|e| e.contains("itself")), "got: {r:?}");

        let r = report("name: w\nscripts:\n  start: x\ndependencies:\n  other: \"not-a-range\"\n");
        assert!(r.errors.iter().any(|e| e.contains("semver")), "got: {r:?}");
    }

    #[test]
    fn report_rejects_duplicate_keys() {
        let r = report("name: w\nname: x\nscripts:\n  start: y\n");
        assert!(!r.valid);
        assert!(
            r.errors.iter().any(|e| e.contains("invalid YAML")),
            "got: {r:?}"
        );
    }

    #[test]
    fn report_rejects_wrong_shapes_with_clear_error() {
        // The messages must name the field and the expected type in plain
        // English — never serde's "expected struct RuntimeSection".
        let r = report("name: w\nruntime: node\nscripts:\n  start: x\n");
        assert!(!r.valid);
        let err = r
            .errors
            .iter()
            .find(|e| e.contains("invalid manifest shape"))
            .expect("shape error");
        assert!(err.contains("`runtime` must be a mapping"), "got: {err}");
        assert!(
            !err.contains("RuntimeSection"),
            "Rust internals must not leak: {err}"
        );

        let r = report("name: w\nscripts:\n  start: x\nresources:\n  cpus: \"four\"\n");
        assert!(!r.valid, "non-integer cpus must be a shape error: {r:?}");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("`resources.cpus` must be a non-negative integer")),
            "got: {r:?}"
        );
    }

    #[test]
    fn shape_errors_cover_common_type_mistakes() {
        // Every problem in one manifest, all reported together with dotted paths.
        let r = report(
            "name: 7\nruntime:\n  base_image: 1\nscripts:\n  start:\n    - npm i\n    - npm start\n\
             env:\n  PORT: 8080\n  EMPTY:\ndependencies:\n  other: 1.0\n",
        );
        assert!(!r.valid);
        let shape = r
            .errors
            .iter()
            .find(|e| e.contains("invalid manifest shape"))
            .expect("shape error present");
        for needle in [
            "`name` must be a string, got the number 7",
            "`runtime.base_image` must be a string",
            "`scripts.start` must be a command string, got a list — use one command string",
            "`env.PORT` must be a string, got the number 8080 — quote it",
            "`env.EMPTY` must be a string, got null — write `EMPTY: \"\"`",
            "`dependencies.other` must be a string",
        ] {
            assert!(shape.contains(needle), "missing {needle:?} in: {shape}");
        }
    }

    #[test]
    fn shape_error_names_the_u32_bound_for_oversized_resources() {
        // 4294967296 IS a non-negative integer; the message must name the
        // actual violated constraint instead of contradicting itself.
        let r = report("name: w\nscripts:\n  start: x\nresources:\n  memory: 4294967296\n");
        assert!(!r.valid);
        assert!(
            r.errors.iter().any(
                |e| e.contains("`resources.memory` must be at most 4294967295, got 4294967296")
            ),
            "got: {r:?}"
        );
    }

    #[test]
    fn sanitize_key_strips_bidi_and_zero_width_format_chars() {
        // U+202E (RLO) visually reverses everything after it; U+200B is an
        // invisible zero-width space. Neither is Cc, so is_control() alone
        // passes them through — they must still never reach the terminal.
        assert_eq!(sanitize_key("ev\u{202e}il"), "evil");
        assert_eq!(sanitize_key("a\u{200b}b\u{feff}c\u{2066}d"), "abcd");

        // End-to-end through the env-key echo path in shape_problems.
        let r = report("name: w\nscripts:\n  start: x\nenv:\n  \"P\u{202e}ORT\": 1\n");
        let err = r
            .errors
            .iter()
            .find(|e| e.contains("must be a string"))
            .expect("env error");
        assert!(
            !err.contains('\u{202e}'),
            "bidi override must be stripped: {err:?}"
        );
        assert!(err.contains("PORT"), "printable key text survives: {err}");
    }

    #[test]
    fn report_trims_padded_name_like_the_add_path() {
        let r = report("name: \" w \"\nscripts:\n  start: x\n");
        assert!(r.valid, "padded name trims to a valid one: {r:?}");
        assert_eq!(r.name.as_deref(), Some("w"));
    }

    #[test]
    fn shape_errors_for_scalar_env_and_non_string_keys() {
        let r = report("name: w\nscripts:\n  start: x\nenv: production\n");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("`env` must be a mapping of string values")),
            "got: {r:?}"
        );

        let r = report("name: w\nscripts:\n  start: x\nenv:\n  1: x\n");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("`env` keys must be strings")),
            "got: {r:?}"
        );
    }

    #[test]
    fn report_empty_and_non_mapping() {
        assert!(!report("").valid);
        assert!(!report("- a\n- b\n").valid);
        assert!(report("- a\n").errors[0].contains("mapping"));
    }

    #[test]
    fn report_warns_on_implausible_base_image() {
        let r = report("name: w\nscripts:\n  start: x\nruntime:\n  base_image: \"bad image!\"\n");
        assert!(r.valid, "implausible base_image is a warning, not an error");
        assert!(r.warnings.iter().any(|w| w.contains("base_image")));
    }

    #[test]
    fn report_sanitizes_unknown_key_names() {
        let mut m = serde_yaml::Mapping::new();
        m.insert("name".into(), "w".into());
        m.insert(
            serde_yaml::Value::from("ev\u{1b}\u{7}il"),
            serde_yaml::Value::from(1),
        );
        let yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping(m)).unwrap();
        let r = report(&yaml);
        assert!(
            r.unknown_keys.iter().any(|k| k == "evil"),
            "control bytes stripped: {:?}",
            r.unknown_keys
        );
    }

    #[test]
    fn schema_enforces_closed_world_and_required_name() {
        let s = manifest_schema_json();
        assert_eq!(s["additionalProperties"], serde_json::json!(false));
        assert_eq!(s["required"], serde_json::json!(["name"]));
        for section in ["RuntimeSection", "ScriptsSection", "ResourcesSection"] {
            assert_eq!(
                s["definitions"][section]["additionalProperties"],
                serde_json::json!(false),
                "{section} must reject unknown keys in the schema"
            );
        }
        // Deprecation is conveyed via descriptions the LLM reads.
        let runtime_kind_desc =
            s["definitions"]["RuntimeSection"]["properties"]["kind"]["description"]
                .as_str()
                .unwrap_or("");
        assert!(runtime_kind_desc.contains("DEPRECATED"));
    }

    #[test]
    fn hello_world_example_passes_our_own_validation() {
        // The example we hand to LLMs must be valid by our own rules — if the
        // schema or validator drifts, this test forces the example to follow.
        let bundle = hello_world_example_json();
        let manifest = bundle["iii.worker.yaml"].as_str().expect("manifest text");
        let r = report_from_str(manifest);
        assert!(r.valid, "example manifest must validate: {r:?}");
        assert!(
            r.deprecated_keys.is_empty(),
            "example must not use deprecated keys"
        );
        // And the code example must name the real npm package.
        let code = bundle["src/index.js"].as_str().unwrap();
        assert!(code.contains("from \"iii-sdk\""));
        let pkg = bundle["package.json"].as_str().unwrap();
        assert!(pkg.contains("\"iii-sdk\""));
    }

    #[test]
    fn shape_errors_describe_bool_mapping_and_tagged_values() {
        let r = report("name: true\nscripts:\n  start: x\n");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("`name` must be a string, got the boolean true")),
            "got: {r:?}"
        );

        let r = report("name:\n  nested: 1\nscripts:\n  start: x\n");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("`name` must be a string, got a mapping")),
            "got: {r:?}"
        );

        // serde_yaml accessors untag, so a tagged STRING still reads as a
        // string; only a tagged non-string reaches the Tagged description.
        let r = report("name: !custom [w]\nscripts:\n  start: x\n");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("`name` must be a string, got a tagged value")),
            "got: {r:?}"
        );
    }

    #[test]
    fn shape_errors_flag_non_string_top_level_keys() {
        let r = report("name: w\nscripts:\n  start: x\n1: y\n");
        assert!(!r.valid);
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("manifest keys must be strings, got the number 1")),
            "got: {r:?}"
        );
    }

    #[test]
    fn scripts_scalar_shape_error_omits_list_hint() {
        let r = report("name: w\nscripts:\n  start: 5\n");
        assert!(!r.valid);
        let err = r
            .errors
            .iter()
            .find(|e| e.contains("`scripts.start`"))
            .expect("scripts error");
        assert!(
            err.contains("must be a command string, got the number 5"),
            "got: {err}"
        );
        assert!(
            !err.contains("use one command string"),
            "list hint only fits lists: {err}"
        );
    }

    #[test]
    fn env_list_value_shape_error_omits_quoting_hint() {
        let r = report("name: w\nscripts:\n  start: x\nenv:\n  FOO:\n    - a\n");
        assert!(!r.valid);
        let err = r
            .errors
            .iter()
            .find(|e| e.contains("`env.FOO`"))
            .expect("env error");
        assert!(err.contains("must be a string, got a list"), "got: {err}");
        assert!(
            !err.contains("quote it") && !err.contains("write `"),
            "scalar hints must not apply to lists: {err}"
        );
    }

    #[test]
    fn report_rejects_manifest_over_size_cap() {
        let content = "x".repeat(MAX_LOCAL_MANIFEST_BYTES as usize + 1);
        let r = report(&content);
        assert!(!r.valid);
        assert_eq!(
            r.errors.len(),
            1,
            "size cap short-circuits all other checks"
        );
        assert!(
            r.errors[0].contains(&format!("capped at {MAX_LOCAL_MANIFEST_BYTES} bytes")),
            "got: {:?}",
            r.errors
        );
    }

    #[test]
    fn report_rejects_path_escaping_name() {
        let r = report("name: \"../escape\"\nscripts:\n  start: x\n");
        assert!(!r.valid);
        assert_eq!(r.name.as_deref(), Some("../escape"));
        assert!(r.errors.iter().any(|e| e.contains("'..'")), "got: {r:?}");
    }

    #[test]
    fn from_value_rejects_non_mapping_document_via_serde_backstop() {
        let doc: YamlValue = serde_yaml::from_str("just-a-string").unwrap();
        let err = WorkerManifest::from_value(&doc).expect_err("non-mapping must fail");
        assert!(err.starts_with("invalid manifest shape:"), "got: {err}");
    }

    #[test]
    fn classify_keys_matches_legacy_validator_semantics() {
        let doc: YamlValue =
            serde_yaml::from_str("name: w\nruntime:\n  kind: bun\n  foo: 1\nconfig: {}\nbad: 2\n")
                .unwrap();
        let m = WorkerManifest::from_value(&doc).unwrap();
        let (unknown, deprecated) = m.classify_keys();
        assert_eq!(unknown, vec!["bad", "runtime.foo"]);
        assert_eq!(deprecated, vec!["config", "runtime.kind"]);
    }
}
