/************************  src/main.rs ********************************/

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::{
    signal,
    sync::RwLock,
    time::{sleep, Duration},
};

mod scheduler;
mod ui;
use scheduler::Scheduler;
use ui::{AppState, UI};

/// gparallel — 1GPU x multi‑process scheduler
#[derive(Parser)]
#[command(author, version, about = "simple gpu‑wise parallel executor")]
struct Cli {
    /// File containing commands to execute (one per line)
    filename: String,

    /// Disable TUI and use plain text output
    #[arg(long)]
    no_tui: bool,

    /// Maximum runtime for each job (e.g., "4h", "30m")
    #[arg(long)]
    max_runtime: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine if we should use TUI
    let stdout_is_tty = atty::is(atty::Stream::Stdout);
    let use_tui = !cli.no_tui && stdout_is_tty;

    // Create shared app state
    let app_state = Arc::new(RwLock::new(AppState::new()));

    // Create scheduler with app state
    let sched = Scheduler::new(app_state.clone(), use_tui).await?;

    // Read commands from file
    let file_content = tokio::fs::read_to_string(&cli.filename)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", cli.filename, e))?;

    for line in file_content.lines() {
        let cmd = line.trim();
        if !cmd.is_empty() {
            sched.submit(cmd.to_string()).await?;
        }
    }

    if use_tui {
        // Try to spawn UI, fall back to non-TUI mode if it fails
        let ui_result = UI::new(app_state.clone()).await;
        match ui_result {
            Ok(ui) => {
                let ui_handle = tokio::spawn(async move { ui.run().await });

                // Set up Ctrl+C handler
                let ctrlc_state = app_state.clone();
                let ctrlc_sched = sched.clone();
                let ctrlc_handle = tokio::spawn(async move {
                    signal::ctrl_c()
                        .await
                        .expect("Failed to install Ctrl+C handler");
                    // Set should_quit flag
                    let mut state = ctrlc_state.write().await;
                    state.should_quit = true;
                    // Kill all running jobs
                    ctrlc_sched.kill_all_jobs().await;
                });

                // Wait for UI to exit (user pressed 'q' or Ctrl+C)
                tokio::select! {
                    result = ui_handle => {
                        result??;
                    }
                    _ = ctrlc_handle => {
                        // Ctrl+C was pressed, jobs have been killed
                        println!("\\nAll jobs terminated.");
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "[gparallel] Failed to initialize TUI: {}, falling back to plain text mode",
                    e
                );

                // Fall back to non-TUI mode - wait for jobs to complete
                let ctrlc_sched = sched.clone();
                tokio::spawn(async move {
                    signal::ctrl_c()
                        .await
                        .expect("Failed to install Ctrl+C handler");
                    println!("\\n[gparallel] Caught Ctrl+C, terminating all jobs...");
                    ctrlc_sched.kill_all_jobs().await;
                    std::process::exit(1);
                });

                // Wait for all jobs to complete
                loop {
                    if sched.is_idle().await {
                        break;
                    }
                    sleep(Duration::from_millis(100)).await;
                }
            }
        }
    } else {
        // Non-TUI mode

        // Set up Ctrl+C handler for non-TUI mode
        let ctrlc_sched = sched.clone();
        tokio::spawn(async move {
            signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
            println!("\n[gparallel] Caught Ctrl+C, terminating all jobs...");
            ctrlc_sched.kill_all_jobs().await;
            std::process::exit(1);
        });

        // Wait for all jobs to complete
        loop {
            if sched.is_idle().await {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    Ok(())
}
