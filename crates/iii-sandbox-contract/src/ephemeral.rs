// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Request and response types of the sixteen ephemeral `sandbox::*` operations. Handlers and
//! runner traits live in the adapter.

use crate::channel::StreamChannelRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// sandbox::create
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "create_request_example")]
pub struct CreateRequest {
    /// Catalog name of the image to boot. Bundled presets are
    /// `"python"` and `"node"`; pass either string verbatim. The only
    /// other accepted values are the literal keys of the adapter's
    /// `custom_images` config — set by the operator. Do NOT pass an OCI
    /// ref like `"ghcr.io/iii-hq/node:latest"` or
    /// `"docker.io/library/node:20"` unless that exact string is the
    /// catalog key. Unknown values return S100 with the allowed set in
    /// the error message.
    pub image: String,
    /// vCPU count; daemon/image default applies when omitted.
    #[serde(default)]
    pub cpus: Option<u32>,
    /// Memory cap in MiB; daemon/image default applies when omitted.
    #[serde(default)]
    pub memory_mb: Option<u32>,
    /// Human label surfaced by `sandbox::list`; not an identifier.
    #[serde(default)]
    pub name: Option<String>,
    /// Whether the VM gets outbound networking; daemon default when omitted.
    #[serde(default)]
    pub network: Option<bool>,
    /// Auto-stop the VM after this many seconds of inactivity; daemon
    /// default applies when omitted.
    #[serde(default)]
    pub idle_timeout_secs: Option<u64>,
    /// Environment entries injected into the VM. Accepts either a
    /// `Vec<"K=V">` (original wire shape) or a `{ K: V }` map.
    /// `handle_create` normalises to the `Vec<String>` shape the boot
    /// path expects before invoking the launcher.
    #[serde(default)]
    pub env: EnvShape,
}

fn create_request_example() -> serde_json::Value {
    serde_json::json!({
        "image": "node",
        "memory_mb": 512,
        "env": { "NODE_ENV": "production" },
        "idle_timeout_secs": 600
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateResponse {
    pub sandbox_id: String,
    pub image: String,
}

// ---------------------------------------------------------------------------
// sandbox::exec
// ---------------------------------------------------------------------------

/// Environment-variable input. Agents naturally pass `{ FOO: "bar" }`
/// (matching Docker/npm/k8s mental models); the original wire shape was
/// `Vec<"K=V">`. The untagged enum accepts both; `into_kv_vec()`
/// normalises to the canonical `Vec<String>` the runner expects.
///
/// `Default` is the empty vec form (the historical wire shape).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EnvShape {
    /// Original wire shape: `["FOO=bar", "PATH=/usr/bin"]`.
    Vec(Vec<String>),
    /// Agent-natural shape: `{ "FOO": "bar", "PATH": "/usr/bin" }`.
    /// Iteration is sorted by key (BTreeMap) so two callers passing the
    /// same map get the same env-var ordering.
    Map(std::collections::BTreeMap<String, String>),
}

impl Default for EnvShape {
    fn default() -> Self {
        // Empty Vec — matches the historical wire default. Cannot use
        // `#[default]` on the variant because schemars 0.8 chokes on
        // `#[default]` for tuple variants; manual impl keeps both
        // schemars and serde happy.
        EnvShape::Vec(Vec::new())
    }
}

impl EnvShape {
    /// Normalise to the canonical `Vec<String>` shape the runner consumes.
    /// Validates env-var names match `[A-Za-z_][A-Za-z0-9_]*` (POSIX shell
    /// portable names, plus the lowercase extension that `is_valid_env_name`
    /// allows for npm/pip/hg-style variables) for the Map form; the `Err`
    /// is the message listing the bad keys, which the adapter wraps as S001.
    pub fn into_kv_vec(self) -> Result<Vec<String>, String> {
        match self {
            EnvShape::Vec(v) => Ok(v),
            EnvShape::Map(m) => {
                let mut bad = Vec::new();
                for k in m.keys() {
                    if !is_valid_env_name(k) {
                        bad.push(k.clone());
                    }
                }
                if !bad.is_empty() {
                    return Err(format!(
                        "env contains invalid variable name(s): {:?}. \
                         Names must match `[A-Za-z_][A-Za-z0-9_]*`.",
                        bad
                    ));
                }
                Ok(m.into_iter().map(|(k, v)| format!("{k}={v}")).collect())
            }
        }
    }
}

fn is_valid_env_name(name: &str) -> bool {
    // POSIX portable env-var names are `[A-Z_][A-Z0-9_]*`, but real-world
    // tooling routinely sets lowercase names (npm_config_*, hg_*, pip_*,
    // .env files in modern frameworks). Accept both cases so the
    // map-shape doesn't surprise agents passing what the tooling docs
    // say to use.
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "exec_request_example")]
pub struct ExecRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// The binary to execute. Accepts three shapes (`handle_exec` picks
    /// one and normalises `cmd`/`args` before passing to the runner):
    ///
    /// 1. **Shell line**: `cmd: "node /home/app/index.js"`. If `cmd`
    ///    contains whitespace AND `args` is empty AND `argv` is empty,
    ///    `cmd` is shlex-split into `(head, tail)` and `head` becomes
    ///    the binary while `tail` becomes the argv.
    /// 2. **cmd + args**: `cmd: "node", args: ["-v"]`. The classic
    ///    POSIX shape; unchanged from earlier versions.
    /// 3. **argv array**: `argv: ["node", "/home/app/index.js"]`.
    ///    Wins over `cmd`/`args` if non-empty; the first element is
    ///    the binary, the rest are arguments.
    ///
    /// Shlex is NOT bash. Shell metacharacters (`;`, `|`, `&&`, `>`,
    /// pipes, redirects, variable expansion) inside the shell-line
    /// shape are split as text, not interpreted. Use `sandbox::run`
    /// with `lang: "shell"` if you need bash semantics.
    #[serde(default)]
    pub cmd: String,
    /// Argv tail passed to `cmd` (each entry is one argv slot).
    #[serde(default)]
    pub args: Vec<String>,
    /// Alternative input shape: a single argv array where the first
    /// element is the binary and the rest are arguments. Mutually
    /// exclusive with `cmd` having whitespace OR `args` being set.
    #[serde(default)]
    pub argv: Vec<String>,
    /// Base64-encoded bytes piped to the child's stdin.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Environment entries added to the child. Accepts either a
    /// `Vec<"K=V">` (the original wire shape) or a `{ K: V }` map.
    /// `handle_exec` normalises to `Vec<String>` before invoking the
    /// runner.
    #[serde(default)]
    pub env: EnvShape,
    /// Kill-after window in ms. Defaults to 300_000 (5 minutes) — sized
    /// for cold `npm install` / `pip install` / `cargo build`. Pass a
    /// smaller value (e.g. 10_000) for probes and version checks.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Working directory inside the sandbox; `/` when omitted.
    #[serde(default)]
    pub workdir: Option<String>,
}

