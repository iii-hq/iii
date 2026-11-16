// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Request and response types of the `<adapter>::worker::*` operations. `iii sandbox` calls
//! them to run one Compose container's VM through the adapter.
//!
//! Every request carries `auth`: the project token, or, with
//! [`FEATURE_SCOPED_AUTH`](crate::FEATURE_SCOPED_AUTH), the token scoped to the VM's state key
//! ([`scoped_auth_token`]). A scoped token controls that one VM and no other.

use crate::channel::StreamChannelRef;
use hmac::{Hmac, Mac};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::BTreeMap;

/// What the scoped token's MAC covers before the state key, so it can never equal another MAC
/// of the project token over the same bytes. A different derivation gets a new version.
const SCOPED_AUTH_DOMAIN: &[u8] = b"iii-sandbox/worker-scope/v1\0";

/// The token that authorizes `worker::*` calls for the VM of `state_key` alone: the lowercase
/// hex of HMAC-SHA256 keyed with the project token over `iii-sandbox/worker-scope/v1\0`
/// followed by the state key. Compose hands it to the container that runs the VM; the adapter
/// recomputes it from the project token it already holds, so neither side stores a token per VM.
/// `tests/fixtures/scoped_auth.json` pins it.
pub fn scoped_auth_token(project_token: &str, state_key: &str) -> String {
    hex::encode(scoped_mac(project_token, state_key).finalize().into_bytes())
}

/// Whether `presented` is the scoped token for `state_key`, compared in constant time. Never
/// with an empty project token, whose MAC anyone can compute, or an empty state key.
pub fn scoped_auth_matches(project_token: &str, state_key: &str, presented: &str) -> bool {
    if project_token.is_empty() || state_key.is_empty() {
        return false;
    }
    let Ok(tag) = hex::decode(presented) else {
        return false;
    };
    scoped_mac(project_token, state_key)
        .verify_slice(&tag)
        .is_ok()
}

fn scoped_mac(project_token: &str, state_key: &str) -> Hmac<Sha256> {
    // HMAC takes a key of any length; only fixed-size MACs can refuse one.
    let mut mac = Hmac::<Sha256>::new_from_slice(project_token.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(SCOPED_AUTH_DOMAIN);
    mac.update(state_key.as_bytes());
    mac
}

/// Where the worker's code comes from.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerSource {
    Bundle {
        /// Manifest name, not the container key.
        package: String,
        /// The container's directory in the project's `.iii/workers/`,
        /// `<project>/.iii/workers/<container>`. `worker::build` copies it into the image; a
        /// `worker::start` with `build` does not mount it. Before contract 1.3 it was the bundle
        /// in Compose's package cache.
        host_path: String,
    },
    /// Local sources always sync into the VM.
    Local {
        host_path: String,
        run_override: Option<String>,
    },
}

/// Idempotent image and base-layer pull; long-running.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerPrepareRequest {
    /// The project token or the VM's scoped token.
    pub auth: String,
    pub source: WorkerSource,
    /// The VM the prepare is for. Required with a scoped token, which the adapter checks
    /// against it. Since contract 1.2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerPrepareResponse {}

/// Builds the image of one container's VM; long-running. Every call builds anew: there is no
/// input-based skip, so a rebuild picks up a moved `:latest` base. The build pulls its base image
/// itself; no `worker::prepare` comes first. Since contract 1.3
/// ([`FEATURE_BUILD`](crate::FEATURE_BUILD)).
///
/// The adapter writes `image.erofs` and then `image.id` (the `build_id`) into `output_dir`, each
/// as a temporary file renamed into place. It builds in a build VM of its own, so a VM running
/// for the same `state_key` keeps the image it booted. One build per `state_key` runs at a time;
/// a second one gets `S509`. A setup or install step that exits non-zero, or an image that
/// cannot be written, fails with `S507`.
///
/// The build lives as long as its caller reads `logs`. When the channel closes or a write to it
/// fails, the adapter stops the build VM, deletes the temporary image, keeps the previous
/// `image.erofs` and `image.id`, and releases the `state_key`, so a retry does not get `S509`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerBuildRequest {
    /// The project token or the VM's scoped token.
    pub auth: String,
    /// The VM the build is for; a scoped token is checked against it.
    pub state_key: String,
    /// Container key, for logs and errors.
    pub worker_name: String,
    pub source: WorkerSource,
    /// Absolute host directory of the build: `<project>/.iii/build/<container>`. Must be under the
    /// adapter's host roots. The adapter writes `image.erofs` there through a temporary file
    /// and a rename.
    pub output_dir: String,
    /// Env of the setup/install steps: `guest_env`, nothing from the host.
    pub env: BTreeMap<String, String>,
    /// Writer end of a channel for the build's output, one `LogRecord` JSON per text frame.
    /// Required: the build lives as long as the caller reads it. When it has had no output to
    /// write for 5 s, the adapter writes a one-byte binary frame to keep the channel alive;
    /// readers take records from text frames only.
    pub logs: StreamChannelRef,
}

