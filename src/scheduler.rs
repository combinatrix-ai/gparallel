/************************  src/schduler.rs ********************************/

use anyhow::Result;
use std::{
    collections::{HashMap, VecDeque},
    env,
    process::Stdio,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncBufReadExt, BufReader as AsyncBufReader},
    process::Command,
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        Mutex, RwLock,
    },
};
use uuid::Uuid;

use crate::ui::{AppState, GpuInfo, JobInfo, JobState};

#[derive(Debug, Clone)]
pub struct JobSpec {
    pub id: Uuid,
    pub cmd: String,
}

#[derive(Clone)]
pub struct Scheduler {
    queue: Arc<Mutex<VecDeque<JobSpec>>>,
    gpu_tx: UnboundedSender<u32>,
    gpu_rx: Arc<Mutex<UnboundedReceiver<u32>>>,
    busy: Arc<AtomicUsize>,
    app_state: Arc<RwLock<AppState>>,
    _gpu_names: Vec<String>,
    running_jobs: Arc<Mutex<HashMap<Uuid, u32>>>, // job_id -> PID
    use_tui: bool,
}

impl Scheduler {
    pub async fn new(app_state: Arc<RwLock<AppState>>, use_tui: bool) -> Result<Self> {
        let (gpus, gpu_names) = detect_gpus_with_info().await?;
        if gpus.is_empty() {
            anyhow::bail!("No GPUs detected");
        }

        let (tx, rx) = unbounded_channel();
        for id in &gpus {
            tx.send(*id)?;
        }

        // Initialize GPU info in app state
        {
            let mut state = app_state.write().await;
            state.gpus = gpus
                .iter()
                .zip(gpu_names.iter())
                .map(|(id, name)| GpuInfo {
                    id: *id,
                    name: name.clone(),
                    free_memory_mb: 0,
                    total_memory_mb: 0,
                })
                .collect();
        }

        // Start GPU memory monitoring
        let state_clone = app_state.clone();
        tokio::spawn(async move {
            loop {
                update_gpu_memory_info(&state_clone).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        });

        Ok(Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            gpu_tx: tx,
            gpu_rx: Arc::new(Mutex::new(rx)),
            busy: Arc::new(AtomicUsize::new(0)),
            app_state,
            _gpu_names: gpu_names,
            running_jobs: Arc::new(Mutex::new(HashMap::new())),
            use_tui,
        })
    }

    pub async fn submit(&self, cmd: String) -> Result<()> {
        let job = JobSpec {
            id: Uuid::new_v4(),
            cmd: cmd.clone(),
        };

        // Add job to UI state
        {
            let mut state = self.app_state.write().await;
            state.jobs.push(JobInfo {
                id: job.id,
                cmd: cmd.clone(),
                state: JobState::Queued,
                log_lines: VecDeque::new(),
            });
        }

        if let Some(gpu) = { self.gpu_rx.lock().await.try_recv().ok() } {
            self.spawn_job(job, gpu).await?;
        } else {
            self.queue.lock().await.push_back(job);
        }
        Ok(())
    }

