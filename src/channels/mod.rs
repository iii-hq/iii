use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use dashmap::DashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::protocol::{ChannelDirection, StreamChannelRef};

pub mod ws_handler;

#[derive(Debug)]
pub enum ChannelItem {
    Text(String),
    Binary(Bytes),
}

#[allow(dead_code)]
pub struct StreamChannel {
    pub(crate) id: String,
    pub(crate) access_key: String,
    pub(crate) tx: tokio::sync::Mutex<Option<mpsc::Sender<ChannelItem>>>,
    pub(crate) rx: tokio::sync::Mutex<Option<mpsc::Receiver<ChannelItem>>>,
    pub(crate) owner_worker_id: Option<Uuid>,
    pub(crate) created_at: Instant,
}

const CHANNEL_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub struct ChannelManager {
    channels: DashMap<String, Arc<StreamChannel>>,
}

impl ChannelManager {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
        }
    }

    /// Creates a new streaming channel with the given buffer size and optional owner.
    /// Returns (writer_ref, reader_ref) -- directions are from the Worker's perspective.
    pub fn create_channel(
        &self,
        buffer_size: usize,
        owner_worker_id: Option<Uuid>,
    ) -> (StreamChannelRef, StreamChannelRef) {
        let buffer_size = buffer_size.max(1);
        let channel_id = Uuid::new_v4().to_string();
        let access_key = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel(buffer_size);

        let channel = Arc::new(StreamChannel {
            id: channel_id.clone(),
            access_key: access_key.clone(),
            tx: tokio::sync::Mutex::new(Some(tx)),
            rx: tokio::sync::Mutex::new(Some(rx)),
            owner_worker_id,
            created_at: Instant::now(),
        });

        self.channels.insert(channel_id.clone(), channel);

        let writer_ref = StreamChannelRef {
            channel_id: channel_id.clone(),
            access_key: access_key.clone(),
            direction: ChannelDirection::Write,
        };
        let reader_ref = StreamChannelRef {
            channel_id,
            access_key,
            direction: ChannelDirection::Read,
        };

        (writer_ref, reader_ref)
    }

    pub fn get_channel(&self, id: &str, key: &str) -> Option<Arc<StreamChannel>> {
        self.channels.get(id).and_then(|ch| {
            if ch.access_key == key {
                Some(ch.value().clone())
            } else {
                None
            }
        })
    }

    pub async fn take_sender(&self, id: &str, key: &str) -> Option<mpsc::Sender<ChannelItem>> {
        let channel = self.get_channel(id, key)?;
        channel.tx.lock().await.take()
    }

    pub async fn take_receiver(&self, id: &str, key: &str) -> Option<mpsc::Receiver<ChannelItem>> {
        let channel = self.get_channel(id, key)?;
        channel.rx.lock().await.take()
    }

    pub fn remove_channel(&self, id: &str) {
        self.channels.remove(id);
    }

    /// Removes all channels owned by the given worker.
    pub fn remove_channels_by_worker(&self, worker_id: &Uuid) {
        let channel_ids: Vec<String> = self
            .channels
            .iter()
            .filter(|entry| entry.value().owner_worker_id.as_ref() == Some(worker_id))
            .map(|entry| entry.key().clone())
            .collect();

        if !channel_ids.is_empty() {
            tracing::info!(
                worker_id = %worker_id,
                count = channel_ids.len(),
                "Cleaning up channels owned by disconnected worker"
            );
        }

        for id in channel_ids {
            self.channels.remove(&id);
        }
    }

    /// Removes channels that have been alive longer than the TTL and have
    /// not been fully connected (at least one of tx/rx is still untaken).
    pub async fn sweep_stale_channels(&self) -> usize {
        let now = Instant::now();
        let mut stale_ids = Vec::new();

        for entry in self.channels.iter() {
            let ch = entry.value();
            if now.duration_since(ch.created_at) > CHANNEL_TTL {
                let tx_taken = ch.tx.lock().await.is_none();
                let rx_taken = ch.rx.lock().await.is_none();
                // If both endpoints were taken, the channel is actively in use
                // and will be cleaned up by ws_handler or views.rs when done.
                // Stale = at least one side never connected.
                if !tx_taken || !rx_taken {
                    stale_ids.push(entry.key().clone());
                }
            }
        }

        let count = stale_ids.len();
        if count > 0 {
            tracing::info!(count, "Sweeping stale orphaned channels");
            for id in stale_ids {
                self.channels.remove(&id);
            }
        }
        count
    }

    /// Spawns a background task that periodically sweeps stale channels.
    pub fn start_sweep_task(
        self: &Arc<Self>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) {
        let mgr = Arc::clone(self);
        tokio::spawn(async move {
            let interval = Duration::from_secs(60);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {
                        mgr.sweep_stale_channels().await;
                    }
                    result = shutdown_rx.changed() => {
                        if result.is_err() || *shutdown_rx.borrow() {
                            tracing::debug!("Channel sweep task shutting down");
                            break;
                        }
                    }
                }
            }
        });
    }
}
