// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::engine::Engine;
use crate::modules::observability::metrics::get_metrics_accumulator;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatEntry {
    pub instance_id: String,
    pub timestamp: String,
    pub engine_version: String,
    pub uptime_seconds: u64,
    pub system: SystemInfo,
    pub registration: RegistrationInfo,
    pub runtime: RuntimeInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub hostname: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationInfo {
    pub triggers: TriggerInfo,
    pub workers: Vec<WorkerInfo>,
    pub functions: FunctionInfo,
    pub modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub count: usize,
    pub types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub id: String,
    pub os: String,
    pub runtime: String,
    pub sdk_version: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub count: usize,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub invocations_total: u64,
    pub invocations_success: u64,
    pub invocations_error: u64,
    pub workers_active: usize,
}

pub struct Collector {
    engine: Arc<Engine>,
    instance_id: String,
    start_time: Instant,
    module_classes: Vec<String>,
}

impl Collector {
    pub fn new(engine: Arc<Engine>, instance_id: String, module_classes: Vec<String>) -> Self {
        Self {
            engine,
            instance_id,
            start_time: Instant::now(),
            module_classes,
        }
    }

    pub fn collect(&self) -> HeartbeatEntry {
        let triggers = self.collect_triggers();
        let workers = self.collect_workers();
        let functions = self.collect_functions();
        let runtime = self.collect_runtime(&workers);

        HeartbeatEntry {
            instance_id: self.instance_id.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            system: SystemInfo {
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
                hostname: hostname(),
            },
            registration: RegistrationInfo {
                triggers,
                workers,
                functions,
                modules: self.module_classes.clone(),
            },
            runtime,
        }
    }

    fn collect_triggers(&self) -> TriggerInfo {
        let registry = &self.engine.trigger_registry;
        let count = registry.triggers.len();
        let types: Vec<String> = registry
            .trigger_types
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        TriggerInfo { count, types }
    }

    fn collect_workers(&self) -> Vec<WorkerInfo> {
        self.engine
            .worker_registry
            .workers
            .iter()
            .map(|entry| {
                let w = entry.value();
                WorkerInfo {
                    id: w.id.to_string(),
                    os: w.os.clone().unwrap_or_else(|| "unknown".to_string()),
                    runtime: w.runtime.clone().unwrap_or_else(|| "unknown".to_string()),
                    sdk_version: w.version.clone().unwrap_or_else(|| "unknown".to_string()),
                    status: w.status.as_str().to_string(),
                }
            })
            .collect()
    }

    fn collect_functions(&self) -> FunctionInfo {
        let registry = &self.engine.functions;
        let ids: Vec<String> = registry
            .functions
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        FunctionInfo {
            count: ids.len(),
            ids,
        }
    }

    fn collect_runtime(&self, workers: &[WorkerInfo]) -> RuntimeInfo {
        let acc = get_metrics_accumulator();
        let active = workers
            .iter()
            .filter(|w| w.status == "available" || w.status == "busy")
            .count();
        RuntimeInfo {
            invocations_total: acc.invocations_total.load(Ordering::Relaxed),
            invocations_success: acc.invocations_success.load(Ordering::Relaxed),
            invocations_error: acc.invocations_error.load(Ordering::Relaxed),
            workers_active: active,
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
