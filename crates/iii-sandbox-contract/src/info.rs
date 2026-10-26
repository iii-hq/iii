// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Contract version and the `adapter::info` handshake types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const CONTRACT_MAJOR: u32 = 1;
pub const CONTRACT_MINOR: u32 = 1;

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
    /// The adapter's own info; `None` when unbound or the adapter is not connected.
    pub info: Option<AdapterInfo>,
}
