use iii_sdk::helpers::{
    ChannelDirection, ChannelItem, create_channel, extract_channel_refs, is_channel_ref,
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
}