fn exec_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "cmd": "node",
        "args": ["/home/app/index.js"],
        "env": { "NODE_ENV": "production" },
        "timeout_ms": 300000
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExecResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub success: bool,
    /// `stdout` was cut at the adapter's output cap. Since contract 1.1; false from older adapters.
    #[serde(default)]
    pub stdout_truncated: bool,
    /// `stderr` was cut at the adapter's output cap. Since contract 1.1; false from older adapters.
    #[serde(default)]
    pub stderr_truncated: bool,
}

// ---------------------------------------------------------------------------
// sandbox::stop
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StopRequest {
    pub sandbox_id: String,
    /// Block until the VM is fully reaped before returning.
    #[serde(default)]
    pub wait: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StopResponse {
    pub sandbox_id: String,
    pub stopped: bool,
}

// ---------------------------------------------------------------------------
// sandbox::list
// ---------------------------------------------------------------------------

// Kept as a struct (rather than `()`) so older clients that send
// `{"show_all": ...}` or similar fields continue to deserialize
// cleanly. serde ignores unknown fields by default; no wire-compat break.
// A plain comment, not a doc: today's schema for this type has no description.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ListRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SandboxSummary {
    pub sandbox_id: String,
    pub name: Option<String>,
    pub image: String,
    pub age_secs: u64,
    /// Any exec running. NOTE: this no longer implies the next exec will be
    /// refused — a sandbox admits `max_concurrent_exec_per_sandbox` at once.
    /// Use `exec_in_flight` / `exec_slots_free` for admission decisions; a
    /// caller that polls this waiting for `false` will wait forever against a
    /// resident process while slots sit idle.
    pub exec_in_progress: bool,
    /// Execs running right now.
    pub exec_in_flight: u32,
    /// Execs this sandbox would admit before returning S003.
    pub exec_slots_free: u32,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListResponse {
    pub sandboxes: Vec<SandboxSummary>,
}

// ---------------------------------------------------------------------------
// sandbox::catalog::list
// ---------------------------------------------------------------------------

/// `sandbox::catalog::list` request. No fields — kept as a struct (not
/// `()`) so adding optional filters (e.g. `include_oci_refs: bool`) later
/// is a non-breaking serde change.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CatalogListRequest {}

/// One row in the catalog response. `name` is the catalog key that
/// callers pass to `sandbox::create` (or `sandbox::run`); `oci_ref` is the
/// fully-qualified OCI reference the daemon will pull. `kind` lets agents
/// distinguish bundled presets from operator-registered custom images
/// without having to maintain their own preset allowlist.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CatalogEntry {
    pub name: String,
    pub oci_ref: String,
    pub kind: CatalogEntryKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogEntryKind {
    /// Bundled with the daemon binary. Stable identifier across releases.
    Preset,
    /// Operator-registered in the adapter's `custom_images` config.
    /// Specific to this deployment.
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CatalogListResponse {
    /// Every image the daemon will accept on `sandbox::create.image`.
    /// Presets come first in stable order; custom entries follow,
    /// sorted by `name` for determinism so an agent that diffs two
    /// `catalog::list` responses sees stable output.
    pub images: Vec<CatalogEntry>,
}

// ---------------------------------------------------------------------------
// sandbox::run
// ---------------------------------------------------------------------------

/// Optional sibling file to drop into the sandbox before the main code runs.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunFile {
    /// Absolute path inside the sandbox guest where the file lands.
    pub path: String,
    /// UTF-8 file body. Streaming and base64 are not supported here;
    /// callers needing those should use `sandbox::create` + repeated
    /// `sandbox::fs::write` calls instead.
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "run_request_example")]
pub struct RunRequest {
    /// Catalog name of the image to boot (preset or `custom_images` key).
    /// Same value space as `sandbox::create`.
    pub image: String,
    /// The actual code to run. Written to a `/tmp/run.{ext}` file inside
    /// the sandbox; the chosen interpreter runs that file.
    pub code: String,
    /// Selects the interpreter and file extension. Required.
    /// Built-in values: `"node"`, `"python"`, `"shell"`.
    /// Any other string is treated as a literal interpreter binary path
    /// inside the VM and the file is written to `/tmp/run.txt`. There is
    /// no default — there's no honest universal answer to "what language
    /// is this code", and silently defaulting to shell makes Python or
    /// JS code produce confusing line-by-line failures.
    pub lang: String,
    /// Optional sibling files (extra source modules, config, fixtures).
    #[serde(default)]
    pub files: Vec<RunFile>,
    /// Env vars exposed to the interpreter. Accepts both `Vec<"K=V">`
    /// and `{ K: V }` map shapes (same as `sandbox::exec.env`).
    #[serde(default)]
    pub env: EnvShape,
    /// Base64-encoded bytes piped to the interpreter's stdin.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Kill-after window for the interpreter, in ms. Defaults to
    /// 300_000 (5 minutes); pass a smaller value to fail fast on
    /// quick probes.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// If true, the sandbox is NOT stopped after the run completes;
    /// `sandbox_id` is returned so the caller can poke around. Default
    /// false (the sandbox is torn down on either success or failure).
    #[serde(default)]
    pub keep_sandbox: bool,
}

