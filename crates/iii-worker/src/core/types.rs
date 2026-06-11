// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[schemars(
    description = "Where to fetch the worker from. Use `registry` for the public iii worker registry, `oci` for an arbitrary container image, or `local` for a filesystem path on the engine/daemon host (works over the trigger too; the path is resolved on the host, not the caller)."
)]
pub enum WorkerSource {
    Registry {
        #[schemars(description = "Registry slug, e.g. \"pdfkit\".")]
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(
            description = "Optional pinned version (semver). Omit to take the latest stable."
        )]
        version: Option<String>,
    },
    Oci {
        #[schemars(description = "Full OCI reference, e.g. \"ghcr.io/iii-hq/node:latest\".")]
        reference: String,
    },
    Local {
        #[schemars(
            description = "Path to a worker PROJECT DIRECTORY on the engine/daemon host (NOT the caller's machine), e.g. \"/srv/workers/my-worker\". The directory MUST contain an `iii.worker.yaml` manifest.\n\nMinimal manifest (just name + how to start):\n  name: my-worker\n  scripts:\n    start: \"node src/index.js\"\n\nAll iii.worker.yaml fields:\n  name        (required) worker id; must be a single path segment (alphanumerics, '-', '_', '.').\n  description (optional) one-line human/LLM-readable summary.\n  scripts.start   (required unless inferred) command that runs the worker process.\n  scripts.setup   (optional) one-time host provisioning, e.g. \"apt-get install -y build-essential\".\n  scripts.install (optional) dependency install, e.g. \"npm install\".\n  dependencies    (optional) map of other-worker-name -> semver range, e.g. { iii-state: \"^0.19\" }.\n  resources.cpus   (optional) integer vCPUs, default 2.\n  resources.memory (optional) MiB, default 2048.\n  env             (optional) string->string vars injected into the worker (III_URL/III_ENGINE_URL are set by the engine and ignored here).\n  runtime.base_image (optional) OCI rootfs override, e.g. \"oven/bun:1\".\n\nUnknown keys are rejected at add time. The setup/install/start scripts run on the host. Works over the trigger as well as the CLI. Dry-run a manifest with `worker::validate`; fetch the full manifest JSON Schema via `worker::schema { function_id: \"iii.worker.yaml\" }`."
        )]
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "add_options_example", example = "add_options_local_example")]
pub struct AddOptions {
    #[schemars(description = "Where the worker comes from (registry/oci/local).")]
    pub source: WorkerSource,
    #[serde(default)]
    #[schemars(
        description = "Force re-download by deleting cached artifacts before installing. Use to recover from a corrupt cache."
    )]
    pub force: bool,
    #[serde(default)]
    #[schemars(
        description = "Reset the worker's config.yaml block before re-adding. Combine with force=true for a clean reinstall."
    )]
    pub reset_config: bool,
    #[serde(default = "default_wait")]
    #[schemars(
        description = "Block until the worker reports ready. Default true. Over the trigger surface installs routinely exceed bus invocation timeouts (npm install etc.) — prefer wait:false and poll worker::status. If a wait:true call times out, the install KEEPS RUNNING server-side: do not re-issue blindly (the project lock will be busy, W120); poll worker::status instead."
    )]
    pub wait: bool,
}

fn add_options_example() -> serde_json::Value {
    serde_json::json!({
        "source": {"kind": "registry", "name": "pdfkit", "version": "1.0.0"},
        "force": false,
        "reset_config": false,
        "wait": true
    })
}

/// Second example so introspecting LLMs see the local-path shape, not just
/// the registry one — the directory must hold an iii.worker.yaml (see the
/// `WorkerSource::Local` field description for the manifest fields).
fn add_options_local_example() -> serde_json::Value {
    serde_json::json!({
        "source": {"kind": "local", "path": "/srv/workers/my-worker"},
        "wait": true
    })
}

