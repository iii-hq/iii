use iii_sdk::{IIIError, RegisterTriggerInput, III};
use serde_json::json;
use tracing::{debug, info};

/// Most triggers use HTTP GET method. The invoke endpoint uses POST.
pub fn register_triggers(bridge: &III) -> Result<(), IIIError> {
    let triggers = vec![
        ("engine::console::status", "_console/status", "GET"),
        ("engine::console::health", "_console/health", "GET"),
        ("engine::console::functions", "_console/functions", "GET"),
        ("engine::console::triggers", "_console/triggers", "GET"),
        (
            "engine::console::trigger_types",
            "_console/trigger-types",
            "GET",
        ),
        ("engine::console::workers", "_console/workers", "GET"),
        ("engine::console::adapters", "_console/adapters", "GET"),
        ("engine::console::alerts_list", "_console/alerts", "GET"),
        (
            "engine::console::sampling_rules",
            "_console/sampling/rules",
            "GET",
        ),
        (
            "engine::console::otel_logs_list",
            "_console/otel/logs",
            "POST",
        ),
        (
            "engine::console::otel_logs_clear",
            "_console/otel/logs/clear",
            "POST",
        ),
        (
            "engine::console::otel_traces_list",
            "_console/otel/traces",
            "POST",
        ),
        (
            "engine::console::otel_traces_clear",
            "_console/otel/traces/clear",
            "POST",
        ),
        (
            "engine::console::otel_traces_tree",
            "_console/otel/traces/tree",
            "POST",
        ),
        (
            "engine::console::metrics_detailed",
            "_console/metrics/detailed",
            "POST",
        ),
        ("engine::console::rollups_list", "_console/rollups", "POST"),
        // State management endpoints - use state module exclusively
        (
            "engine::console::state_groups_list",
            "_console/states/groups",
            "GET",
        ),
        (
            "engine::console::state_group_items",
            "_console/states/group",
            "POST",
        ),
        (
            "engine::console::state_item_set",
            "_console/states/:group/item",
            "POST",
        ),
        (
            "engine::console::state_item_delete",
            "_console/states/:group/item/:key",
            "DELETE",
        ),
        // Streams discovery (separate from state)
        (
            "engine::console::streams_list",
            "_console/streams/list",
            "GET",
        ),
        // Flow visualization endpoints
        (
            "engine::console::flow_config_get",
            "_console/flows/config/:flow_id",
            "GET",
        ),
        (
            "engine::console::flow_config_save",
            "_console/flows/config/:flow_id",
            "POST",
        ),
        (
            "engine::console::cron_trigger",
            "_console/cron/trigger",
            "POST",
        ),
        // Function invocation endpoint
        ("engine::console::invoke", "_console/invoke", "POST"),
        // Queue management endpoints
        ("engine::console::queues_list", "_console/queues", "GET"),
        (
            "engine::console::queue_detail",
            "_console/queues/:topic",
            "GET",
        ),
        (
            "engine::console::queue_publish",
            "_console/queues/:topic/publish",
            "POST",
        ),
        // DLQ management endpoints
        ("engine::console::dlq_list", "_console/dlq", "GET"),
        (
            "engine::console::dlq_messages",
            "_console/dlq/:topic/messages",
            "POST",
        ),
        (
            "engine::console::dlq_redrive",
            "_console/dlq/:topic/redrive",
            "POST",
        ),
        (
            "engine::console::dlq_redrive_message",
            "_console/dlq/:topic/messages/:id/redrive",
            "POST",
        ),
        (
            "engine::console::dlq_discard_message",
            "_console/dlq/:topic/messages/:id/discard",
            "DELETE",
        ),
    ];

    // Register each trigger with the bridge
    for (function_path, api_path, method) in triggers {
        let config = json!({
            "api_path": api_path,
            "http_method": method
        });

        debug!("Registering API trigger: {} -> {}", api_path, function_path);

        bridge.register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: function_path.to_string(),
            config,
        })?;

        info!(
            "Successfully registered API trigger: {} -> {}",
            api_path, function_path
        );
    }

    Ok(())
}