fn run_request_example() -> serde_json::Value {
    serde_json::json!({
        "image": "node",
        "code": "console.log('hello world')",
        "lang": "node",
        "timeout_ms": 300000
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub success: bool,
    /// As `ExecResponse::stdout_truncated`.
    #[serde(default)]
    pub stdout_truncated: bool,
    /// As `ExecResponse::stderr_truncated`.
    #[serde(default)]
    pub stderr_truncated: bool,
    /// Present only if `keep_sandbox: true` was set on the request.
    /// Otherwise null (sandbox auto-stopped).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_id: Option<String>,
}

// ---------------------------------------------------------------------------
// sandbox::fs::*
// ---------------------------------------------------------------------------

/// Single directory entry returned by `sandbox::fs::ls` / `sandbox::fs::stat`.
/// `mode` is the octal permission string (e.g. `"0644"`); `mtime` is
/// Unix seconds. `is_symlink` reflects the entry itself — FS handlers
/// never follow symlinks unless the op doc-comment says otherwise.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct FsEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mode: String,
    pub mtime: i64,
    pub is_symlink: bool,
}

/// Single grep hit. `line` is 1-based. `content` is already truncated
/// to `max_line_bytes` (with a trailing `…`) if the original line was
/// longer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct FsMatch {
    #[serde(alias = "file")]
    pub path: String,
    pub line: u64,
    pub content: String,
}