fn default_wait() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AddStatus {
    Installed,
    AlreadyCurrent,
    Repaired,
    Replaced,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddOutcome {
    #[schemars(description = "Resolved worker name (matches what `worker::list` reports).")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Installed version read from iii.lock. Null when the lockfile read failed; re-query via worker::list."
    )]
    pub version: Option<String>,
    #[schemars(
        description = "Outcome status: installed (new), already_current, repaired, or replaced."
    )]
    pub status: AddStatus,
    #[schemars(
        description = "Echoes `wait`. Does NOT confirm the worker is actually running — verify via worker::list."
    )]
    pub awaited_ready: bool,
    #[schemars(
        description = "Absolute path of the project config file the worker entry was written to."
    )]
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RemoveOptions {
    /// Worker names to remove. Empty + `all = false` is rejected (W103 MissingTarget).
    #[serde(default)]
    pub names: Vec<String>,
    /// Remove every installed worker. Cannot be combined with non-empty `names`.
    #[serde(default)]
    pub all: bool,
    /// Required for destructive operations. `yes = false` returns W104 (ConsentRequired).
    #[serde(default)]
    pub yes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RemoveOutcome {
    pub removed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateOptions {
    /// Names to update. Empty list means "update all installed workers".
    #[serde(default)]
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateEntry {
    pub name: String,
    pub from_version: String,
    pub to_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateOutcome {
    pub updated: Vec<UpdateEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "start_options_example")]
pub struct StartOptions {
    #[schemars(
        description = "Installed worker name (must already be in iii.config.yaml). See `worker::list`."
    )]
    pub name: String,
    #[serde(default)]
    #[schemars(
        description = "Override the engine WS port the worker connects back to. Default = engine's iii-worker-manager port."
    )]
    pub port: Option<u16>,
    #[serde(default)]
    #[schemars(
        description = "Forward a YAML config file to the worker as `--config <path>`. Binary workers only; OCI ignores it."
    )]
    pub config: Option<String>,
    #[serde(default = "default_wait")]
    #[schemars(
        description = "Block until the worker reports ready. Default true. Set false to return immediately."
    )]
    pub wait: bool,
}

fn start_options_example() -> serde_json::Value {
    serde_json::json!({"name": "pdfkit", "wait": true})
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StartOutcome {
    pub name: String,
    /// Resolved pid from the worker's pidfile post-start. `None` when
    /// the pidfile wasn't present or couldn't be read (e.g., engine
    /// builtins that don't keep a pidfile).
    pub pid: Option<u32>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "stop_options_example")]
pub struct StopOptions {
    #[schemars(description = "Worker name to stop. Use `worker::list` to see what's running.")]
    pub name: String,
    #[serde(default)]
    #[schemars(
        description = "Required for destructive operations. yes=false returns W104 (ConsentRequired)."
    )]
    pub yes: bool,
}

fn stop_options_example() -> serde_json::Value {
    serde_json::json!({"name": "pdfkit", "yes": true})
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StopOutcome {
    pub name: String,
    /// Whether the worker process is no longer alive after the stop call.
    /// `false` means the stop attempt didn't take effect within the
    /// daemon's grace window — the worker may still be running.
    pub stopped: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "list_options_example")]
pub struct ListOptions {
    #[serde(default)]
    #[schemars(
        description = "Filter to running workers only. Default false returns all known workers regardless of run state."
    )]
    pub running_only: bool,
}

fn list_options_example() -> serde_json::Value {
    serde_json::json!({"running_only": false})
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "logs_options_example")]
pub struct LogsOptions {
    #[schemars(description = "Worker whose logs to read, e.g. \"pdfkit\".")]
    pub name: String,
    #[serde(default = "default_logs_tail")]
    #[schemars(
        description = "Trailing lines to return per stream. Default 100, capped at 1000. Only the last 1 MiB of each log file is scanned."
    )]
    pub tail: usize,
    #[serde(default)]
    #[schemars(
        description = "Return raw log lines including ANSI color codes and spinner redraw frames. Default false: terminal escape sequences are stripped and spinner-only frames dropped, which is what automation/LLM callers want."
    )]
    pub raw: bool,
}

fn default_logs_tail() -> usize {
    100
}

fn logs_options_example() -> serde_json::Value {
    serde_json::json!({"name": "pdfkit", "tail": 100})
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogsOutcome {
    #[schemars(description = "Worker whose logs were read.")]
    pub name: String,
    #[schemars(
        description = "Host directory the logs were read from. Null when the worker has no log directory yet (never started on this host, or logs were cleared)."
    )]
    pub logs_dir: Option<String>,
    #[schemars(description = "Trailing stdout lines — the worker/guest console stream.")]
    pub stdout: Vec<String>,
    #[schemars(
        description = "Trailing stderr lines — host-side boot/runtime messages; during startup these chronologically precede stdout."
    )]
    pub stderr: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "status_options_example")]
pub struct StatusOptions {
    #[schemars(
        description = "Worker to inspect, e.g. \"pdfkit\". Use worker::list for the full roster."
    )]
    pub name: String,
}

