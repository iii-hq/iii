use anyhow::Result;
use axum::{
    body::Body,
    extract::Path,
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{any, get},
    Json, Router,
};
use rust_embed::Embed;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::proxy::http::{ProxyConfig, ProxyState};
use crate::proxy::{http::http_proxy_handler, ws::ws_proxy_handler};

#[derive(Embed)]
#[folder = "assets/"]
struct Assets;

/// Server configuration
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub engine_host: String,
    pub engine_port: u16,
    pub ws_port: u16,
    pub enable_flow: bool,
}

pub struct AppState {
    pub config: ServerConfig,
    #[allow(dead_code)]
    pub proxy: Arc<ProxyState>,
}

/// Generate index.html with runtime config injected
fn get_index_html(config: &ServerConfig) -> String {
    let runtime_config = json!({
        "basePath": "/",
        "engineHost": config.engine_host,
        "enginePort": config.engine_port,
        "wsPort": config.ws_port,
        "enableFlow": config.enable_flow,
    });

    // Get the base index.html from embedded assets
    let index_content = Assets::get("index.html")
        .map(|file| String::from_utf8_lossy(&file.data).to_string())
        .unwrap_or_else(|| {
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>iii Console</title>
</head>
<body>
    <div id="root"></div>
    <script>console.error('Assets not found. Build frontend first.');</script>
</body>
</html>"#
                .to_string()
        });

    // Inject the runtime config script before the closing </head> tag
    let config_json = serde_json::to_string(&runtime_config).unwrap_or_else(|_| "{}".to_string());
    let config_script = format!(
        r#"<script>window.__CONSOLE_CONFIG__={};</script>"#,
        config_json
    );

    index_content.replace("</head>", &format!("{}</head>", config_script))
}

/// Serve the /api/config endpoint with runtime configuration
async fn serve_config(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
) -> Json<serde_json::Value> {
    let config = &state.config;
    Json(json!({
        "engineHost": config.engine_host,
        "enginePort": config.engine_port,
        "wsPort": config.ws_port,
        "consolePort": config.port,
        "version": env!("CARGO_PKG_VERSION"),
        "enableFlow": config.enable_flow,
    }))
}

/// Serve the index.html with runtime config
async fn serve_index(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
) -> Html<String> {
    Html(get_index_html(&state.config))
}

/// Serve static files or fallback to index.html for SPA routing
async fn serve_static_or_index(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
    Path(path): Path<String>,
) -> Response {
    // Try to serve the static file first
    if let Some(file) = Assets::get(&path) {
        let mime = mime_guess::from_path(&path).first_or_octet_stream();
        let body = Body::from(file.data.to_vec());

        Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime.as_ref()).unwrap(),
            )
            .body(body)
            .unwrap()
    } else {
        // Fallback to index.html for SPA routing
        Html(get_index_html(&state.config)).into_response()
    }
}

/// Run the console server
pub async fn run_server(config: ServerConfig) -> Result<()> {
    // Resolve hostname to IP address
    let host = if config.host == "localhost" {
        "127.0.0.1".to_string()
    } else if config.host == "0.0.0.0" || config.host.parse::<std::net::IpAddr>().is_ok() {
        config.host.clone()
    } else {
        // Try to resolve hostname
        use std::net::ToSocketAddrs;
        format!("{}:0", config.host)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .map(|addr| addr.ip().to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string())
    };

    let addr: SocketAddr = format!("{}:{}", host, config.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid address: {}", e))?;

    let proxy_config = ProxyConfig {
        engine_host: config.engine_host.clone(),
        engine_port: config.engine_port,
        ws_port: config.ws_port,
    };
    let proxy_state = Arc::new(ProxyState {
        config: proxy_config,
        // .no_proxy() prevents the HTTP client from using system proxy settings,
        // avoiding proxy loops since this is a local reverse proxy.
        client: reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client"),
    });

    let app_state = Arc::new(AppState {
        config,
        proxy: proxy_state.clone(),
    });

    // Build CORS layer - restrict to console origins
    let mut origins: Vec<HeaderValue> = vec![
        format!("http://127.0.0.1:{}", app_state.config.port)
            .parse()
            .unwrap(),
        format!("http://localhost:{}", app_state.config.port)
            .parse()
            .unwrap(),
        format!("https://127.0.0.1:{}", app_state.config.port)
            .parse()
            .unwrap(),
        format!("https://localhost:{}", app_state.config.port)
            .parse()
            .unwrap(),
    ];

    // Add configured host origins if different from defaults
    let cors_host = if app_state.config.host == "0.0.0.0" {
        "localhost" // 0.0.0.0 is not a valid Origin host; browsers use the resolved name
    } else {
        &app_state.config.host
    };
    if cors_host != "localhost" && cors_host != "127.0.0.1" {
        if let Ok(v) =
            format!("http://{}:{}", cors_host, app_state.config.port).parse::<HeaderValue>()
        {
            origins.push(v);
        }
        if let Ok(v) =
            format!("https://{}:{}", cors_host, app_state.config.port).parse::<HeaderValue>()
        {
            origins.push(v);
        }
    }
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::ACCEPT, header::AUTHORIZATION]);

    let mut app = Router::new()
        .route("/", get(serve_index))
        .route("/api/config", get(serve_config));

    // IMPORTANT: Call .with_state(proxy) BEFORE merging. This converts
    // Router<Arc<ProxyState>> to Router<()>, which can be merged into
    // any Router<S>. The proxy routes carry their own state; the main
    // app routes use AppState. This ordering is required by Axum.
    let proxy_router = Router::new()
        .route("/ws/streams", any(ws_proxy_handler))
        .route("/api/engine/{*path}", any(http_proxy_handler))
        .with_state(proxy_state);

    app = app.merge(proxy_router);

    let app = app
        .route("/{*path}", get(serve_static_or_index))
        .layer(cors)
        .with_state(app_state);

    info!("Console available at http://{}", addr);

    // Create the listener
    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Start the server
    axum::serve(listener, app).await?;

    Ok(())
}
