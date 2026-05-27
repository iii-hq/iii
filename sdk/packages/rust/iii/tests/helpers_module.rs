use iii_sdk::helpers::{
    ChannelDirection, ChannelItem, create_channel, extract_channel_refs, is_channel_ref,
    register_trigger_type, unregister_trigger_type,
};
use serde_json::json;

#[test]
fn helpers_module_reexports_channel_utilities() {
    let value = json!({});
    assert!(!is_channel_ref(&value));
    let refs = extract_channel_refs(&value);
    assert!(refs.is_empty());
    let _: ChannelDirection = ChannelDirection::Read;
    let _: ChannelItem = ChannelItem::Text("x".into());
}

#[test]
fn helpers_module_exposes_free_functions() {
    // `create_channel` is async — referencing the symbol is enough to prove it exists.
    let _ = create_channel;
    let _ = unregister_trigger_type as fn(&iii_sdk::III, String);
    // `register_trigger_type` is generic — typed function-pointer cast is impractical;
    // existence in the import above is enough to verify the symbol exists.
    let _ = register_trigger_type::<DummyHandler, (), ()>;
}

struct DummyHandler;

#[async_trait::async_trait]
impl iii_sdk::TriggerHandler for DummyHandler {
    async fn register_trigger(
        &self,
        _: iii_sdk::TriggerConfig,
    ) -> Result<(), iii_sdk::IIIError> {
        Ok(())
    }
    async fn unregister_trigger(
        &self,
        _: iii_sdk::TriggerConfig,
    ) -> Result<(), iii_sdk::IIIError> {
        Ok(())
    }
}
