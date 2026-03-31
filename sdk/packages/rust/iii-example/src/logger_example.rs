use iii_sdk::{III, IIIError, Logger, RegisterFunction};
use serde_json::{Value, json};

pub fn setup(iii: &III) {
    iii.register_function(
        RegisterFunction::new_async(
            "example::logger_demo",
            |input: Value| async move {
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

                Ok::<Value, IIIError>(json!({ "status": "ok" }))
            },
        )
        .description("Demonstrates Logger with all log levels"),
    );
}
