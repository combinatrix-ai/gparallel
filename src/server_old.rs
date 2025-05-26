/************************  src/server.rs ******************************/

use crate::scheduler::{JobSpec, Scheduler};
use anyhow::Context;
use std::path::Path;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use uuid::Uuid;

pub async fn run(socket_path: &str, per_task: usize) -> anyhow::Result<()> {
    if Path::new(socket_path).exists() {
        std::fs::remove_file(socket_path).ok();
    }
    let listener = UnixListener::bind(socket_path)?;

    let visible = std::env::var("CUDA_VISIBLE_DEVICES")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.parse::<usize>().ok())
        .collect::<Vec<_>>();

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut sched = Scheduler::new(visible, per_task);

    // accept loop
    let accept_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let tx = accept_tx.clone();
                    tokio::spawn(handle_client(stream, tx));
                }
                Err(e) => eprintln!("listener error: {e}"),
            }
        }
    });

    // main scheduler loop
    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                match msg {
                    ServerMsg::Submit(job) => sched.submit(job),
                    // status queries handled per‑connection
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                sched.tick().await;
            }
        }
    }
}

enum ServerMsg {
    Submit(JobSpec),
}

async fn handle_client(mut stream: UnixStream, tx: mpsc::UnboundedSender<ServerMsg>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let mut reader = BufReader::new(&mut stream);
    let mut buf = String::new();
    if reader.read_line(&mut buf).await.ok().filter(|&n| n > 0).is_none() {
        return;
    }
    match serde_json::from_str::<serde_json::Value>(&buf) {
        Ok(v) => match v["type"].as_str() {
            Some("submit") => {
                if let (Some(cmd), Some(gpus)) = (v["cmd"].as_str(), v["gpus"].as_u64()) {
                    let job = JobSpec { id: Uuid::new_v4(), cmd: cmd.into(), gpus: gpus as usize };
                    let _ = tx.send(ServerMsg::Submit(job));
                    let _ = stream.write_all(b"{\"ok\":true}\n").await;
                }
            }
            Some("status") => {
                // For brevity: return stub
                let _ = stream.write_all(b"{\"running\":[],\"queued\":[]}\n").await;
            }
            _ => {}
        },
        Err(e) => {
            let _ = stream.write_all(format!("{{\"error\":{e}}}\n").as_bytes()).await;
        }
    }
}
