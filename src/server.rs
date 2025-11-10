use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use jsonschema::JSONSchema;
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};
use tokio_util::codec::{Decoder, LengthDelimitedCodec};
use uuid::Uuid;

mod protocol;
use protocol::*;

#[derive(Clone, PartialEq)]
enum ClientStatus {
    Pending,
    Registered,
}

#[derive(Clone)]
struct CompiledMethod {
    name: String,
    params_schema: Arc<JSONSchema>,
    result_schema: Arc<JSONSchema>,
}

#[derive(Clone)]
struct ClientHandle {
    name: String,
    description: String,
    tx: mpsc::Sender<Message>,
    methods: Arc<Vec<CompiledMethod>>,
    status: ClientStatus,
}

#[derive(Default)]
struct Engine {
    id: Uuid,
    clients: Arc<DashMap<ClientAddr, ClientHandle>>,
    routing: Arc<DashMap<String, Uuid>>,
    pending: Arc<DashMap<ClientAddr, oneshot::Sender<Message>>>,
}

impl Engine {
    fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            clients: Arc::new(DashMap::new()),
            routing: Arc::new(DashMap::new()),
            pending: Arc::new(DashMap::new()),
        }
    }

    fn add_client(
        &self,
        client_address: ClientAddr,
        tx: tokio::sync::mpsc::Sender<Message>,
        methods: Vec<CompiledMethod>,
        name: String,
        description: String,
        status: ClientStatus,
    ) -> ClientAddr {
        let handle = ClientHandle {
            tx,
            methods: Arc::new(methods),
            status,
            name,
            description,
        };
        println!(">> new client connected: {}", client_address);
        self.clients.insert(client_address.clone(), handle);
        client_address
    }

    pub async fn listen(self: Arc<Self>, addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        println!("Engine {} listening on {}", self.id, addr);

        loop {
            let (socket, peer) = listener.accept().await?;
            let this = self.clone();
            tokio::spawn(async move {
                if let Err(e) = this.handle_client(socket, peer).await {
                    eprintln!("client error: {e:?}");
                }
            });
        }
    }

    async fn send_msg(&self, client_address: &ClientAddr, msg: Message) -> bool {
        if let Some(client) = self.clients.get(client_address) {
            client.tx.send(msg).await.is_ok()
        } else {
            false
        }
    }

    async fn router_msg(&self, str_peer_addr: &ClientAddr, msg: &Message) -> anyhow::Result<()> {
        match msg {
            Message::InvokeFunctionMessage { to: Some(to), .. }
            | Message::Notify { to: Some(to), .. } => {
                if !self.send_msg(to, msg.clone()).await {
                    eprintln!("no such client to route message to: {to}");
                }
            }
            Message::Register {
                name: worker_name,
                description: worker_description,
                methods: method_defs,
            } => {
                let mut client = self
                    .clients
                    .get_mut(str_peer_addr)
                    .expect("No client found for REGISTER message");

                // Compile method schemas
                let mut compiled_methods = Vec::with_capacity(method_defs.len());
                for md in method_defs {
                    let params_schema = match JSONSchema::compile(&md.params_schema) {
                        Ok(schema) => Arc::new(schema),
                        Err(err) => {
                            let _ = self
                                .send_msg(
                                    str_peer_addr,
                                    Message::Error {
                                        id: Uuid::new_v4(),
                                        code: "invalid_params_schema".into(),
                                        message: format!(
                                            "method {} params schema invalid: {err}",
                                            md.name
                                        ),
                                    },
                                )
                                .await;
                            return Ok(());
                        }
                    };

                    let result_schema = match JSONSchema::compile(&md.result_schema) {
                        Ok(schema) => Arc::new(schema),
                        Err(err) => {
                            let _ = self
                                .send_msg(
                                    str_peer_addr,
                                    Message::Error {
                                        id: Uuid::new_v4(),
                                        code: "invalid_result_schema".into(),
                                        message: format!(
                                            "method {} result schema invalid: {err}",
                                            md.name
                                        ),
                                    },
                                )
                                .await;
                            return Ok(());
                        }
                    };

                    compiled_methods.push(CompiledMethod {
                        name: md.name.clone(),
                        params_schema,
                        result_schema,
                    });
                }

                // Update client handle with methods and status
                // (In a real implementation, we would identify the client properly)
                client.methods = Arc::new(compiled_methods.clone());
                client.status = ClientStatus::Registered;
                client.name = worker_name.clone();
                client.description = worker_description.clone();

                // (In a real implementation, we would identify the client properly)
                // Here we just print the registered methods for demonstration
                println!(
                    "Registered methods: {:?}",
                    compiled_methods
                        .iter()
                        .map(|m| &m.name)
                        .collect::<Vec<&String>>()
                );
            }
            _ => {
                eprintln!("cannot route message without 'to' field");
            }
        }
        Ok(())
    }

    fn register_new_client(
        &self,
        client_address: ClientAddr,
        tx: tokio::sync::mpsc::Sender<Message>,
    ) -> anyhow::Result<()> {
        // Start with no method metadata; a REGISTER message will update this later
        let methods: Vec<CompiledMethod> = Vec::new();
        let name = String::from("unnamed");
        let description = String::from("no description");
        self.add_client(
            client_address,
            tx,
            methods,
            name,
            description,
            ClientStatus::Pending,
        );
        Ok(())
    }

    fn remove_client_by_addr(&self, client_address: &ClientAddr) {
        println!(">> removing client: {}", client_address);
        if let Some(client) = self.clients.get(client_address) {
            let client_id = client_address.clone();
            for method in client.methods.iter() {
                let route_key = format!("{}:{}", method.name, client_id);
                println!(">> removing route: {}", route_key);
                self.routing.remove(&route_key);
            }
        }
        self.clients.remove(client_address);
    }

    async fn handle_client(
        self: Arc<Self>,
        socket: TcpStream,
        peer: SocketAddr,
    ) -> anyhow::Result<()> {
        let str_peer_addr = peer.to_string();
        let codec = LengthDelimitedCodec::builder()
            .max_frame_length(64 * 1024 * 1024) // 64 MiB, just for testing
            .new_codec();
        let framed = codec.framed(socket);
        let (mut wr, mut rd) = framed.split::<Bytes>();

        // Channel used to push outbound messages to this client
        let (tx, mut rx) = mpsc::channel::<Message>(64);

        // Writer task
        let writer = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let bytes = serde_json::to_vec(&msg).unwrap();
                if let Err(e) = wr.send(Bytes::from(bytes)).await {
                    eprintln!("send error: {e:?}");
                    break;
                }
            }
        });

        self.register_new_client(str_peer_addr.clone(), tx.clone())?;
        // Reader loop
        while let Some(frame) = rd.next().await {
            let bytes: BytesMut = frame?;
            let msg: Message = serde_json::from_slice(&bytes)?;
            println!("Received message from client {}: {:?}", str_peer_addr, msg);
            self.router_msg(&str_peer_addr, &msg).await?;
        }

        writer.abort();
        println!(">> Client {} disconnected", str_peer_addr);
        self.remove_client_by_addr(&str_peer_addr);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let engine = Arc::new(Engine::new());
    let eng = engine.clone();

    tokio::spawn(async move {
        eng.listen("127.0.0.1:8080").await.unwrap();
    });

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}
