/**
 * Pattern: Channels (Rust)
 * Comparable to: Unix pipes, gRPC streaming, WebSocket data streams
 *
 * Demonstrates binary streaming between workers: creating channels,
 * passing refs across functions, writing/reading binary data, and
 * using text messages for signaling.
 *
 * How-to references:
 *   - Channels: https://iii.dev/docs/how-to/use-channels
 */

use std::time::Duration;

use iii_sdk::{
    register_worker, InitOptions, RegisterFunction, TriggerRequest,
    ChannelReader, extract_channel_refs,
    builtin_triggers::*,
    IIITrigger,
};
use serde_json::json;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ProduceInput {
    records: Vec<serde_json::Value>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("III_ENGINE_URL").unwrap_or("ws://127.0.0.1:49134".into());
    let iii = register_worker(&url, InitOptions::default());

    // -----------------------------------------------------------------------
    // 1. Producer — creates a channel and streams binary data
    // -----------------------------------------------------------------------
    let iii_producer = iii.clone();
    iii.register_function(RegisterFunction::new_async(
        "pipeline::produce",
        move |input: ProduceInput| {
            let iii = iii_producer.clone();
            async move {
                // Create a channel pair
                let channel = iii.create_channel(None).await.map_err(|e| e.to_string())?;

                // Pass the reader ref to the consumer via trigger without waiting
                iii.trigger(TriggerRequest {
                    function_id: "pipeline::consume".into(),
                    payload: json!({
                        "reader_ref": channel.reader_ref,
                        "record_count": input.records.len(),
                    }),
                    action: Some(TriggerAction::Void),
                    timeout_ms: None,
                })
                .await
                .map_err(|e| e.to_string())?;

                // Send metadata as a text message
                channel
                    .writer
                    .send_message(&serde_json::to_string(&json!({
                        "type": "metadata",
                        "format": "ndjson",
                        "encoding": "utf-8",
                    })).unwrap())
                    .await
                    .map_err(|e| e.to_string())?;

                // Stream records as binary data (newline-delimited JSON)
                for record in &input.records {
                    let mut line = serde_json::to_string(record).unwrap();
                    line.push('\n');
                    channel
                        .writer
                        .write(line.as_bytes())
                        .await
                        .map_err(|e| e.to_string())?;
                }

                // Signal end of stream
                channel.writer.close().await.map_err(|e| e.to_string())?;

                Ok(json!({ "status": "streaming", "records": input.records.len() }))
            }
        },
    ));

    // -----------------------------------------------------------------------
    // 2. Consumer — receives a channel ref and reads the stream
    // -----------------------------------------------------------------------
    let iii_consumer = iii.clone();
    iii.register_function(RegisterFunction::new_async(
        "pipeline::consume",
        move |input: serde_json::Value| {
            let iii = iii_consumer.clone();
            async move {
                // Extract channel refs from the payload
                let refs = extract_channel_refs(&input);
                let reader_ref = refs
                    .iter()
                    .find(|(k, _)| k == "reader_ref")
                    .map(|(_, r)| r.clone())
                    .ok_or("missing reader_ref")?;

                // Create reader from the ref
                let reader = ChannelReader::new(iii.address(), &reader_ref);

                // Listen for text messages
                reader
                    .on_message(|msg| {
                        println!("Metadata: {}", msg);
                    })
                    .await;

                // Read entire binary stream
                let raw = reader.read_all().await.map_err(|e| e.to_string())?;
                let text = String::from_utf8(raw).map_err(|e| e.to_string())?;
                let records: Vec<serde_json::Value> = text
                    .trim()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();

                Ok(json!({ "processed": records.len() }))
            }
        },
    ));

    // -----------------------------------------------------------------------
    // 3. HTTP trigger to kick off the pipeline
    // -----------------------------------------------------------------------
    iii.register_trigger(
        IIITrigger::Http(HttpTriggerConfig::new("/pipeline/start").method(HttpMethod::Post))
            .for_function("pipeline::produce"),
    )
    .expect("failed to register http trigger");

    // Keep the process alive for event processing
    tokio::time::sleep(Duration::from_secs(u64::MAX)).await;
    iii.shutdown();
    Ok(())
}
