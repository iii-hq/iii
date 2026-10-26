#![allow(dead_code)]

//! A TCP proxy between a worker and the engine that counts the bytes the
//! worker sends and can drop the connection, for wire-level assertions
//! against a real engine.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::broadcast,
};

pub struct CountingProxy {
    url: String,
    sent: Arc<AtomicUsize>,
    drop_all: broadcast::Sender<()>,
}

impl CountingProxy {
    /// Listens on a random local port and forwards to `engine_ws_url`.
    pub async fn start(engine_ws_url: &str) -> Self {
        let upstream = engine_ws_url
            .trim_start_matches("ws://")
            .trim_end_matches('/')
            .to_string();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let sent = Arc::new(AtomicUsize::new(0));
        let (drop_all, _) = broadcast::channel(4);
        let counter = sent.clone();
        let dropper = drop_all.clone();
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let Ok(server) = TcpStream::connect(&upstream).await else {
                    continue;
                };
                let counter = counter.clone();
                let mut dropped = dropper.subscribe();
                tokio::spawn(async move {
                    let (mut client_rx, mut client_tx) = client.into_split();
                    let (mut server_rx, mut server_tx) = server.into_split();
                    let up = async {
                        let mut buf = vec![0u8; 64 * 1024];
                        loop {
                            let n = match client_rx.read(&mut buf).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => n,
                            };
                            counter.fetch_add(n, Ordering::SeqCst);
                            if server_tx.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                    };
                    let down = async {
                        let mut buf = vec![0u8; 64 * 1024];
                        loop {
                            let n = match server_rx.read(&mut buf).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => n,
                            };
                            if client_tx.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                    };
                    tokio::select! {
                        _ = up => {}
                        _ = down => {}
                        _ = dropped.recv() => {}
                    }
                });
            }
        });
        Self {
            url,
            sent,
            drop_all,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Bytes the worker sent so far.
    pub fn sent_bytes(&self) -> usize {
        self.sent.load(Ordering::SeqCst)
    }

    /// Cuts every proxied connection; the worker reconnects through the proxy.
    pub fn drop_connections(&self) {
        let _ = self.drop_all.send(());
    }
}
