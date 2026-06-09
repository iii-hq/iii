//! Helper free functions that operate on an [`IIIClient`] instance.
//!
//! These were previously instance methods on `IIIClient`. They take `&IIIClient` as the
//! first argument so the public API surface of `IIIClient` stays focused on the
//! core lifecycle and registration methods.

pub use crate::channels::{ChannelDirection, ChannelItem, extract_channel_refs, is_channel_ref};

use std::sync::Arc;

use serde_json::Value;

use crate::error::Error;
use crate::iii::{IIIClient, RegisterFunction};
use crate::stream_provider::IStream;
use crate::types::{
    Channel, StreamDeleteInput, StreamGetInput, StreamListGroupsInput, StreamListInput,
    StreamSetInput,
};

/// Create a streaming channel pair for worker-to-worker data transfer.
///
/// Free-function form of `IIIClient`'s former `create_channel` instance method.
pub async fn create_channel(iii: &IIIClient, buffer_size: Option<usize>) -> Result<Channel, Error> {
    crate::iii::internal_create_channel(iii, buffer_size).await
}

/// Register a custom stream provider for a stream name.
///
/// Wires the 5 callable `stream::*` functions (`get`, `set`, `delete`, `list`,
/// `list_groups`) on the engine through the supplied [`IStream`] implementor.
/// `update` is **not** registered — atomic updates remain engine-side.
pub fn create_stream<S>(iii: &IIIClient, stream_name: impl Into<String>, stream: S)
where
    S: IStream,
{
    let stream: Arc<S> = Arc::new(stream);
    let stream_name = stream_name.into();

    let s = stream.clone();
    iii.register_function(
        format!("stream::get({stream_name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamGetInput =
                    serde_json::from_value(input).map_err(|e| Error::Serde(e.to_string()))?;
                let out = s.get(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );

    let s = stream.clone();
    iii.register_function(
        format!("stream::set({stream_name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamSetInput =
                    serde_json::from_value(input).map_err(|e| Error::Serde(e.to_string()))?;
                let out = s.set(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );

    let s = stream.clone();
    iii.register_function(
        format!("stream::delete({stream_name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamDeleteInput =
                    serde_json::from_value(input).map_err(|e| Error::Serde(e.to_string()))?;
                let out = s.delete(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );

    let s = stream.clone();
    iii.register_function(
        format!("stream::list({stream_name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamListInput =
                    serde_json::from_value(input).map_err(|e| Error::Serde(e.to_string()))?;
                let out = s.list(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );

    let s = stream.clone();
    iii.register_function(
        format!("stream::list_groups({stream_name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamListGroupsInput =
                    serde_json::from_value(input).map_err(|e| Error::Serde(e.to_string()))?;
                let out = s.list_groups(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );
}
