use iii_sdk::{
    ApiRequest, ApiResponse, III, IIIError, Logger, RegisterFunctionMessage, RegisterTriggerInput,
    execute_traced_request,
};
use serde_json::json;

pub fn setup(iii: &III) {
    let client = reqwest::Client::new();

    let get_client = client.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "api::get::http::rust::fetch".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |_input| {
            let client = get_client.clone();
            let logger = Logger::new();

            async move {
                logger.info("Fetching todo from external API", None);

                let request = client
                    .get("https://jsonplaceholder.typicode.com/todos/1")
                    .build()
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let response = execute_traced_request(&client, request)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let status = response.status().as_u16();
                logger.info(
                    "Fetched todo successfully",
                    Some(json!({ "status": status })),
                );

                let data: serde_json::Value = response
                    .json::<serde_json::Value>()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let api_response = ApiResponse {
                    status_code: 200,
                    body: json!({ "upstream_status": status, "data": data }),
                    headers: [("Content-Type".into(), "application/json".into())].into(),
                };

                Ok(serde_json::to_value(api_response)?)
            }
        },
    );

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "http".to_string(),
        function_id: "api::get::http::rust::fetch".to_string(),
        config: json!({
            "api_path": "http-fetch",
            "http_method": "GET",
            "description": "Fetch a todo from JSONPlaceholder (demonstrates OTel fetch instrumentation)",
            "metadata": { "tags": ["http-example"] }
        }),
    })
    .expect("failed to register GET http-fetch trigger");

    let post_client = client.clone();
    iii.register_function(
        RegisterFunctionMessage {
            id: "api::post::http::rust::fetch".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        move |input| {
            let client = post_client.clone();
            async move {
                let logger = Logger::new();
                let req: ApiRequest = serde_json::from_value(input)
                    .unwrap_or_else(|_| serde_json::from_value(json!({})).unwrap());

                logger.info("Posting to httpbin", Some(json!({ "body": req.body })));

                let payload = if req.body.is_null() {
                    json!({ "message": "hello from iii rust" })
                } else {
                    req.body.clone()
                };

                let request = client
                    .post("https://httpbin.org/post")
                    .header("Content-Type", "application/json")
                    .json(&payload)
                    .build()
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let response = execute_traced_request(&client, request)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let status = response.status().as_u16();
                logger.info("Post completed", Some(json!({ "status": status })));

                let data: serde_json::Value = response
                    .json::<serde_json::Value>()
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                let api_response = ApiResponse {
                    status_code: status,
                    body: json!({ "upstream_status": status, "data": data }),
                    headers: [("Content-Type".into(), "application/json".into())].into(),
                };

                Ok(serde_json::to_value(api_response)?)
            }
        },
    );

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "http".to_string(),
        function_id: "api::post::http::rust::fetch".to_string(),
        config: json!({
            "api_path": "http-fetch",
            "http_method": "POST",
            "description": "POST to httpbin (demonstrates request body size in OTel spans)",
            "metadata": { "tags": ["http-example"] }
        }),
    })
    .expect("failed to register POST http-fetch trigger");
}
