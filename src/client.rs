use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::{Duration, sleep};

#[derive(Serialize)]
struct MessageClient {
    msg_type: String,
    content: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stream = TcpStream::connect("127.0.0.1:8080").await?;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    // Task to read messages from the server
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            println!("Server msg: {line}");
        }
    });

    for _ in 1..=5 {
        send_random_message(&mut writer).await?;
    }

    let wait_message = "Waiting 30 seconds before sending more messages...\n";
    writer.write_all(wait_message.as_bytes()).await?;
    writer.write_all(b"\n").await?;

    sleep(Duration::from_secs(30)).await;

    for _ in 1..=5 {
        send_random_message(&mut writer).await?;
    }

    Ok(())
}

async fn send_random_message(writer: &mut tokio::net::tcp::OwnedWriteHalf) -> anyhow::Result<()> {
    let mensagens = [
        "Hello server!",
        "How are you?",
        "This is an async message!",
        "Goodbye 👋",
    ];

    for msg in mensagens.iter() {
        writer.write_all(msg.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        sleep(Duration::from_secs(1)).await;
    }
    sleep(Duration::from_secs(5)).await;
    let json_msg = MessageClient {
        msg_type: "greeting".to_string(),
        content: "Hello from client in JSON!".to_string(),
    };
    let jason_msg = serde_json::to_string(&json_msg)?;
    writer.write_all(jason_msg.as_bytes()).await?;
    writer.write_all(b"\n").await?;

    Ok(())
}
