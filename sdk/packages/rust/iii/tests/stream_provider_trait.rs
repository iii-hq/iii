use async_trait::async_trait;
use iii_sdk::{
    DeleteResult, IStream, SetResult, StreamDeleteInput, StreamGetInput, StreamListGroupsInput,
    StreamListInput, StreamSetInput, StreamUpdateInput, UpdateResult,
};
use serde_json::Value;

struct DummyStream;

#[async_trait]
impl IStream for DummyStream {
    async fn get(&self, _: StreamGetInput) -> Result<Option<Value>, iii_sdk::IIIError> {
        Ok(None)
    }
    async fn set(&self, _: StreamSetInput) -> Result<Option<SetResult>, iii_sdk::IIIError> {
        Ok(None)
    }
    async fn delete(&self, _: StreamDeleteInput) -> Result<DeleteResult, iii_sdk::IIIError> {
        Ok(DeleteResult::default())
    }
    async fn list(&self, _: StreamListInput) -> Result<Vec<Value>, iii_sdk::IIIError> {
        Ok(vec![])
    }
    async fn list_groups(
        &self,
        _: StreamListGroupsInput,
    ) -> Result<Vec<String>, iii_sdk::IIIError> {
        Ok(vec![])
    }
    async fn update(
        &self,
        _: StreamUpdateInput,
    ) -> Result<Option<UpdateResult>, iii_sdk::IIIError> {
        Ok(None)
    }
}

#[test]
fn dummy_stream_implements_istream() {
    let _: Box<dyn IStream> = Box::new(DummyStream);
}
