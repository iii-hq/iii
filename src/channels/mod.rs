use std::sync::Arc;

use axum::body::Bytes;
use dashmap::DashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::protocol::{StreamChannelRef, StreamDirection};

pub mod ws_handler;

#[derive(Debug)]
pub enum ChannelItem {
    Text(String),
    Binary(Bytes),
}

pub struct StreamChannel {
    pub id: String,
    pub access_key: String,
    pub tx: tokio::sync::Mutex<Option<mpsc::Sender<ChannelItem>>>,
    pub rx: tokio::sync::Mutex<Option<mpsc::Receiver<ChannelItem>>>,
}

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

    /// Creates a new streaming channel with the given buffer size.
    /// Returns (writer_ref, reader_ref) -- directions are from the Worker's perspective.
    pub fn create_channel(&self, buffer_size: usize) -> (StreamChannelRef, StreamChannelRef) {
        let channel_id = Uuid::new_v4().to_string();
        let access_key = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel(buffer_size);

        let channel = Arc::new(StreamChannel {
            id: channel_id.clone(),
            access_key: access_key.clone(),
            tx: tokio::sync::Mutex::new(Some(tx)),
            rx: tokio::sync::Mutex::new(Some(rx)),
        });

        self.channels.insert(channel_id.clone(), channel);

        let writer_ref = StreamChannelRef {
            channel_id: channel_id.clone(),
            access_key: access_key.clone(),
            direction: StreamDirection::Write,
        };
        let reader_ref = StreamChannelRef {
            channel_id,
            access_key,
            direction: StreamDirection::Read,
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
}
