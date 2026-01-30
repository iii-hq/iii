// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::super::structs::CronSchedulerAdapter;
use crate::{
    builtins::kv::BuiltinKvStore,
    engine::Engine,
    modules::cron::registry::{CronAdapterFuture, CronAdapterRegistration},
};

const DEFAULT_LOCK_TTL_MS: u64 = 30_000;
const DEFAULT_LOCK_INDEX: &str = "cron_locks";

pub struct KvCronLock {
    kv: Arc<BuiltinKvStore>,
    instance_id: String,
    lock_ttl_ms: u64,
    lock_index: String,
}

impl KvCronLock {
    pub async fn new(config: Option<Value>) -> anyhow::Result<Self> {
        let lock_ttl_ms = config
            .as_ref()
            .and_then(|cfg| cfg.get("lock_ttl_ms"))
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_LOCK_TTL_MS);
        let lock_index = config
            .as_ref()
            .and_then(|cfg| cfg.get("lock_index"))
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_LOCK_INDEX)
            .to_string();

        tracing::warn!(
            lock_index = %lock_index,
            lock_ttl_ms = %lock_ttl_ms,
            "KvCronAdapter uses process-local locks; cron jobs may run on every engine instance"
        );

        let kv = Arc::new(BuiltinKvStore::new(config));
        let instance_id = uuid::Uuid::new_v4().to_string();

        Ok(Self {
            kv,
            instance_id,
            lock_ttl_ms,
            lock_index,
        })
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> CronAdapterFuture {
    Box::pin(async move {
        Ok(Arc::new(KvCronLock::new(config).await?) as Arc<dyn CronSchedulerAdapter>)
    })
}

crate::register_adapter!(<CronAdapterRegistration> "modules::cron::KvCronAdapter", make_adapter);

#[async_trait]
impl CronSchedulerAdapter for KvCronLock {
    async fn try_acquire_lock(&self, job_id: &str) -> bool {
        self.kv
            .try_acquire_lock(
                &self.lock_index,
                job_id,
                &self.instance_id,
                self.lock_ttl_ms,
            )
            .await
    }

    async fn release_lock(&self, job_id: &str) {
        let _ = self
            .kv
            .release_lock(&self.lock_index, job_id, &self.instance_id)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use super::*;
    use tracing_subscriber::{EnvFilter, fmt::MakeWriter, layer::SubscriberExt};

    #[derive(Clone, Default)]
    struct TestWriter {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl TestWriter {
        fn output(&self) -> String {
            String::from_utf8_lossy(&self.buffer.lock().unwrap()).to_string()
        }
    }

    struct TestWriterGuard {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for TestWriterGuard {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.buffer.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for TestWriter {
        type Writer = TestWriterGuard;

        fn make_writer(&'a self) -> Self::Writer {
            TestWriterGuard {
                buffer: Arc::clone(&self.buffer),
            }
        }
    }

    #[tokio::test]
    async fn test_kv_cron_adapter_locking() {
        let adapter = KvCronLock::new(None).await.unwrap();
        let job_id = "job:kv";

        assert!(adapter.try_acquire_lock(job_id).await);
        assert!(!adapter.try_acquire_lock(job_id).await);
        adapter.release_lock(job_id).await;
        assert!(adapter.try_acquire_lock(job_id).await);
    }

    #[tokio::test]
    async fn test_kv_cron_adapter_warns_about_process_local_locking() {
        let writer = TestWriter::default();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new("warn"))
            .with(tracing_subscriber::fmt::layer().with_writer(writer.clone()));
        let _guard = tracing::subscriber::set_default(subscriber);

        KvCronLock::new(None).await.unwrap();

        let output = writer.output();
        assert!(
            output.contains(
                "KvCronAdapter uses process-local locks; cron jobs may run on every engine instance"
            ),
            "expected warning not emitted: {}",
            output
        );
    }
}
