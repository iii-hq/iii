use async_trait::async_trait;
use iii_helpers::stream::{
    StreamDeleteInput, StreamDeleteResult, StreamGetInput, StreamListGroupsInput, StreamListInput,
    StreamSetInput, StreamSetResult, StreamUpdateInput, StreamUpdateResult,
};
use iii_sdk::IStream;
use serde_json::Value;

struct DummyStream;

#[async_trait]
impl IStream for DummyStream {
    async fn get(&self, _: StreamGetInput) -> Result<Option<Value>, iii_sdk::Error> {
        Ok(None)
    }
    async fn set(&self, _: StreamSetInput) -> Result<Option<StreamSetResult>, iii_sdk::Error> {
        Ok(None)
    }
    async fn delete(&self, _: StreamDeleteInput) -> Result<StreamDeleteResult, iii_sdk::Error> {
        Ok(StreamDeleteResult::default())
    }
    async fn list(&self, _: StreamListInput) -> Result<Vec<Value>, iii_sdk::Error> {
        Ok(vec![])
    }
    async fn list_groups(&self, _: StreamListGroupsInput) -> Result<Vec<String>, iii_sdk::Error> {
        Ok(vec![])
    }
    async fn update(
        &self,
        _: StreamUpdateInput,
    ) -> Result<Option<StreamUpdateResult>, iii_sdk::Error> {
        Ok(None)
    }
}

#[test]
fn dummy_stream_implements_istream() {
    let _: Box<dyn IStream> = Box::new(DummyStream);
}