    async fn spawn_job(&self, job: JobSpec, gpu: u32) -> Result<()> {
        self.busy.fetch_add(1, Ordering::SeqCst);

        // Update job state to running
        {
            let mut state = self.app_state.write().await;
            if let Some(job_info) = state.jobs.iter_mut().find(|j| j.id == job.id) {
                job_info.state = JobState::Running { gpu_id: gpu };
            }
        }

        let queue = self.queue.clone();
        let tx = self.gpu_tx.clone();
        let busy = self.busy.clone();
        let app_state = self.app_state.clone();
        let running_jobs = self.running_jobs.clone();
        let use_tui = self.use_tui;

        let mut child = Command::new("bash");
        child.arg("-c").arg(&job.cmd);
        child.env("CUDA_VISIBLE_DEVICES", gpu.to_string());

        if self.use_tui {
            child
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        } else {
            child
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        }

        tokio::spawn(async move {
            let mut child_process = match child.spawn() {
                Ok(cp) => cp,
                Err(e) => {
                    eprintln!("[gparallel] Failed to spawn job {}: {}", job.id, e);
                    // Update job state to failed
                    {
                        let mut state = app_state.write().await;
                        if let Some(job_info) = state.jobs.iter_mut().find(|j| j.id == job.id) {
                            job_info.state = JobState::Failed;
                        }
                    }
                    tx.send(gpu).ok();
                    busy.fetch_sub(1, Ordering::SeqCst);
                    return;
                }
            };

            // Track the PID
            if let Some(pid) = child_process.id() {
                running_jobs.lock().await.insert(job.id, pid);
            }

            // Capture stdout (only in TUI mode)
            if use_tui {
                if let Some(stdout) = child_process.stdout.take() {
                    let job_id = job.id;
                    let state_clone = app_state.clone();
                    tokio::spawn(async move {
                        let reader = AsyncBufReader::new(stdout);
                        let mut lines = reader.lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            let mut state = state_clone.write().await;
                            if let Some(job_info) = state.jobs.iter_mut().find(|j| j.id == job_id) {
                                job_info.log_lines.push_back(line.clone());
                                if job_info.log_lines.len() > 1000 {
                                    job_info.log_lines.pop_front();
                                }
                            }
                        }
                    });
                }

                // Capture stderr
                if let Some(stderr) = child_process.stderr.take() {
                    let job_id = job.id;
                    let state_clone = app_state.clone();
                    tokio::spawn(async move {
                        let reader = AsyncBufReader::new(stderr);
                        let mut lines = reader.lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            let mut state = state_clone.write().await;
                            if let Some(job_info) = state.jobs.iter_mut().find(|j| j.id == job_id) {
                                job_info.log_lines.push_back(format!("[stderr] {}", line));
                                if job_info.log_lines.len() > 1000 {
                                    job_info.log_lines.pop_front();
                                }
                            }
                        }
                    });
                }
            }

            let status = child_process.wait().await;

            // Update job state based on exit status
            {
                let mut state = app_state.write().await;
                if let Some(job_info) = state.jobs.iter_mut().find(|j| j.id == job.id) {
                    job_info.state = match status {
                        Ok(s) if s.success() => JobState::Completed,
                        _ => JobState::Failed,
                    };
                }
            }

            // Remove from running jobs
            running_jobs.lock().await.remove(&job.id);

            loop {
                // 1. try to fetch next job for same GPU
                let maybe_job = {
                    let mut q = queue.lock().await;
                    q.pop_front()
                };

                match maybe_job {
                    Some(next) => {
                        // Update existing job state to running
                        {
                            let mut state = app_state.write().await;
                            if let Some(job_info) = state.jobs.iter_mut().find(|j| j.id == next.id)
                            {
                                job_info.state = JobState::Running { gpu_id: gpu };
                            }
                        }

                        // launch next job (reusing same GPU)
                        let mut next_child = Command::new("bash");
                        next_child.arg("-c").arg(&next.cmd);
                        next_child.env("CUDA_VISIBLE_DEVICES", gpu.to_string());

                        if use_tui {
                            next_child
                                .stdin(Stdio::null())
                                .stdout(Stdio::piped())
                                .stderr(Stdio::piped());
                        } else {
                            next_child
                                .stdin(Stdio::null())
                                .stdout(Stdio::inherit())
                                .stderr(Stdio::inherit());
                        }

                        let mut child_process = match next_child.spawn() {
                            Ok(cp) => cp,
                            Err(e) => {
                                eprintln!("[gparallel] Failed to spawn job {}: {}", next.id, e);
                                // Update job state to failed
                                {
                                    let mut state = app_state.write().await;
                                    if let Some(job_info) =
                                        state.jobs.iter_mut().find(|j| j.id == next.id)
                                    {
                                        job_info.state = JobState::Failed;
                                    }
                                }
                                continue;
                            }
                        };

                        // Track the PID
                        if let Some(pid) = child_process.id() {
                            running_jobs.lock().await.insert(next.id, pid);
                        }

                        // Capture stdout (only in TUI mode)
                        if use_tui {
                            if let Some(stdout) = child_process.stdout.take() {
                                let job_id = next.id;
                                let state_clone = app_state.clone();
                                tokio::spawn(async move {
                                    let reader = AsyncBufReader::new(stdout);
                                    let mut lines = reader.lines();
                                    while let Ok(Some(line)) = lines.next_line().await {
                                        let mut state = state_clone.write().await;
                                        if let Some(job_info) =
                                            state.jobs.iter_mut().find(|j| j.id == job_id)
                                        {
                                            job_info.log_lines.push_back(line.clone());
                                            if job_info.log_lines.len() > 1000 {
                                                job_info.log_lines.pop_front();
                                            }
                                        }
                                    }
                                });
                            }

                            // Capture stderr
                            if let Some(stderr) = child_process.stderr.take() {
                                let job_id = next.id;
                                let state_clone = app_state.clone();
                                tokio::spawn(async move {
                                    let reader = AsyncBufReader::new(stderr);
                                    let mut lines = reader.lines();
                                    while let Ok(Some(line)) = lines.next_line().await {
                                        let mut state = state_clone.write().await;
                                        if let Some(job_info) =
                                            state.jobs.iter_mut().find(|j| j.id == job_id)
                                        {
                                            job_info
                                                .log_lines
                                                .push_back(format!("[stderr] {}", line));
                                            if job_info.log_lines.len() > 1000 {
                                                job_info.log_lines.pop_front();
                                            }
                                        }
                                    }
                                });
                            }
                        }

                        let status = child_process.wait().await;

                        // Update job state based on exit status
                        {
                            let mut state = app_state.write().await;
                            if let Some(job_info) = state.jobs.iter_mut().find(|j| j.id == next.id)
                            {
                                job_info.state = match status {
                                    Ok(s) if s.success() => JobState::Completed,
                                    _ => JobState::Failed,
                                };
                            }
                        }

                        // Remove from running jobs
                        running_jobs.lock().await.remove(&next.id);

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

    pub async fn kill_all_jobs(&self) {
        let jobs = self.running_jobs.lock().await;
        for (job_id, pid) in jobs.iter() {
            println!("[gparallel] Killing job {} (PID {})", job_id, pid);
            // Use nix to send SIGTERM to the process
            if let Err(e) = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(*pid as i32),
                nix::sys::signal::Signal::SIGTERM,
            ) {
                eprintln!("[gparallel] Failed to kill job {}: {}", job_id, e);
            }
        }

        // Give processes a moment to terminate gracefully
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Force kill any remaining processes
        let jobs = self.running_jobs.lock().await;
        for (job_id, pid) in jobs.iter() {
            if let Err(e) = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(*pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            ) {
                // Process might have already terminated
                if e != nix::errno::Errno::ESRCH {
                    eprintln!("[gparallel] Failed to force kill job {}: {}", job_id, e);
                }
            }
        }
    }
}

// ------------------------------------------------
// GPU detection helpers
// ------------------------------------------------
async fn detect_gpus_with_info() -> Result<(Vec<u32>, Vec<String>)> {
    if let Ok(list) = env::var("CUDA_VISIBLE_DEVICES") {
        let ids: Vec<u32> = list
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if !ids.is_empty() {
            let names = vec!["GPU".to_string(); ids.len()];
            return Ok((ids, names));
        }
    }

    // Try NVML first for better GPU info
    if let Ok(nvml) = nvml_wrapper::Nvml::init() {
        if let Ok(count) = nvml.device_count() {
            if count > 0 {
                let mut ids = Vec::new();
                let mut names = Vec::new();
                for i in 0..count {
                    ids.push(i as u32);
                    if let Ok(device) = nvml.device_by_index(i) {
                        if let Ok(name) = device.name() {
                            names.push(name);
                        } else {
                            names.push(format!("GPU{}", i));
                        }
                    } else {
                        names.push(format!("GPU{}", i));
                    }
                }
                return Ok((ids, names));
            }
        }
    }

    // Fallback to nvidia-smi
    if let Ok(out) = Command::new("nvidia-smi").arg("-L").output().await {
        if out.status.success() {
            let output = String::from_utf8_lossy(&out.stdout);
            let mut ids = Vec::new();
            let mut names = Vec::new();

            for (i, line) in output.lines().enumerate() {
                if line.contains("GPU") {
                    ids.push(i as u32);
                    // Try to parse GPU name from line like "GPU 0: NVIDIA GeForce RTX 4090 (UUID: ...)"
                    if let Some(start) = line.find(':') {
                        if let Some(end) = line.find('(') {
                            let name = line[start + 1..end].trim();
                            names.push(name.to_string());
                        } else {
                            names.push(format!("GPU{}", i));
                        }
                    } else {
                        names.push(format!("GPU{}", i));
                    }
                }
            }

            if !ids.is_empty() {
                return Ok((ids, names));
            }
        }
    }

    eprintln!("[gparallel] WARN: cannot detect GPUs → use GPU0 only");
    Ok((vec![0], vec!["GPU0".to_string()]))
}

async fn update_gpu_memory_info(app_state: &Arc<RwLock<AppState>>) {
    if let Ok(nvml) = nvml_wrapper::Nvml::init() {
        let mut state = app_state.write().await;
        for gpu_info in state.gpus.iter_mut() {
            if let Ok(device) = nvml.device_by_index(gpu_info.id) {
                if let Ok(mem_info) = device.memory_info() {
                    gpu_info.free_memory_mb = mem_info.free / (1024 * 1024);
                    gpu_info.total_memory_mb = mem_info.total / (1024 * 1024);
                }
            }
        }
    }
}
