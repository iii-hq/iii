// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

pub mod amplitude;
pub mod collector;

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::engine::Engine;
use crate::modules::module::Module;

use self::amplitude::{AmplitudeClient, AmplitudeEvent};
use self::collector::collector;

// =============================================================================
// Configuration
// =============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
}

fn default_enabled() -> bool {
    true
}

fn default_heartbeat_interval() -> u64 {
    30
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            api_key: String::new(),
            heartbeat_interval_secs: 30,
        }
    }
}

// =============================================================================
// Install ID persistence
// =============================================================================

fn get_or_create_install_id() -> String {
    let base_dir = dirs::home_dir().unwrap_or_else(|| {
        tracing::warn!(
            "Failed to resolve home directory, falling back to temp dir for telemetry_id"
        );
        std::env::temp_dir()
    });
    let path = base_dir.join(".iii").join("telemetry_id");

    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Atomic write: write to temp file then rename to avoid partial reads
    let tmp_path = path.with_extension("tmp");
    if std::fs::write(&tmp_path, &id).is_ok() {
        // Set owner-only permissions on Unix before rename
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp_path, perms).ok();
        }
        std::fs::rename(&tmp_path, &path).ok();
    }

    id
}

// =============================================================================
// Telemetry Module
// =============================================================================

pub struct TelemetryModule {
    engine: Arc<Engine>,
    config: TelemetryConfig,
    client: Arc<AmplitudeClient>,
    install_id: String,
    start_time: Instant,
}

impl TelemetryModule {
    fn build_event(&self, event_type: &str, properties: serde_json::Value) -> AmplitudeEvent {
        AmplitudeEvent {
            device_id: self.install_id.clone(),
            event_type: event_type.to_string(),
            event_properties: properties,
            platform: "III Engine".to_string(),
            os_name: std::env::consts::OS.to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            time: chrono::Utc::now().timestamp_millis(),
            insert_id: Some(uuid::Uuid::new_v4().to_string()),
        }
    }
}

/// A no-op telemetry module used when telemetry is disabled.
struct DisabledTelemetryModule;

#[async_trait]
impl Module for DisabledTelemetryModule {
    fn name(&self) -> &'static str {
        "Telemetry"
    }

    async fn create(
        _engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Module>> {
        Ok(Box::new(DisabledTelemetryModule))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        _shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Module for TelemetryModule {
    fn name(&self) -> &'static str {
        "Telemetry"
    }

    async fn create(engine: Arc<Engine>, config: Option<Value>) -> anyhow::Result<Box<dyn Module>> {
        let mut telemetry_config: TelemetryConfig = match config {
            Some(cfg) => serde_json::from_value(cfg)?,
            None => TelemetryConfig::default(),
        };

        // Check env var override
        if let Ok(env_val) = std::env::var("III_TELEMETRY_ENABLED")
            && (env_val == "false" || env_val == "0")
        {
            telemetry_config.enabled = false;
        }

        if !telemetry_config.enabled {
            tracing::info!("Anonymous telemetry disabled.");
            return Ok(Box::new(DisabledTelemetryModule));
        }

        if telemetry_config.api_key.is_empty() {
            telemetry_config.api_key = "e8fb1f8d290a72dbb2d9b264926be4bf".to_string();
        }

        let install_id = get_or_create_install_id();

        tracing::info!("Anonymous telemetry enabled. Set III_TELEMETRY_ENABLED=false to disable.");

        let client = Arc::new(AmplitudeClient::new(telemetry_config.api_key.clone()));

        Ok(Box::new(TelemetryModule {
            engine,
            config: telemetry_config,
            client,
            install_id,
            start_time: Instant::now(),
        }))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        // Collect active module names from registered functions
        let active_modules: Vec<String> = self
            .engine
            .functions
            .iter()
            .filter_map(|entry| entry.key().split('.').next().map(String::from))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let event = self.build_event(
            "engine_started",
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "active_modules": active_modules,
            }),
        );

        let client_event = event;
        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            if let Err(e) = client.send_event(client_event).await {
                tracing::debug!(error = %e, "Failed to send engine_started telemetry event");
            }
        });

        Ok(())
    }

    async fn start_background_tasks(
        &self,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let interval_secs = self.config.heartbeat_interval_secs;
        let client = Arc::clone(&self.client);
        let install_id = self.install_id.clone();
        let mut shutdown_rx = shutdown;

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));

            // Skip the first immediate tick
            interval.tick().await;

            loop {
                tokio::select! {
                    result = shutdown_rx.changed() => {
                        if result.is_err() {
                            tracing::debug!("[Telemetry] Shutdown channel closed");
                            break;
                        }
                        if *shutdown_rx.borrow() {
                            tracing::debug!("[Telemetry] Heartbeat task shutting down");
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        let telemetry_snapshot = collector().snapshot();

                        let accumulator = crate::modules::observability::metrics::get_metrics_accumulator();
                        let invocations_total = accumulator.invocations_total.load(std::sync::atomic::Ordering::Relaxed);
                        let invocations_success = accumulator.invocations_success.load(std::sync::atomic::Ordering::Relaxed);
                        let invocations_error = accumulator.invocations_error.load(std::sync::atomic::Ordering::Relaxed);
                        let workers_spawns = accumulator.workers_spawns.load(std::sync::atomic::Ordering::Relaxed);
                        let workers_deaths = accumulator.workers_deaths.load(std::sync::atomic::Ordering::Relaxed);

                        let properties = serde_json::json!({
                            "invocations": {
                                "total": invocations_total,
                                "success": invocations_success,
                                "error": invocations_error,
                            },
                            "workers": {
                                "spawns": workers_spawns,
                                "deaths": workers_deaths,
                                "active": workers_spawns.saturating_sub(workers_deaths),
                            },
                            "modules": telemetry_snapshot,
                        });

                        let event = AmplitudeEvent {
                            device_id: install_id.clone(),
                            event_type: "engine_heartbeat".to_string(),
                            event_properties: properties,
                            platform: "III Engine".to_string(),
                            os_name: std::env::consts::OS.to_string(),
                            app_version: env!("CARGO_PKG_VERSION").to_string(),
                            time: chrono::Utc::now().timestamp_millis(),
                            insert_id: Some(uuid::Uuid::new_v4().to_string()),
                        };

                        if let Err(e) = client.send_event(event).await {
                            tracing::debug!(error = %e, "Failed to send heartbeat telemetry event");
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        let uptime_secs = self.start_time.elapsed().as_secs();
        let telemetry_snapshot = collector().snapshot();

        let event = self.build_event(
            "engine_stopped",
            serde_json::json!({
                "uptime_secs": uptime_secs,
                "counters": telemetry_snapshot,
            }),
        );

        // Fire-and-forget with a short timeout to avoid blocking shutdown
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.client.send_event(event),
        )
        .await;

        Ok(())
    }
}

crate::register_module!(
    "modules::telemetry::TelemetryModule",
    TelemetryModule,
    mandatory
);
