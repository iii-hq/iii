// The trait signatures name the deprecated legacy `Stream*Input` models.
#![allow(deprecated)]

use iii_helpers::stream::{
    StreamDeleteInput, StreamDeleteResult, StreamGetInput, StreamListGroupsInput, StreamListInput,
    StreamSetInput, StreamSetResult, StreamUpdateInput, StreamUpdateResult,
};
use serde_json::Value;

use crate::error::Error;

/// Custom stream-provider trait. Implementors override the engine's built-in
/// stream storage for a specific stream name when registered through
/// `create_stream` in the `helpers` submodule.
#[deprecated(
    note = "IStream is deprecated (iii-stream) and will be removed in an upcoming release. Behavior is unchanged for now. Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams"
)]
pub trait IStream: Send + Sync + 'static {
    fn get(
        &self,
        input: StreamGetInput,
    ) -> impl Future<Output = Result<Option<Value>, Error>> + Send;
    fn set(
        &self,
        input: StreamSetInput,
    ) -> impl Future<Output = Result<Option<StreamSetResult>, Error>> + Send;
    fn delete(
        &self,
        input: StreamDeleteInput,
    ) -> impl Future<Output = Result<StreamDeleteResult, Error>> + Send;
    fn list(
        &self,
        input: StreamListInput,
    ) -> impl Future<Output = Result<Vec<Value>, Error>> + Send;
    fn list_groups(
        &self,
        input: StreamListGroupsInput,
    ) -> impl Future<Output = Result<Vec<String>, Error>> + Send;
    fn update(
        &self,
        input: StreamUpdateInput,
    ) -> impl Future<Output = Result<Option<StreamUpdateResult>, Error>> + Send;
}