/// One `sed` file-level outcome. `success=false` carries the human
/// failure message in `error`; otherwise `error` is absent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct FsSedFileResult {
    #[serde(alias = "file")]
    pub path: String,
    pub replacements: u64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "ls_request_example")]
pub struct LsRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path of the directory to list inside the sandbox guest.
    pub path: String,
}

fn ls_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app"
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LsResponse {
    pub entries: Vec<FsEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "stat_request_example")]
pub struct StatRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path to inspect inside the sandbox guest.
    pub path: String,
}

fn stat_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/index.js"
    })
}

/// Mirrors `FsEntry`. Note: the shell protocol does not carry uid/gid;
/// those fields are absent at this trigger level.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatResponse {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mode: String,
    pub mtime: i64,
    pub is_symlink: bool,
}

impl From<FsEntry> for StatResponse {
    fn from(e: FsEntry) -> Self {
        StatResponse {
            name: e.name,
            is_dir: e.is_dir,
            size: e.size,
            mode: e.mode,
            mtime: e.mtime,
            is_symlink: e.is_symlink,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "mkdir_request_example")]
pub struct MkdirRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path of the directory to create inside the sandbox guest.
    pub path: String,
    /// Octal permissions for the new directory (e.g. `"0755"`).
    #[serde(default = "default_dir_mode")]
    pub mode: String,
    /// Create intermediate parent directories like `mkdir -p`.
    #[serde(default)]
    pub parents: bool,
}

fn mkdir_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/cache",
        "mode": "0755",
        "parents": true
    })
}

fn default_dir_mode() -> String {
    "0755".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MkdirResponse {
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "rm_request_example")]
pub struct RmRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path to remove inside the sandbox guest.
    pub path: String,
    /// Remove directories and their contents recursively (like `rm -rf`).
    #[serde(default)]
    pub recursive: bool,
}

fn rm_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/temp",
        "recursive": true
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RmResponse {
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "chmod_request_example")]
pub struct ChmodRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path to modify inside the sandbox guest.
    pub path: String,
    /// Octal permissions (e.g. `"0644"`, `"0755"`).
    pub mode: String,
    /// Optional UID to chown to. Pair with `gid` for a full chown.
    #[serde(default)]
    pub uid: Option<u32>,
    /// Optional GID to chown to. Pair with `uid` for a full chown.
    #[serde(default)]
    pub gid: Option<u32>,
    /// Apply recursively to all entries under `path`.
    #[serde(default)]
    pub recursive: bool,
}

fn chmod_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/script.sh",
        "mode": "0755"
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChmodResponse {
    pub updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "mv_request_example")]
pub struct MvRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Source absolute path inside the sandbox guest.
    pub src: String,
    /// Destination absolute path inside the sandbox guest.
    pub dst: String,
    /// Allow overwriting `dst` if it already exists.
    #[serde(default)]
    pub overwrite: bool,
}

fn mv_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "src": "/home/app/old.js",
        "dst": "/home/app/new.js",
        "overwrite": false
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MvResponse {
    pub moved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "grep_request_example")]
pub struct GrepRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Root path to search inside the sandbox guest. Treated as a directory
    /// when `recursive: true`, else as a single file.
    pub path: String,
    /// Regex pattern (Rust regex syntax, anchored fragments allowed).
    pub pattern: String,
    /// Descend into subdirectories under `path`. Defaults to true.
    #[serde(default = "default_recursive")]
    pub recursive: bool,
    /// Case-insensitive match.
    #[serde(default)]
    pub ignore_case: bool,
    /// Gitignore-style include filter applied relative to `path`.
    #[serde(default)]
    pub include_glob: Vec<String>,
    /// Gitignore-style exclude filter applied relative to `path`.
    #[serde(default)]
    pub exclude_glob: Vec<String>,
    /// Maximum number of matches before truncation. Default 10_000.
    #[serde(default = "default_max_matches")]
    pub max_matches: u64,
    /// Maximum bytes per matched line before content is truncated with `…`.
    /// Default 4096.
    #[serde(default = "default_max_line_bytes")]
    pub max_line_bytes: u64,
}

fn grep_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/src",
        "pattern": "TODO|FIXME",
        "recursive": true,
        "include_glob": ["**/*.js", "**/*.ts"]
    })
}

