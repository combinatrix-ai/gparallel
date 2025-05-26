/************************  src/main.rs ********************************/

use anyhow::Result;
use clap::Parser;
use tokio::{io::{self, AsyncBufReadExt, BufReader}, time::{sleep, Duration}};

mod scheduler;
use scheduler::Scheduler;

/// gparallel — 1GPU x multi‑process scheduler
#[derive(Parser)]
#[command(author, version, about = "simple gpu‑wise parallel executor")]
struct Cli {}

#[tokio::main]
async fn main() -> Result<()> {
    let _cli = Cli::parse();

    let sched = Scheduler::new().await?;

    // 読み取りは行単位
    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    while let Some(line) = lines.next_line().await? {
        let cmd = line.trim();
        if cmd.is_empty() { continue; }
        sched.submit(cmd.to_string()).await?;
    }

    // すべてのジョブが終わるまで待機
    loop {
        if sched.is_idle().await {
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}
