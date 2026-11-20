// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! `sandbox::build` and `sandbox::start`: one Compose container's VM, built and run through the
//! router on top of the adapter's `worker::*` calls (tech-specs/2026-11-19-iii-call-sandbox,
//! section 4). A container's scripts call them with `iii call`, which binds its stdout and stderr
//! to the `stdout` and `stderr` sinks; Compose hands the rest of the request in
//! `III_CALL_DEFAULTS`.

use std::collections::BTreeMap;

use schemars::{JsonSchema, r#gen::SchemaGenerator, schema::Schema};
use serde::{Deserialize, Serialize};

use crate::{BaseImageRef, ContractVersion, StreamChannelRef, WorkerSource};

/// Builds a container's VM image.
pub const SANDBOX_BUILD: &str = "sandbox::build";
/// Boots a container's last build and follows it until it exits or its caller leaves.
pub const SANDBOX_START: &str = "sandbox::start";

/// The schema of a `stdout` or `stderr` sink: the write end of a pipe, as every SDK describes an
/// `OutputStream`, so `iii call` binds its own output to it.
fn output_stream(_: &mut SchemaGenerator) -> Schema {
    serde_json::from_value(serde_json::json!({
        "title": "OutputStream",
        "description": "A stream the function writes: the write end of an engine pipe. `iii call` binds its own output to it.",
        "type": "object",
        "x-iii-stream": "write",
        "required": ["access_key", "channel_id", "direction"],
        "properties": {
            "channel_id": { "type": "string" },
            "access_key": { "type": "string" },
            "direction": { "type": "string", "const": "write" }
        }
    }))
    .expect("the OutputStream schema is a schema")
}

/// The request of `sandbox::build`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SandboxBuildRequest {
    /// The container key, the VM's worker name.
    pub worker: String,
    /// The adapter's name, such as `sandbox-libkrun`.
    pub adapter: String,
    /// The namespace the adapter's functions are registered in.
    pub namespace: String,
    /// The VM's scoped token, or the project token.
    pub auth: String,
    /// The VM's state key.
    pub state_key: String,
    pub source: WorkerSource,
    /// The environment of the build's setup and install steps: the worker's own variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Absolute host directory of the build, `<project>/.iii/build/<container>`.
    pub build_dir: String,
    /// The image the build starts from, over the manifest's `runtime.base_image`. A binary
    /// package needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_image: Option<String>,
    /// How long the build may take, in milliseconds. No limit when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Where the build's stdout goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "output_stream")]
    pub stdout: Option<StreamChannelRef>,
    /// Where the build's stderr goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "output_stream")]
    pub stderr: Option<StreamChannelRef>,
}

/// The answer of `sandbox::build`, once the image is in place.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SandboxBuildResponse {
    /// Digest of the image written, `sha256:<hex>`, also in `image.id`.
    pub build_id: String,
    /// The base image the build started from.
    pub base_image: BaseImageRef,
    /// When the build finished, in milliseconds since the Unix epoch.
    pub built_at_ms: u64,
    /// The adapter that built it, as `adapter::info` names it.
    pub adapter: String,
    pub adapter_version: String,
    /// The sandbox contract the adapter implements.
    pub contract: ContractVersion,
}

/// The request of `sandbox::start`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SandboxStartRequest {
    /// The container key, the VM's worker name.
    pub worker: String,
    /// The adapter's name, such as `sandbox-libkrun`.
    pub adapter: String,
    /// The namespace the adapter's functions are registered in.
    pub namespace: String,
    /// The VM's scoped token, or the project token.
    pub auth: String,
    /// The VM's state key: the same key boots the same persistent data.
    pub state_key: String,
    pub source: WorkerSource,
    /// The guest's whole environment: the worker's own variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The directory the build was written to.
    pub build_dir: String,
    /// The image to boot, as its build recorded it.
    pub build_id: String,
    /// The engine the guest connects to.
    pub engine_url: String,
    /// How long a stop may take once the caller left, in milliseconds. 10 s when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_grace_ms: Option<u64>,
    /// The command the guest runs instead of its manifest's `scripts.start`. Only for a
    /// `path://` worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Where the guest's stdout goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "output_stream")]
    pub stdout: Option<StreamChannelRef>,
    /// Where the guest's stderr goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "output_stream")]
    pub stderr: Option<StreamChannelRef>,
}

/// The answer of `sandbox::start`: the VM exited with 0, or its caller left and it was stopped.
/// A VM that exits otherwise fails the call with `WORKER_EXITED`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SandboxStartResponse {
    pub vm_id: String,
    pub instance: u64,
    /// The caller left, and the VM was stopped.
    pub stopped: bool,
    /// The VM's exit code, when it reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// The signal that ended the VM, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sinks_are_described_as_output_streams() {
        for schema in [
            serde_json::to_value(schemars::schema_for!(SandboxBuildRequest)).unwrap(),
            serde_json::to_value(schemars::schema_for!(SandboxStartRequest)).unwrap(),
        ] {
            for field in ["stdout", "stderr"] {
                let property = &schema["properties"][field];
                assert_eq!(
                    property["properties"]["direction"]["const"], "write",
                    "{field}"
                );
                assert_eq!(property["x-iii-stream"], "write", "{field}");
            }
        }
    }

    #[test]
    fn a_request_reads_with_unknown_fields_and_without_sinks() {
        let request: SandboxStartRequest = serde_json::from_value(serde_json::json!({
            "worker": "api", "adapter": "sandbox-libkrun", "namespace": "default",
            "auth": "t", "state_key": "s",
            "source": { "kind": "local", "host_path": "/p/api" },
            "build_dir": "/p/.iii/build/api", "build_id": "sha256:1",
            "engine_url": "ws://127.0.0.1:49134", "timeout_ms": 5
        }))
        .unwrap();
        assert_eq!(request.worker, "api");
        assert!(request.stdout.is_none() && request.command.is_none());
    }
}