/// Reply of `worker::build`, once the image is in place. Since contract 1.3.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerBuildResponse {
    /// Digest of the image written, `sha256:<hex>`; passed back on `worker::start`.
    pub build_id: String,
    /// The base image the build started from.
    pub base_image: BaseImageRef,
    /// When the build finished, in milliseconds since the Unix epoch.
    pub built_at_ms: u64,
}

/// The base image a build started from. Since contract 1.3.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BaseImageRef {
    /// The image reference the adapter pulled, e.g. `docker.io/iiidev/node:latest`.
    pub reference: String,
    /// Resolved digest, `None` when the cached rootfs carries none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// Boots the VM. The adapter reads resources from the worker's manifest.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerStartRequest {
    /// The project token or the VM's scoped token.
    pub auth: String,
    /// Hash of the compose state dir and container key; the same key gets the same persistent upper.
    pub state_key: String,
    /// Container key; becomes `III_WORKER_NAME`.
    pub worker_name: String,
    pub source: WorkerSource,
    /// Complete guest env. Never on the `__vm-boot` argv: the adapter writes it
    /// to `__vm-boot`'s stdin, which sends it to the guest init over the
    /// `iii.env` virtio-console port, not the kernel cmdline (only a build
    /// without an embedded init still uses the cmdline).
    pub env: BTreeMap<String, String>,
    /// The engine's URL as seen from the host.
    pub engine_url: String,
    /// Boot the image `worker::build` wrote, without running `scripts.setup`/`scripts.install`.
    /// With it, the caller does not call `worker::prepare`. An older adapter ignores the field,
    /// so a caller sends it only to one that announces [`FEATURE_BUILD`](crate::FEATURE_BUILD).
    /// Since contract 1.3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<WorkerBuildRef>,
}

/// The build `worker::start` boots. The adapter compares `build_id` with `build_dir/image.id`
/// and refuses a mismatch or a missing file with `S508`; it does not hash the image on start.
/// Since contract 1.3.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerBuildRef {
    /// [`WorkerBuildResponse::build_id`] of the build.
    pub build_id: String,
    /// The `output_dir` the build was written to.
    pub build_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerStartResponse {
    /// Opaque; stable per `state_key`.
    pub vm_id: String,
    /// Increments on every boot of this `vm_id`.
    pub instance: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerStatusRequest {
    pub auth: String,
    pub vm_id: String,
    pub instance: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Preparing,
    Running,
    Exited,
    Stopped,
    Unknown,
}

/// Reply of `worker::status` and `worker::stop`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerStatus {
    pub vm_id: String,
    pub instance: u64,
    pub state: State,
    pub exit_code: Option<i32>,
    /// The signal that ended the VM, if one did.
    pub signal: Option<i32>,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerLogsRequest {
    pub auth: String,
    pub vm_id: String,
    /// `LogRecord.cursor` of the last record the caller has; reading resumes right after it.
    /// Opaque to the caller. A cursor past the end of the log (after a rotation) reads from the
    /// start.
    pub cursor: Option<u64>,
    /// Keep the channel open while the instance runs.
    pub follow: bool,
    /// Writer end of a channel the caller created; one `LogRecord` JSON per text frame.
    pub channel: StreamChannelRef,
    /// Start after the record that is last when the request arrives, ignoring `cursor`: a client
    /// that reattaches to a running instance gets only what follows, not the log again. Since
    /// contract 1.2 ([`FEATURE_LOGS_FROM_END`](crate::FEATURE_LOGS_FROM_END)); an older adapter
    /// ignores it. The response's `cursor` says where the stream started.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_end: bool,
}

/// Returned once the log pump starts; the records travel over the channel.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct WorkerLogsResponse {
    /// With `from_end`: the cursor of the record that was last when the request arrived, the
    /// one the stream starts after, so a stream reopened later resumes there instead of at the
    /// end again; `None` when the log was empty. Since contract 1.2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogRecord {
    pub stream: LogStream,
    pub line: String,
    pub ts_ms: u64,
    /// Resume point: pass it as `WorkerLogsRequest.cursor` to continue after this record.
    pub cursor: u64,
}

/// Idempotent; `grace_ms: 0` kills. Replies with the final `WorkerStatus`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerStopRequest {
    pub auth: String,
    pub vm_id: String,
    /// Names the boot to stop, so a stale stop cannot hit a newer one.
    pub instance: u64,
    pub grace_ms: u64,
}

/// Refused while the VM is running.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerDestroyRequest {
    pub auth: String,
    pub state_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerDestroyResponse {
    pub destroyed: bool,
}
