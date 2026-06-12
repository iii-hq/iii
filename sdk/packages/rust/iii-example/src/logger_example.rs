use iii_observability::Logger;
use iii_sdk::{Error, IIIClient, RegisterFunction};
use serde_json::{Value, json};

pub fn setup(iii: &IIIClient) {
    iii.register_function(
        "example::logger_demo",
        RegisterFunction::new_async(|input: Value| async move {
            let logger = Logger::new();

            logger.info("Processing request", Some(json!({ "input": input })));

            logger.debug(
                "Validating input fields",
                Some(json!({ "step": "validation" })),
            );

            logger.warn(
                "Using default timeout",
                Some(json!({ "timeout_ms": 5000, "reason": "not configured" })),
            );

            logger.info("Request processed successfully", None);

            Ok::<Value, Error>(json!({ "status": "ok" }))
        })
        .description("Demonstrates Logger with all log levels"),
    );
}
