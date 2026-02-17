// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[allow(deprecated)]
pub async fn auth_middleware(request: Request<Body>, next: Next) -> Result<Response, Response> {
    let token = std::env::var("III_ADMIN_TOKEN").unwrap_or_default();
    if token.is_empty() {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "token_not_configured",
            "III_ADMIN_TOKEN environment variable is not set",
        ));
    }

    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    let expected = format!("Bearer {}", token);
    if ring::constant_time::verify_slices_are_equal(expected.as_bytes(), auth_header.as_bytes())
        .is_err()
    {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "Invalid or missing authorization token",
        ));
    }

    Ok(next.run(request).await)
}

pub fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = json!({
        "error": {
            "code": code,
            "message": message,
        }
    });

    (status, axum::Json(body)).into_response()
}
