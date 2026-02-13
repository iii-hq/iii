// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod bridge;
pub mod kv_store;
pub mod redis_adapter;

use async_trait::async_trait;
use iii_sdk::{UpdateOp, UpdateResult, types::SetResult};
use serde_json::Value;

#[async_trait]
pub trait StateAdapter: Send + Sync {
    async fn set(&self, scope: &str, key: &str, data: Value) -> anyhow::Result<SetResult>;
    async fn get(&self, scope: &str, key: &str) -> anyhow::Result<Option<Value>>;
    async fn delete(&self, scope: &str, key: &str) -> anyhow::Result<()>;
    async fn update(
        &self,
        scope: &str,
        key: &str,
        ops: Vec<UpdateOp>,
    ) -> anyhow::Result<UpdateResult>;
    async fn list(&self, scope: &str) -> anyhow::Result<Vec<Value>>;
    async fn list_groups(&self) -> anyhow::Result<Vec<String>>;
    async fn destroy(&self) -> anyhow::Result<()>;
}
