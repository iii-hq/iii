use async_trait::async_trait;
use iii_helpers::stream::{
    DeleteResult, SetResult, StreamDeleteInput, StreamGetInput, StreamListGroupsInput,
    StreamListInput, StreamSetInput, StreamUpdateInput, UpdateResult,
};
use serde_json::Value;

use crate::error::Error;

/// Custom stream-provider trait. Implementors override the engine's built-in
/// stream storage for a specific stream name when registered through
/// `create_stream` in the `helpers` submodule.
#[async_trait]
pub trait IStream: Send + Sync + 'static {
    async fn get(&self, input: StreamGetInput) -> Result<Option<Value>, Error>;
    async fn set(&self, input: StreamSetInput) -> Result<Option<SetResult>, Error>;
    async fn delete(&self, input: StreamDeleteInput) -> Result<DeleteResult, Error>;
    async fn list(&self, input: StreamListInput) -> Result<Vec<Value>, Error>;
    async fn list_groups(&self, input: StreamListGroupsInput) -> Result<Vec<String>, Error>;
    async fn update(&self, input: StreamUpdateInput) -> Result<Option<UpdateResult>, Error>;
}
