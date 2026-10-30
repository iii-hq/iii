use iii_sandbox_contract::{ChannelDirection, StreamChannelRef};
use iii_sdk::channels as sdk;
use serde_json::{from_value, json, to_value};

#[test]
fn contract_stream_channel_ref_matches_iii_sdk() {
    for (ours, theirs, wire) in [
        (ChannelDirection::Read, sdk::ChannelDirection::Read, "read"),
        (
            ChannelDirection::Write,
            sdk::ChannelDirection::Write,
            "write",
        ),
    ] {
        let want = json!({"channel_id": "c", "access_key": "k", "direction": wire});
        let ours = StreamChannelRef {
            channel_id: "c".into(),
            access_key: "k".into(),
            direction: ours,
        };
        let theirs = sdk::StreamChannelRef {
            channel_id: "c".into(),
            access_key: "k".into(),
            direction: theirs,
        };
        assert_eq!(to_value(&ours).unwrap(), want);
        assert_eq!(to_value(&theirs).unwrap(), want);

        let parsed: sdk::StreamChannelRef = from_value(to_value(&ours).unwrap()).unwrap();
        assert_eq!(to_value(&parsed).unwrap(), want);
    }
}
