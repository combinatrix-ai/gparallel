/************************  src/schduler.rs ********************************/

use anyhow::{Context, Result};
use std::{
    collections::VecDeque,
    env,
    process::Stdio,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{
    process::Command,
    sync::{mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender}, Mutex},
};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct JobSpec {
    id: Uuid,
    cmd: String,
}

pub struct Scheduler {
    queue: Arc<Mutex<VecDeque<JobSpec>>>,
    gpu_tx: UnboundedSender<u32>,
    gpu_rx: Arc<Mutex<UnboundedReceiver<u32>>>,
    busy: Arc<AtomicUsize>,
}

impl Scheduler {
    pub async fn new() -> Result<Self> {
        let gpus = detect_gpus().await?;
        if gpus.is_empty() {
            anyhow::bail!("No GPUs detected");
        }

        let (tx, rx) = unbounded_channel();
        for id in &gpus {
            tx.send(*id)?;
        }

        Ok(Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            gpu_tx: tx,
            gpu_rx: Arc::new(Mutex::new(rx)),
            busy: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub async fn submit(&self, cmd: String) -> Result<()> {
        let job = JobSpec { id: Uuid::new_v4(), cmd };
        if let Some(gpu) = { self.gpu_rx.lock().await.try_recv().ok() } {
            self.spawn_job(job, gpu).await?;
        } else {
            self.queue.lock().await.push_back(job);
        }
        Ok(())
    }

    async fn spawn_job(&self, job: JobSpec, gpu: u32) -> Result<()> {
        println!("[gparallel] launch job {} on GPU {}: {}", job.id, gpu, job.cmd);
        self.busy.fetch_add(1, Ordering::SeqCst);

        let queue = self.queue.clone();
        let tx = self.gpu_tx.clone();
        let busy = self.busy.clone();

        let mut child = Command::new("bash");
        child.arg("-c").arg(&job.cmd);
        child.env("CUDA_VISIBLE_DEVICES", gpu.to_string());
        child.stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::inherit());

        tokio::spawn(async move {
            let _ = child.status().await;
            println!("[gparallel] finished job {} (GPU {})", job.id, gpu);

            loop {
                // 1. try to fetch next job for same GPU
                let maybe_job = {
                    let mut q = queue.lock().await;
                    q.pop_front()
                };

                match maybe_job {
                    Some(next) => {
                        // launch next job (reusing same GPU)
                        println!("[gparallel] launch job {} on GPU {}: {}", next.id, gpu, next.cmd);
                        let mut next_child = Command::new("bash");
                        next_child.arg("-c").arg(&next.cmd);
                        next_child.env("CUDA_VISIBLE_DEVICES", gpu.to_string());
                        next_child.stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
                        if let Err(e) = next_child.status().await {
                            eprintln!("[gparallel] ERROR: {e}");
                        }
                        println!("[gparallel] finished job {} (GPU {})", next.id, gpu);
                        // continue loop to see if more jobs remain
                        continue;
                    }
                    None => {
                        // no queued job, release GPU
                        tx.send(gpu).ok();
                        busy.fetch_sub(1, Ordering::SeqCst);
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    pub async fn is_idle(&self) -> bool {
        self.queue.lock().await.is_empty() && self.busy.load(Ordering::SeqCst) == 0
    }
}

// ------------------------------------------------
// GPU detection helpers (same as before)
// ------------------------------------------------
async fn detect_gpus() -> Result<Vec<u32>> {
    if let Ok(list) = env::var("CUDA_VISIBLE_DEVICES") {
        let ids: Vec<u32> = list.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if !ids.is_empty() {
            return Ok(ids);
        }
    }
    if let Ok(nvml) = nvml_wrapper::Nvml::init() {
        if let Ok(count) = nvml.device_count() {
            if count > 0 {
                return Ok((0..count).map(|i| i as u32).collect());
            }
        }
    }
    if let Ok(path) = which::which("nvidia-smi") {
        if let Ok(out) = Command::new(path).arg("-L").output().await {
            if out.status.success() {
                let n = String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.contains("GPU")).count();
                if n > 0 {
                    return Ok((0..n).map(|i| i as u32).collect());
                }
            }
        }
    }
    eprintln!("[gparallel] WARN: cannot detect GPUs → use GPU0 only");
    Ok(vec![0])
}
