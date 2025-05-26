/************************  src/standalone.rs *************************/

use anyhow::Result;
use tokio::io::{self, AsyncBufReadExt};

use crate::scheduler::Scheduler;

pub async fn run(cmd_args: Vec<String>) -> Result<()> {
    let mut sched = Scheduler::new();

    // 1) コマンドラインに渡された場合は 1 つのジョブとして扱う
    if !cmd_args.is_empty() {
        sched.submit(cmd_args.join(" ")).await;
    } else {
        // 2) 引数がなければ STDIN から 1 行 = 1 ジョブ を読む
        let mut lines = io::BufReader::new(io::stdin()).lines();
        while let Some(line) = lines.next_line().await? {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            sched.submit(trimmed.to_string()).await;
        }
    }

    // すべてのジョブが完了するまで待つ
    sched.wait_all().await
}
