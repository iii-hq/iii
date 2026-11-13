// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Contract version and the `adapter::info` handshake types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const CONTRACT_MAJOR: u32 = 1;
pub const CONTRACT_MINOR: u32 = 4;

/// `worker::*` accepts a token scoped to one VM, [`crate::scoped_auth_token`], besides the
/// project token. Since contract 1.2.
pub const FEATURE_SCOPED_AUTH: &str = "worker.scoped_auth";
/// The adapter stops a VM whose last log follower is gone and that no client reattaches to
/// within its grace. Since contract 1.2.
pub const FEATURE_LEASE: &str = "worker.lease";
/// `worker::logs` honours [`crate::WorkerLogsRequest::from_end`]. Since contract 1.2.
pub const FEATURE_LOGS_FROM_END: &str = "worker.logs_from_end";
/// `worker::build`, and `worker::start` booting the image it built. Since contract 1.3.
///
/// An adapter advertises it only when it can boot a build (overlay mode); `sandbox-libkrun` in
/// legacy mode (`III_ROOTFS_MODE=legacy`, or a build without an embedded iii-init) does not.
pub const FEATURE_BUILD: &str = "worker.build";
/// `worker::build` and `worker::start` take a [`crate::WorkerSource::Binary`], and
/// `worker::build` honours [`crate::WorkerBuildRequest::base_image`]. Since contract 1.4.
pub const FEATURE_BINARY: &str = "worker.binary";
/// `worker::destroy` keeps the VM's data disk unless [`crate::WorkerDestroyRequest::prune`] is
/// set. Since contract 1.4.
pub const FEATURE_PRUNE: &str = "worker.prune";

/// Contract version. A different major is incompatible (S302); a minor only adds fields.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContractVersion {
    pub major: u32,
    pub minor: u32,
}

impl ContractVersion {
    pub const CURRENT: Self = Self {
        major: CONTRACT_MAJOR,
        minor: CONTRACT_MINOR,
    };
}

/// Request of `<adapter>::adapter::info` and `sandbox::adapter::info`; carries no fields.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdapterInfoRequest {}

/// Reply of `<adapter>::adapter::info`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdapterInfo {
    /// The adapter's name, e.g. `"sandbox-libkrun"`.
    pub adapter: String,
    pub adapter_version: String,
    /// The contract version the adapter implements.
    pub contract: ContractVersion,
    /// What the adapter implements beyond the base contract, e.g. [`FEATURE_SCOPED_AUTH`].
    /// Since contract 1.2; an older adapter sends none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}

impl AdapterInfo {
    /// Whether the adapter announced `feature`.
    pub fn supports(&self, feature: &str) -> bool {
        self.features.iter().any(|announced| announced == feature)
    }
}

/// Reply of `sandbox::adapter::info`, which the router answers itself.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RouterInfo {
    /// The bound adapter; `None` when no adapter is bound.
    pub adapter: Option<String>,
    /// The adapter's function namespace; `None` when no adapter is bound.
    pub namespace: Option<String>,
    /// The engine's own `ContractVersion::CURRENT`.
    pub contract: ContractVersion,
    /// The adapter's own info; `None` when unbound, or when the adapter is not connected
    /// or does not answer within 2 s.
    pub info: Option<AdapterInfo>,
}
