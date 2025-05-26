/************************  src/client.rs ******************************/

use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use uuid::Uuid;

pub async fn submit(socket: &str, cmd_opt: Option<String>, gpus: usize) -> anyhow::Result<()> {
    let mut stream = UnixStream::connect(socket).await?;
    if let Some(cmd) = cmd_opt {
        let msg = json!({"type":"submit","cmd":cmd,"gpus":gpus});
        stream.write_all(msg.to_string().as_bytes()).await?;
        stream.write_all(b"\n").await?;
    } else {
        // read stdin lines
        use tokio::io::stdin;
        let stdin = stdin();
        let reader = BufReader::new(stdin);
        tokio::pin!(reader);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            let msg = json!({"type":"submit","cmd":line,"gpus":gpus});
            stream.write_all(msg.to_string().as_bytes()).await?;
            stream.write_all(b"\n").await?;
        }
    }
    Ok(())
}

pub async fn status(socket: &str) -> anyhow::Result<()> {
    let mut stream = UnixStream::connect(socket).await?;
    stream.write_all(b"{\"type\":\"status\"}\n").await?;
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader.read_line(&mut buf).await?;
    println!("{buf}");
    Ok(())
}
