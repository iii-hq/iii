// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Same JSON as `iii_sdk::channels::StreamChannelRef`, pinned by `tests/stream_channel_ref.rs`.
//! Copied rather than re-exported so the contract crate does not depend on the SDK.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChannelDirection {
    #[default]
    Read,
    Write,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct StreamChannelRef {
    pub channel_id: String,
    pub access_key: String,
    pub direction: ChannelDirection,
}
