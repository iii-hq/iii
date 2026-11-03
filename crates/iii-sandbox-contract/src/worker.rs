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

/// The token that authorizes `worker::*` calls for the VM of `state_key` alone: the lowercase
/// hex of HMAC-SHA256 keyed with the project token over the state key. Compose hands it to the
/// container that runs the VM; the adapter recomputes it from the project token it already
/// holds, so neither side stores a token per VM. `tests/fixtures/scoped_auth.json` pins it.
pub fn scoped_auth_token(project_token: &str, state_key: &str) -> String {
    hex::encode(scoped_mac(project_token, state_key).finalize().into_bytes())
}

/// Whether `presented` is the scoped token for `state_key`, compared in constant time.
pub fn scoped_auth_matches(project_token: &str, state_key: &str, presented: &str) -> bool {
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
        /// The bundle in Compose's package cache.
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
    /// ignores it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_end: bool,
}

/// Returned once the log pump starts; the records travel over the channel.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkerLogsResponse {}

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
