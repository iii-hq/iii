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
use iii_sdk::{
    UpdateOp, UpdateResult,
    types::{DeleteResult, SetResult},
};
use serde_json::Value;

use crate::{
    builtins::pubsub_lite::Subscriber,
    modules::stream::{StreamMetadata, StreamWrapperMessage},
};

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
