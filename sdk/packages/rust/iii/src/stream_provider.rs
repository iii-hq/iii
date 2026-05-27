use async_trait::async_trait;
use serde_json::Value;

use crate::error::IIIError;
use crate::types::{
    DeleteResult, SetResult, StreamDeleteInput, StreamGetInput, StreamListGroupsInput,
    StreamListInput, StreamSetInput, StreamUpdateInput, UpdateResult,
};

/// Custom stream-provider trait. Implementors override the engine's built-in
/// stream storage for a specific stream name when registered through
/// `create_stream` in the `helpers` submodule.
#[async_trait]
pub trait IStream: Send + Sync + 'static {
    async fn get(&self, input: StreamGetInput) -> Result<Option<Value>, IIIError>;
    async fn set(&self, input: StreamSetInput) -> Result<Option<SetResult>, IIIError>;
    async fn delete(&self, input: StreamDeleteInput) -> Result<DeleteResult, IIIError>;
    async fn list(&self, input: StreamListInput) -> Result<Vec<Value>, IIIError>;
    async fn list_groups(
        &self,
        input: StreamListGroupsInput,
    ) -> Result<Vec<String>, IIIError>;
    async fn update(
        &self,
        input: StreamUpdateInput,
    ) -> Result<Option<UpdateResult>, IIIError>;
}
