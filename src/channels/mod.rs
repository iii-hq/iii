// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

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

        let candidates: Vec<(String, Arc<StreamChannel>)> = self
            .channels
            .iter()
            .filter(|entry| now.duration_since(entry.value().created_at) > CHANNEL_TTL)
            .map(|entry| (entry.key().clone(), Arc::clone(entry.value())))
            .collect();

        let mut stale_ids = Vec::new();
        for (id, ch) in candidates {
            let tx_taken = ch.tx.lock().await.is_none();
            let rx_taken = ch.rx.lock().await.is_none();
            // If both endpoints were taken, the channel is actively in use
            // and will be cleaned up by ws_handler or views.rs when done.
            // Stale = at least one side never connected.
            if !tx_taken || !rx_taken {
                stale_ids.push(id);
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
    pub fn start_sweep_task(self: &Arc<Self>, mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_manager_new() {
        let mgr = ChannelManager::new();
        assert!(mgr.channels.is_empty());
    }

    #[test]
    fn test_channel_manager_default() {
        let mgr = ChannelManager::default();
        assert!(mgr.channels.is_empty());
    }

    #[test]
    fn test_create_channel_returns_writer_and_reader_refs() {
        let mgr = ChannelManager::new();
        let (writer_ref, reader_ref) = mgr.create_channel(16, None);

        // Both refs share the same channel id and access key.
        assert_eq!(writer_ref.channel_id, reader_ref.channel_id);
        assert_eq!(writer_ref.access_key, reader_ref.access_key);

        // Directions are correct.
        assert!(matches!(writer_ref.direction, ChannelDirection::Write));
        assert!(matches!(reader_ref.direction, ChannelDirection::Read));

        // Channel was inserted into the manager.
        assert_eq!(mgr.channels.len(), 1);
    }

    #[test]
    fn test_create_channel_buffer_size_at_least_one() {
        let mgr = ChannelManager::new();
        // buffer_size 0 should be clamped to 1 (max(1)).
        let (writer_ref, _reader_ref) = mgr.create_channel(0, None);
        // Channel should still be created successfully.
        assert!(mgr.channels.contains_key(&writer_ref.channel_id));
    }

    #[test]
    fn test_get_channel_correct_key() {
        let mgr = ChannelManager::new();
        let (writer_ref, _) = mgr.create_channel(8, None);

        let channel = mgr.get_channel(&writer_ref.channel_id, &writer_ref.access_key);
        assert!(channel.is_some());
    }

    #[test]
    fn test_get_channel_wrong_key() {
        let mgr = ChannelManager::new();
        let (writer_ref, _) = mgr.create_channel(8, None);

        let channel = mgr.get_channel(&writer_ref.channel_id, "wrong-key");
        assert!(channel.is_none());
    }

    #[test]
    fn test_get_channel_nonexistent_id() {
        let mgr = ChannelManager::new();
        let channel = mgr.get_channel("nonexistent", "any-key");
        assert!(channel.is_none());
    }

    #[test]
    fn test_remove_channel() {
        let mgr = ChannelManager::new();
        let (writer_ref, _) = mgr.create_channel(8, None);
        assert_eq!(mgr.channels.len(), 1);

        mgr.remove_channel(&writer_ref.channel_id);
        assert!(mgr.channels.is_empty());
    }

    #[test]
    fn test_remove_channel_nonexistent_is_noop() {
        let mgr = ChannelManager::new();
        mgr.remove_channel("nonexistent");
        assert!(mgr.channels.is_empty());
    }

    #[tokio::test]
    async fn test_take_sender() {
        let mgr = ChannelManager::new();
        let (writer_ref, _) = mgr.create_channel(8, None);

        // First take should succeed.
        let sender = mgr
            .take_sender(&writer_ref.channel_id, &writer_ref.access_key)
            .await;
        assert!(sender.is_some());

        // Second take should return None (already taken).
        let sender2 = mgr
            .take_sender(&writer_ref.channel_id, &writer_ref.access_key)
            .await;
        assert!(sender2.is_none());
    }

    #[tokio::test]
    async fn test_take_receiver() {
        let mgr = ChannelManager::new();
        let (_, reader_ref) = mgr.create_channel(8, None);

        // First take should succeed.
        let receiver = mgr
            .take_receiver(&reader_ref.channel_id, &reader_ref.access_key)
            .await;
        assert!(receiver.is_some());

        // Second take should return None (already taken).
        let receiver2 = mgr
            .take_receiver(&reader_ref.channel_id, &reader_ref.access_key)
            .await;
        assert!(receiver2.is_none());
    }

    #[tokio::test]
    async fn test_take_sender_wrong_key() {
        let mgr = ChannelManager::new();
        let (writer_ref, _) = mgr.create_channel(8, None);

        let sender = mgr.take_sender(&writer_ref.channel_id, "bad-key").await;
        assert!(sender.is_none());
    }

    #[tokio::test]
    async fn test_take_receiver_wrong_key() {
        let mgr = ChannelManager::new();
        let (_, reader_ref) = mgr.create_channel(8, None);

        let receiver = mgr.take_receiver(&reader_ref.channel_id, "bad-key").await;
        assert!(receiver.is_none());
    }

    #[test]
    fn test_remove_channels_by_worker() {
        let mgr = ChannelManager::new();
        let worker_a = Uuid::new_v4();
        let worker_b = Uuid::new_v4();

        mgr.create_channel(8, Some(worker_a));
        mgr.create_channel(8, Some(worker_a));
        mgr.create_channel(8, Some(worker_b));
        mgr.create_channel(8, None);

        assert_eq!(mgr.channels.len(), 4);

        mgr.remove_channels_by_worker(&worker_a);

        // Only the two channels owned by worker_a should be removed.
        assert_eq!(mgr.channels.len(), 2);

        // worker_b and unowned channels should remain.
        let remaining_owners: Vec<Option<Uuid>> = mgr
            .channels
            .iter()
            .map(|e| e.value().owner_worker_id)
            .collect();
        assert!(remaining_owners.contains(&Some(worker_b)));
        assert!(remaining_owners.contains(&None));
    }

    #[test]
    fn test_remove_channels_by_worker_no_matches() {
        let mgr = ChannelManager::new();
        mgr.create_channel(8, None);
        mgr.create_channel(8, None);

        let random_worker = Uuid::new_v4();
        mgr.remove_channels_by_worker(&random_worker);

        // Nothing should be removed.
        assert_eq!(mgr.channels.len(), 2);
    }

    #[tokio::test]
    async fn test_sweep_stale_channels_removes_nothing_when_fresh() {
        let mgr = ChannelManager::new();
        mgr.create_channel(8, None);
        mgr.create_channel(8, None);

        // Channels are freshly created, so none should be stale.
        let removed = mgr.sweep_stale_channels().await;
        assert_eq!(removed, 0);
        assert_eq!(mgr.channels.len(), 2);
    }

    #[tokio::test]
    async fn test_sweep_stale_channels_removes_orphaned_old_channels() {
        let mgr = ChannelManager::new();
        let (tx, rx) = mpsc::channel(1);
        let channel = Arc::new(StreamChannel {
            id: "stale-channel".to_string(),
            access_key: "stale-key".to_string(),
            tx: tokio::sync::Mutex::new(Some(tx)),
            rx: tokio::sync::Mutex::new(Some(rx)),
            owner_worker_id: None,
            created_at: Instant::now() - CHANNEL_TTL - Duration::from_secs(1),
        });
        mgr.channels.insert(channel.id.clone(), channel);

        let removed = mgr.sweep_stale_channels().await;
        assert_eq!(removed, 1);
        assert!(mgr.channels.is_empty());
    }

    #[tokio::test]
    async fn test_sweep_stale_channels_keeps_fully_connected_old_channels() {
        let mgr = ChannelManager::new();
        let channel = Arc::new(StreamChannel {
            id: "active-channel".to_string(),
            access_key: "active-key".to_string(),
            tx: tokio::sync::Mutex::new(None),
            rx: tokio::sync::Mutex::new(None),
            owner_worker_id: None,
            created_at: Instant::now() - CHANNEL_TTL - Duration::from_secs(1),
        });
        mgr.channels.insert(channel.id.clone(), channel);

        let removed = mgr.sweep_stale_channels().await;
        assert_eq!(removed, 0);
        assert_eq!(mgr.channels.len(), 1);
    }

    #[tokio::test]
    async fn test_start_sweep_task_stops_on_shutdown() {
        let mgr = Arc::new(ChannelManager::new());
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        mgr.start_sweep_task(shutdown_rx);
        shutdown_tx.send(true).expect("signal shutdown");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    #[tokio::test]
    async fn test_channel_send_receive_text() {
        let mgr = ChannelManager::new();
        let (writer_ref, reader_ref) = mgr.create_channel(8, None);

        let sender = mgr
            .take_sender(&writer_ref.channel_id, &writer_ref.access_key)
            .await
            .unwrap();
        let mut receiver = mgr
            .take_receiver(&reader_ref.channel_id, &reader_ref.access_key)
            .await
            .unwrap();

        sender
            .send(ChannelItem::Text("hello".to_string()))
            .await
            .unwrap();
        drop(sender);

        let item = receiver.recv().await.unwrap();
        match item {
            ChannelItem::Text(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected Text item"),
        }

        // After sender is dropped and all messages consumed, recv returns None.
        let next = receiver.recv().await;
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn test_channel_send_receive_binary() {
        let mgr = ChannelManager::new();
        let (writer_ref, reader_ref) = mgr.create_channel(8, None);

        let sender = mgr
            .take_sender(&writer_ref.channel_id, &writer_ref.access_key)
            .await
            .unwrap();
        let mut receiver = mgr
            .take_receiver(&reader_ref.channel_id, &reader_ref.access_key)
            .await
            .unwrap();

        let data = Bytes::from_static(b"\x00\x01\x02\x03");
        sender
            .send(ChannelItem::Binary(data.clone()))
            .await
            .unwrap();
        drop(sender);

        let item = receiver.recv().await.unwrap();
        match item {
            ChannelItem::Binary(b) => assert_eq!(b, data),
            _ => panic!("Expected Binary item"),
        }
    }

    #[test]
    fn test_create_multiple_channels_unique_ids() {
        let mgr = ChannelManager::new();
        let (w1, _) = mgr.create_channel(8, None);
        let (w2, _) = mgr.create_channel(8, None);
        let (w3, _) = mgr.create_channel(8, None);

        // All channel ids should be unique.
        assert_ne!(w1.channel_id, w2.channel_id);
        assert_ne!(w2.channel_id, w3.channel_id);
        assert_ne!(w1.channel_id, w3.channel_id);
        assert_eq!(mgr.channels.len(), 3);
    }
}
