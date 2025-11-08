use serde_json::Value;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Mutex, mpsc},
    task::JoinHandle,
};
use uuid::Uuid;

type ClientId = Uuid;
mod protocol {
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "lowercase")]
    pub enum Message {
        Register {
            from: Uuid,
            methods: Vec<MethodDef>,
        },
        Call {
            id: Uuid,
            from: Uuid,
            to: Option<Uuid>,
            method: String,
            params: Value,
        },
        Result {
            id: Uuid,
            from: Uuid,
            ok: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            result: Option<Value>,
            #[serde(skip_serializing_if = "Option::is_none")]
            error: Option<ErrorBody>,
        },
        Error {
            id: Uuid,
            from: Uuid,
            code: String,
            message: String,
        },
        Notify {
            from: Uuid,
            to: Option<Uuid>,
            method: String,
            params: Value,
        },
        Ping {
            from: Uuid,
        },
        Pong {
            from: Uuid,
        },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MethodDef {
        pub name: String,
        pub params_schema: serde_json::Value,
        pub result_schema: serde_json::Value,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ErrorBody {
        pub code: String,
        pub message: String,
    }
}

use protocol::*;
struct Client {
    id: ClientId,
    addr: SocketAddr,
    tx: mpsc::UnboundedSender<String>,
    handle: JoinHandle<()>,
}

#[derive(Default)]
struct Engine {
    clients: HashMap<ClientId, Client>,
}

impl Engine {
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
            // Try to parse as JSON
            if let Ok(json_msg) = serde_json::from_str::<Value>(line) {
                let msg_type = json_msg.get("msg_type");
                if let Some(msg_type) = msg_type {
                    println!("cliente {id} (json): tipo de mensagem = {}", msg_type);
                } else {
                    println!("cliente {id} (json): mensagem sem tipo");
                }
            } else {
                let message = format!("cliente {id}: {line}");
                println!("{}", message);
            }
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
    let state = Arc::new(Mutex::new(Engine::default()));
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
            if let Err(e) = Engine::handle_connection(state, socket, addr).await {
                eprintln!("Erro com {addr}: {e}");
            }
        });
        // example of broadcasting a message every 30 seconds
    }
}
