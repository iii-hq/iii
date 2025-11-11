use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use std::{collections::HashSet, net::SocketAddr, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_util::codec::{Decoder, LengthDelimitedCodec};
use uuid::Uuid;

mod protocol;
use protocol::*;

#[derive(Clone)]
struct Worker {
    channel: mpsc::Sender<Message>,
}

struct Function {
    worker: Worker,
    _function_path: String,
    _description: Option<String>,
}

struct Invocation {
    _invocation_id: Uuid,
    _function_path: String,
    worker: Worker,
}

#[derive(Default)]
struct Engine {
    functions: Arc<DashMap<String, Function>>,
    invocations: Arc<DashMap<Uuid, Invocation>>,
}

impl Engine {
    fn new() -> Self {
        Self {
            functions: Arc::new(DashMap::new()),
            invocations: Arc::new(DashMap::new()),
        }
    }

    pub async fn listen(self: Arc<Self>, addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        println!("Engine listening on {}", addr);

        loop {
            let (socket, peer) = listener.accept().await?;
            let this = self.clone();
            tokio::spawn(async move {
                if let Err(e) = this.handle_worker(socket, peer).await {
                    eprintln!("worker error: {e:?}");
                }
            });
        }
    }

    async fn send_msg(&self, worker: &Worker, msg: Message) -> bool {
        worker.channel.send(msg).await.is_ok()
    }

    async fn router_msg(&self, worker: &Worker, msg: &Message) -> anyhow::Result<()> {
        match msg {
            Message::InvokeFunction {
                invocation_id,
                function_path,
                data,
            } => {
                let function_option = self.functions.get(function_path);
                match function_option {
                    Some(function) => {
                        function
                            .worker
                            .channel
                            .send(Message::InvokeFunction {
                                invocation_id: invocation_id.clone(),
                                function_path: function_path.clone(),
                                data: data.clone(),
                            })
                            .await?;

                        return Ok(());
                    }
                    None => {
                        self.send_msg(
                            &worker,
                            Message::InvocationResult {
                                invocation_id: invocation_id.clone(),
                                function_path: function_path.clone(),
                                result: None,
                                error: Some(ErrorBody {
                                    code: "function_not_found".into(),
                                    message: "Function not found".to_string(),
                                }),
                            },
                        )
                        .await;

                        return Ok(());
                    }
                }
            }

            Message::InvocationResult {
                invocation_id,
                function_path,
                result,
                error,
            } => {
                let invocation = self.invocations.get(invocation_id);
                match invocation {
                    Some(invocation) => {
                        invocation
                            .worker
                            .channel
                            .send(Message::InvocationResult {
                                invocation_id: invocation_id.clone(),
                                function_path: function_path.clone(),
                                result: result.clone(),
                                error: error.clone(),
                            })
                            .await?;
                    }
                    None => {
                        // TODO let's see if we need to handle invocation result for
                        // callee workers that were removed afterwards
                    }
                }
                return Ok(());
            }

            Message::RegisterFunction {
                function_path,
                description,
            } => {
                let new_function = Function {
                    worker: worker.clone(),
                    _function_path: function_path.clone(),
                    _description: description.clone(),
                };
                self.functions.insert(function_path.clone(), new_function);

                return Ok(());
            }

            _ => {
                return Err(anyhow::anyhow!("Unknown message: {:?}", msg));
            }
        }
    }

    async fn handle_worker(
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

        // Channel used to push outbound messages to this worker
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

        let worker = Worker { channel: tx };

        // Reader loop
        while let Some(frame) = rd.next().await {
            let bytes: BytesMut = frame?;
            let msg: Message = serde_json::from_slice(&bytes)?;
            println!("Received message from worker {}: {:?}", str_peer_addr, msg);
            self.router_msg(&worker, &msg).await?;
        }

        writer.abort();
        println!(">> Worker {} disconnected", str_peer_addr);

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let engine = Arc::new(Engine::new());
    let eng = engine.clone();

    tokio::spawn(async move {
        eng.listen("127.0.0.1:49134").await.unwrap();
    });

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}
