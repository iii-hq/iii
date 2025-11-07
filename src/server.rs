use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Mutex, mpsc},
    task::JoinHandle,
};
use uuid::Uuid;

type ClientId = Uuid;

struct Client {
    id: ClientId,
    addr: SocketAddr,
    tx: mpsc::UnboundedSender<String>,
    handle: JoinHandle<()>,
}

#[derive(Default)]
struct ServerState {
    clients: HashMap<ClientId, Client>,
}

impl ServerState {
    fn add_client(
        &mut self,
        addr: SocketAddr,
        writer: tokio::net::tcp::OwnedWriteHalf,
    ) -> ClientId {
        let id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let mut writer = writer;

        // Write task
        let handle = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if writer.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
                if writer.write_all(b"\n").await.is_err() {
                    break;
                }
            }
        });

        let client = Client {
            id,
            addr,
            tx,
            handle,
        };
        self.clients.insert(id, client);
        println!(">> client {id} conected ({} active clients)", self.count());
        id
    }

    fn remove_client(&mut self, id: &ClientId) {
        if let Some(client) = self.clients.remove(id) {
            println!("<< client {id} disconected");
            client.handle.abort();
        }
    }

    fn count(&self) -> usize {
        self.clients.len()
    }

    fn broadcast(&self, msg: &str) {
        for client in self.clients.values() {
            let _ = client.tx.send(msg.to_string());
        }
    }

    fn send_msg(&self, client_id: &ClientId, msg: &str) -> bool {
        if let Some(client) = self.clients.get(client_id) {
            let _ = client.tx.send(msg.to_string());
            true
        } else {
            false
        }
    }

    async fn handle_connection(
        state: Arc<Mutex<Self>>,
        stream: TcpStream,
        addr: SocketAddr,
    ) -> anyhow::Result<()> {
        let (reader, writer) = stream.into_split();

        // Register client and create write task
        let id = {
            let mut guard = state.lock().await;
            guard.add_client(addr, writer)
        };

        // notify other clients
        {
            let guard = state.lock().await;
            guard.broadcast(&format!("* client {id} ({addr}) available *"));
        }

        // Read messages from the client in a loop
        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let message = format!("cliente {id}: {line}");
            println!("{}", message);
        }

        // Client disconnected, clean up
        {
            let mut guard = state.lock().await;
            guard.remove_client(&id);
            guard.broadcast(&format!("* client {id} disconnected *"));
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = Arc::new(Mutex::new(ServerState::default()));
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on 127.0.0.1:8080");

    // example of broadcasting a message every 2 seconds
    {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let guard = state.lock().await;
                guard.broadcast("Broadcast message to all clients every 2 seconds .");
            }
        });
    }

    loop {
        let (socket, addr) = listener.accept().await?;
        let state = state.clone();

        tokio::spawn(async move {
            if let Err(e) = ServerState::handle_connection(state, socket, addr).await {
                eprintln!("Erro com {addr}: {e}");
            }
        });
        // example of broadcasting a message every 30 seconds
    }
}
