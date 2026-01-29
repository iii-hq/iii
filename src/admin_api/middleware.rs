use axum::{
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};

pub async fn auth_middleware<B>(request: Request<B>, next: Next<B>) -> Result<Response, StatusCode> {
    let token = std::env::var("III_ADMIN_TOKEN").unwrap_or_default();
    if token.is_empty() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    let expected = format!("Bearer {}", token);
    if auth_header != expected {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}
