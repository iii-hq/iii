//! Helper free functions that operate on an [`III`] instance.
//!
//! These were previously instance methods on `III`. They take `&III` as the
//! first argument so the public API surface of `III` stays focused on the
//! core lifecycle and registration methods.

pub use crate::channels::{
    ChannelDirection, ChannelItem, extract_channel_refs, is_channel_ref,
};

use std::sync::Arc;

use serde_json::Value;

use crate::error::IIIError;
use crate::iii::{III, RegisterFunction, RegisterTriggerType, TriggerTypeRef};
use crate::stream_provider::IStream;
use crate::triggers::TriggerHandler;
use crate::types::{
    Channel, StreamDeleteInput, StreamGetInput, StreamListGroupsInput, StreamListInput,
    StreamSetInput,
};

/// Create a streaming channel pair for worker-to-worker data transfer.
///
/// Free-function form of `III`'s former `create_channel` instance method.
pub async fn create_channel(
    iii: &III,
    buffer_size: Option<usize>,
) -> Result<Channel, IIIError> {
    crate::iii::internal_create_channel(iii, buffer_size).await
}

/// Register a custom stream provider for a stream name.
///
/// Wires the 5 callable `stream::*` functions (`get`, `set`, `delete`, `list`,
/// `list_groups`) on the engine through the supplied [`IStream`] implementor.
/// `update` is **not** registered — atomic updates remain engine-side.
pub fn create_stream<S>(iii: &III, name: impl Into<String>, stream: S)
where
    S: IStream,
{
    let stream: Arc<S> = Arc::new(stream);
    let name = name.into();

    let s = stream.clone();
    iii.register_function(
        format!("stream::get({name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamGetInput = serde_json::from_value(input)
                    .map_err(|e| IIIError::Serde(e.to_string()))?;
                let out = s.get(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );

    let s = stream.clone();
    iii.register_function(
        format!("stream::set({name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamSetInput = serde_json::from_value(input)
                    .map_err(|e| IIIError::Serde(e.to_string()))?;
                let out = s.set(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );

    let s = stream.clone();
    iii.register_function(
        format!("stream::delete({name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamDeleteInput = serde_json::from_value(input)
                    .map_err(|e| IIIError::Serde(e.to_string()))?;
                let out = s.delete(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );

    let s = stream.clone();
    iii.register_function(
        format!("stream::list({name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamListInput = serde_json::from_value(input)
                    .map_err(|e| IIIError::Serde(e.to_string()))?;
                let out = s.list(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );

    let s = stream.clone();
    iii.register_function(
        format!("stream::list_groups({name})"),
        RegisterFunction::new_async(move |input: Value| {
            let s = s.clone();
            async move {
                let typed: StreamListGroupsInput = serde_json::from_value(input)
                    .map_err(|e| IIIError::Serde(e.to_string()))?;
                let out = s.list_groups(typed).await?;
                Ok(serde_json::to_value(out).unwrap_or_default())
            }
        }),
    );
}

/// Register a custom trigger type with the engine.
///
/// Free-function form of `III`'s former `register_trigger_type` method.
pub fn register_trigger_type<H, C, R>(
    iii: &III,
    registration: RegisterTriggerType<H, C, R>,
) -> TriggerTypeRef<C, R>
where
    H: TriggerHandler + 'static,
{
    crate::iii::internal_register_trigger_type(iii, registration)
}

/// Unregister a previously registered trigger type by id.
pub fn unregister_trigger_type(iii: &III, id: impl Into<String>) {
    crate::iii::internal_unregister_trigger_type(iii, id.into());
}
