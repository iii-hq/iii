#![allow(deprecated)] // exercises the deprecated legacy IStream provider surface

use iii_helpers::stream::{
    StreamDeleteInput, StreamDeleteResult, StreamGetInput, StreamListGroupsInput, StreamListInput,
    StreamSetInput, StreamSetResult, StreamUpdateInput, StreamUpdateResult,
};
use iii_sdk::IStream;
use serde_json::Value;

use std::pin::Pin;

struct DummyStream;

impl IStream for DummyStream {
    fn get<'a>(
        &'a self,
        _: StreamGetInput,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, iii_sdk::Error>> + Send + 'a>> {
        Box::pin(async move { Ok(None) })
    }
    fn set<'a>(
        &'a self,
        _: StreamSetInput,
    ) -> Pin<Box<dyn Future<Output = Result<Option<StreamSetResult>, iii_sdk::Error>> + Send + 'a>>
    {
        Box::pin(async move { Ok(None) })
    }
    fn delete<'a>(
        &'a self,
        _: StreamDeleteInput,
    ) -> Pin<Box<dyn Future<Output = Result<StreamDeleteResult, iii_sdk::Error>> + Send + 'a>> {
        Box::pin(async move { Ok(StreamDeleteResult::default()) })
    }
    fn list<'a>(
        &'a self,
        _: StreamListInput,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, iii_sdk::Error>> + Send + 'a>> {
        Box::pin(async move { Ok(vec![]) })
    }
    fn list_groups<'a>(
        &'a self,
        _: StreamListGroupsInput,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, iii_sdk::Error>> + Send + 'a>> {
        Box::pin(async move { Ok(vec![]) })
    }
    fn update<'a>(
        &'a self,
        _: StreamUpdateInput,
    ) -> Pin<Box<dyn Future<Output = Result<Option<StreamUpdateResult>, iii_sdk::Error>> + Send + 'a>>
    {
        Box::pin(async move { Ok(None) })
    }
}

#[test]
fn dummy_stream_implements_istream() {
    let _: Box<dyn IStream> = Box::new(DummyStream);
}