fn default_recursive() -> bool {
    true
}
fn default_max_matches() -> u64 {
    10_000
}
fn default_max_line_bytes() -> u64 {
    4096
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GrepResponse {
    pub matches: Vec<FsMatch>,
    pub truncated: bool,
}

/// Find-and-replace request, accepting either an explicit `files` list
/// or a `path` walked like grep does. Exactly one of those two must be
/// provided — `handle_sed` returns S210 otherwise.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "sed_request_example")]
pub struct SedRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Legacy form: explicit list of paths to rewrite.
    #[serde(default)]
    pub files: Vec<String>,
    /// New form: walk `path` like grep does. May be a directory or a
    /// single file. Mutually exclusive with `files`.
    #[serde(default)]
    pub path: Option<String>,
    /// Whether to descend into subdirectories. Only meaningful with
    /// `path`. Defaults to `true` so the path-form behaves like grep.
    #[serde(default = "default_true")]
    pub recursive: bool,
    /// Gitignore-style include filter applied to paths relative to
    /// `path`. Only meaningful with `path`.
    #[serde(default)]
    pub include_glob: Vec<String>,
    /// Gitignore-style exclude filter applied to paths relative to
    /// `path`. Only meaningful with `path`.
    #[serde(default)]
    pub exclude_glob: Vec<String>,
    pub pattern: String,
    pub replacement: String,
    #[serde(default = "default_true")]
    pub regex: bool,
    #[serde(default)]
    pub first_only: bool,
    #[serde(default)]
    pub ignore_case: bool,
}

fn default_true() -> bool {
    true
}

fn sed_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/src",
        "pattern": "foo",
        "replacement": "bar",
        "recursive": true
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SedResponse {
    pub results: Vec<FsSedFileResult>,
    pub total_replacements: u64,
}

/// File body for `sandbox::fs::write`. Untagged so the JSON shape
/// decides which variant runs:
///
/// - A bare JSON string (`"console.log('hi')"`) → [`WriteContent::Utf8`].
///   This is what LLM agents naturally pass and the recommended form
///   for source files / configs / small text.
/// - An object that matches [`StreamChannelRef`] → [`WriteContent::Stream`].
///   The existing channel-streaming path for large or binary payloads
///   coming from a programmatic caller that can construct a channel.
///
/// Serde tries variants in declaration order; `Utf8` matches first
/// because every JSON string deserialises into `String`, and
/// `StreamChannelRef` requires an object. Binary inline data uses the
/// separate `content_b64` field on [`WriteRequest`] (not a variant
/// here, so a caller can't accidentally pass base64 expecting it to be
/// decoded — they have to opt in by name).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum WriteContent {
    /// Inline UTF-8 string. Written to the file verbatim.
    Utf8(String),
    /// Streaming channel for large / binary uploads.
    Stream(StreamChannelRef),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "write_request_example")]
pub struct WriteRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute destination path inside the sandbox guest.
    pub path: String,
    /// Octal permissions for the new file (default `"0644"`).
    #[serde(default = "default_file_mode")]
    pub mode: String,
    /// Create missing parent directories before writing.
    #[serde(default)]
    pub parents: bool,
    /// File body. Pass a UTF-8 string for source/text (the agent-natural
    /// form), or a `StreamChannelRef` object for streaming large/binary
    /// uploads. Mutually exclusive with `content_b64`; exactly one of the
    /// two must be set.
    #[serde(default)]
    pub content: Option<WriteContent>,
    /// Base64-encoded inline body for small binary payloads.
    /// Mutually exclusive with `content`.
    #[serde(default)]
    pub content_b64: Option<String>,
}

fn write_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/index.js",
        "content": "console.log('hello world')\n",
        "mode": "0644",
        "parents": true
    })
}

fn default_file_mode() -> String {
    "0644".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WriteResponse {
    pub bytes_written: u64,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(example = "read_request_example")]
pub struct ReadRequest {
    /// UUID returned by `sandbox::create`.
    pub sandbox_id: String,
    /// Absolute path to read inside the sandbox guest.
    pub path: String,
}

fn read_request_example() -> serde_json::Value {
    serde_json::json!({
        "sandbox_id": "00000000-0000-0000-0000-000000000000",
        "path": "/home/app/index.js"
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadResponse {
    /// Channel ref for the file body. Always set; callers can subscribe
    /// to receive the full file contents as bytes. Preserved for wire
    /// compatibility with peers that statically type this field as
    /// `StreamChannelRef`.
    pub content: StreamChannelRef,
    /// Inline UTF-8 body. Populated for text files under the adapter's
    /// inline cap (1 MiB) that decode cleanly as UTF-8.
    /// `None` for large or binary files; subscribe to `content` instead.
    /// When `Some`, the same bytes are also delivered through `content`
    /// so legacy callers keep working — new callers can use `body`
    /// directly and skip the channel subscription.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub body: Option<String>,
    pub size: u64,
    pub mode: String,
    pub mtime: i64,
}