fn status_options_example() -> serde_json::Value {
    serde_json::json!({"name": "pdfkit"})
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(
    description = "One worker's install/runtime status plus a recent sanitized log tail and a next-step hint. The poll target after worker::add { wait: false } (or after a wait:true call hit a bus timeout — the install continues server-side)."
)]
pub struct StatusOutcome {
    #[schemars(description = "Worker name inspected.")]
    pub name: String,
    #[schemars(
        description = "True when the worker is declared in the project config.yaml (i.e. worker::add completed its config write)."
    )]
    pub installed: bool,
    #[schemars(
        description = "How the worker is provisioned: \"oci\" | \"local\" | \"binary\" | \"bundle\" | \"builtin\" (config-only/engine builtin) | \"not-installed\"."
    )]
    pub worker_type: String,
    #[schemars(
        description = "True when the worker's process is alive (or, for engine builtins, when the engine itself is running). False during install/boot AND after a crash — check stderr_tail to tell which."
    )]
    pub running: bool,
    #[schemars(description = "Worker process pid when one is observable; null for engine builtins.")]
    pub pid: Option<u32>,
    #[schemars(description = "Installed version from iii.lock; null when not lockfile-tracked.")]
    pub version: Option<String>,
    #[schemars(description = "Host log directory, when any logs exist yet.")]
    pub logs_dir: Option<String>,
    #[schemars(
        description = "Last stderr lines (terminal escapes stripped) — host-side boot/install progress and errors live here (e.g. npm install failures)."
    )]
    pub stderr_tail: Vec<String>,
    #[schemars(description = "Last stdout lines (terminal escapes stripped) — the worker/guest console.")]
    pub stdout_tail: Vec<String>,
    #[schemars(
        description = "Suggested next step derived from the state above (e.g. retry guidance, which trigger to call). Advisory, not machine-stable."
    )]
    pub hint: String,
}

/// Reject names that could escape the per-worker directories these names
/// are joined into (`~/.iii/logs/<name>`, `~/.iii/workers/<name>`, …).
pub fn validate_worker_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Worker name cannot be empty".into());
    }
    if name.contains("..") {
        return Err(format!("Worker name '{}' contains '..' sequence", name));
    }
    if name.starts_with('.') {
        return Err(format!(
            "Worker name '{}' cannot start with '.' (reserved for internal control directories)",
            name
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!(
            "Worker name '{}' contains invalid characters. Only alphanumeric, dash, underscore, and dot are allowed.",
            name
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerEntry {
    #[schemars(description = "Worker name as listed in iii.config.yaml.")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Version from iii.lock. Null for engine builtins (iii-stream/iii-http/etc) that aren't lock-tracked."
    )]
    pub version: Option<String>,
    #[schemars(description = "Whether the worker is currently running.")]
    pub running: bool,
    #[schemars(
        description = "Process id when discoverable via ps. Null for engine builtins or when the pid wasn't recoverable."
    )]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListOutcome {
    pub workers: Vec<WorkerEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClearOptions {
    /// Worker names whose artifacts to wipe. Empty + `all = false` is
    /// rejected (W103 MissingTarget). Empty + `all = true` wipes everything.
    #[serde(default)]
    pub names: Vec<String>,
    /// Wipe ALL worker artifacts. Cannot be combined with non-empty `names`.
    #[serde(default)]
    pub all: bool,
    /// Required for destructive operations. `yes = false` returns W104 (ConsentRequired).
    #[serde(default)]
    pub yes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClearOutcome {
    pub cleared_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn worker_source_registry_round_trip() {
        let v = json!({ "kind": "registry", "name": "pdfkit", "version": "1.0.0" });
        let parsed: WorkerSource = serde_json::from_value(v.clone()).unwrap();
        assert!(matches!(
            &parsed,
            WorkerSource::Registry { name, version } if name == "pdfkit" && version.as_deref() == Some("1.0.0")
        ));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), v);
    }

    #[test]
    fn worker_source_oci_round_trip() {
        let v = json!({ "kind": "oci", "reference": "ghcr.io/iii-hq/node:latest" });
        let parsed: WorkerSource = serde_json::from_value(v.clone()).unwrap();
        assert!(
            matches!(&parsed, WorkerSource::Oci { reference } if reference == "ghcr.io/iii-hq/node:latest")
        );
        assert_eq!(serde_json::to_value(&parsed).unwrap(), v);
    }

    #[test]
    fn worker_source_local_round_trip() {
        let v = json!({ "kind": "local", "path": "./my-worker" });
        let parsed: WorkerSource = serde_json::from_value(v.clone()).unwrap();
        assert!(
            matches!(&parsed, WorkerSource::Local { path } if path.to_str() == Some("./my-worker"))
        );
    }

    #[test]
    fn add_options_default_round_trip() {
        let opts = AddOptions {
            source: WorkerSource::Registry {
                name: "x".into(),
                version: None,
            },
            force: false,
            reset_config: false,
            wait: true,
        };
        let v = serde_json::to_value(&opts).unwrap();
        let parsed: AddOptions = serde_json::from_value(v).unwrap();
        assert_eq!(parsed.wait, true);
    }
}
