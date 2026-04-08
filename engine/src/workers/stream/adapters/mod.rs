// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod bridge;
pub mod kv_store;
pub mod redis_adapter;

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use iii_sdk::{
    UpdateOp, UpdateResult,
    types::{DeleteResult, SetResult},
};
use serde_json::Value;

use crate::{
    builtins::pubsub_lite::Subscriber,
    workers::stream::{StreamMetadata, StreamWrapperMessage},
};

const STREAM_KEY_PREFIX: &str = "stream:";
const ENCODED_STREAM_NAME_PREFIX: &str = "b64~";

pub(super) fn stream_storage_key(stream_name: &str, group_id: &str) -> String {
    format!(
        "{}{}:{}",
        STREAM_KEY_PREFIX,
        encode_stream_name_segment(stream_name),
        group_id
    )
}

pub(super) fn stream_storage_prefix(stream_name: &str) -> String {
    format!(
        "{}{}:",
        STREAM_KEY_PREFIX,
        encode_stream_name_segment(stream_name)
    )
}

pub(super) fn parse_stream_storage_key(key: &str) -> Option<(String, String)> {
    let rest = key.strip_prefix(STREAM_KEY_PREFIX)?;
    let (stream_name, group_id) = rest.split_once(':')?;
    let stream_name = decode_stream_name_segment(stream_name)?;
    Some((stream_name, group_id.to_string()))
}

fn encode_stream_name_segment(stream_name: &str) -> String {
    if stream_name.contains(':') || stream_name.starts_with(ENCODED_STREAM_NAME_PREFIX) {
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(stream_name);
        format!("{ENCODED_STREAM_NAME_PREFIX}{encoded}")
    } else {
        stream_name.to_string()
    }
}

fn decode_stream_name_segment(segment: &str) -> Option<String> {
    if let Some(encoded) = segment.strip_prefix(ENCODED_STREAM_NAME_PREFIX) {
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .ok()?;
        String::from_utf8(decoded).ok()
    } else {
        Some(segment.to_string())
    }
}

#[async_trait]
pub trait StreamAdapter: Send + Sync {
    async fn set(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        data: Value,
    ) -> anyhow::Result<SetResult>;

    async fn get(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
    ) -> anyhow::Result<Option<Value>>;

    async fn delete(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
    ) -> anyhow::Result<DeleteResult>;

    async fn get_group(&self, stream_name: &str, group_id: &str) -> anyhow::Result<Vec<Value>>;

    async fn list_groups(&self, stream_name: &str) -> anyhow::Result<Vec<String>>;

    /// List all available stream with their metadata
    async fn list_all_stream(&self) -> anyhow::Result<Vec<StreamMetadata>>;

    async fn emit_event(&self, message: StreamWrapperMessage) -> anyhow::Result<()>;

    async fn subscribe(
        &self,
        id: String,
        connection: Arc<dyn StreamConnection>,
    ) -> anyhow::Result<()>;

    async fn unsubscribe(&self, id: String) -> anyhow::Result<()>;

    async fn watch_events(&self) -> anyhow::Result<()>;

    async fn destroy(&self) -> anyhow::Result<()>;

    async fn update(
        &self,
        stream_name: &str,
        group_id: &str,
        item_id: &str,
        ops: Vec<UpdateOp>,
    ) -> anyhow::Result<UpdateResult>;
}

#[async_trait]
pub trait StreamConnection: Subscriber + Send + Sync {
    async fn cleanup(&self);

    /// Handle a stream message that has already been deserialized.
    /// This is the optimized path - deserialize once, call many times.
    async fn handle_stream_message(&self, msg: &StreamWrapperMessage) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::{parse_stream_storage_key, stream_storage_key, stream_storage_prefix};

    #[test]
    fn stream_storage_key_preserves_group_ids_with_colons() {
        let key = stream_storage_key("orders", "region:us");
        let parsed = parse_stream_storage_key(&key).unwrap();
        assert_eq!(parsed.0, "orders");
        assert_eq!(parsed.1, "region:us");
    }

    #[test]
    fn stream_storage_key_encodes_stream_names_with_colons() {
        let key = stream_storage_key("orders:v2", "region:us");
        let parsed = parse_stream_storage_key(&key).unwrap();
        assert_eq!(parsed.0, "orders:v2");
        assert_eq!(parsed.1, "region:us");
        assert_eq!(
            stream_storage_prefix("orders:v2"),
            "stream:b64~b3JkZXJzOnYy:"
        );
    }

    #[test]
    fn stream_storage_key_encodes_names_that_start_with_reserved_prefix() {
        let key = stream_storage_key("b64~orders", "default");
        let parsed = parse_stream_storage_key(&key).unwrap();
        assert_eq!(parsed.0, "b64~orders");
        assert_eq!(parsed.1, "default");
    }
}
