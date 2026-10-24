// The trait signatures name the deprecated legacy `Stream*Input` models.
#![allow(deprecated)]

use std::pin::Pin;

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
    fn get<'a>(
        &'a self,
        input: StreamGetInput,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, Error>> + Send +'a >>;
    fn set<'a>(
        &'a self,
        input: StreamSetInput,
    ) -> Pin<Box<dyn Future<Output = Result<Option<StreamSetResult>, Error>> + Send+'a >>;
    fn delete<'a>(
        &'a self,
        input: StreamDeleteInput,
    ) -> Pin<Box<dyn Future<Output = Result<StreamDeleteResult, Error>> + Send+'a >>;
    fn list<'a>(
        &'a self,
        input: StreamListInput,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, Error>> + Send+'a >>;
    fn list_groups<'a>(
        &'a self,
        input: StreamListGroupsInput,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, Error>> + Send+'a >>;
    fn update<'a>(
        &'a self,
        input: StreamUpdateInput,
    ) -> Pin<Box<dyn Future<Output = Result<Option<StreamUpdateResult>, Error>> + Send+'a >>;
}
