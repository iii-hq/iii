//! The low-level channel items live under `iii_sdk::protocol`. `create_channel`
//! and the `helpers` re-exports of these items are gone (compile-fail
//! doctests in `src/lib.rs` pin that).

use iii_sdk::protocol::{
    ChannelDirection, ChannelItem, StreamChannelRef, extract_channel_refs, is_channel_ref,
};
use serde_json::json;

#[test]
fn protocol_module_exports_channel_utilities() {
    let value = json!({});
    assert!(!is_channel_ref(&value));
    let refs = extract_channel_refs(&value);
    assert!(refs.is_empty());
    let _: ChannelDirection = ChannelDirection::Read;
    let _: ChannelItem = ChannelItem::Text("x".into());

    let wire = json!({ "body": { "channel_id": "c", "access_key": "k", "direction": "read" } });
    let refs = extract_channel_refs(&wire);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].0, "body");
    let expected: StreamChannelRef = serde_json::from_value(wire["body"].clone()).unwrap();
    assert_eq!(refs[0].1, expected);
}
