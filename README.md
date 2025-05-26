# gparallel 🖥️🚀

**gparallel** is a *single‑binary GPU scheduler* for desktops, workstations and small on‑prem servers—**think “GNU parallel for CUDA GPUs.”**
It fills the gap between handwritten shell loops and heavyweight cluster managers such as Slurm, Kubernetes, or run.ai. 

* Drop the binary on a machine that has multiple CUDA GPUs.
* Pipe a list of commands to **gparallel**.
* Each command runs on exactly one free GPU.
* As soon as a job finishes, the GPU is immediately re‑used for the next item in the queue.

> **Made for researchers, data‑scientists and hobbyists** who want to saturate every GPU in a
> single box without rewriting their scripts or managing a cluster.

---

## Quick Start

```bash
# Create a command list (1 command per line)
cat > commands.txt <<'EOF'
python train.py --cfg exp1
python train.py --cfg exp2
python train.py --cfg exp3
EOF

# Fire and forget 🏃‍♂️
cat commands.txt | gparallel
```

Typical output (8 × A100 example):
```
[gparallel] launch job 3c93… on GPU 0: python train.py --cfg exp1
[gparallel] launch job f1ed… on GPU 1: python train.py --cfg exp2
…
[gparallel] finished job 3c93… (GPU 0)
[gparallel] launch job 7a61… on GPU 0: python train.py --cfg exp4
```

---

## Installation

```bash
# Using Cargo (recommended)
cargo install gparallel            # crates.io (once published)

# Or build from source
cargo build --release              # binary at target/release/gparallel
```

The binary is fully static (musl) and contains no runtime dependencies besides the NVIDIA driver.

---

## How It Works

1. **Detect GPUs** in the following order:
   1. Use the IDs listed in `CUDA_VISIBLE_DEVICES` if present.
   2. Query NVML for the physical device count.
   3. Fallback to counting lines of `nvidia‑smi -L`.
   4. If all else fails, assume GPU 0 and print a warning.
2. Maintain two queues:
   * `gpu_pool` — free GPU IDs (Tokio unbounded channel).
   * `job_queue` — pending shell commands (async `Mutex<VecDeque<…>>`).
3. When a command arrives it is either executed immediately (if a GPU is free) or
   stored in `job_queue`.
4. Each job is spawned via `bash -c …` with `CUDA_VISIBLE_DEVICES` set to a single ID.
5. After a job exits, the same GPU picks the next queued command; if none are left it
   returns the GPU to `gpu_pool`.

Everything runs on a single Tokio runtime; there is **no busy‑waiting**.

---

## Roadmap

* `-n <GPUs>` — allow a job to consume *n* GPUs simultaneously.
* Daemon mode with Unix‑socket to accept jobs while running.
* Curses / Web UI for live monitoring.
* Persistent queue (sled / sqlite) for crash‑safe recovery.

---

## FAQ

| Question | Answer |
|----------|--------|
| *Can I run multi‑GPU jobs?* | Not yet. A future `-n` flag will reserve multiple GPUs per command. |
| *Does it work on Windows?*  | It runs under WSL 2 with NVIDIA pass‑through. Native Windows builds are untested. |
| *How do I limit to a subset of GPUs?* | Set `CUDA_VISIBLE_DEVICES` before invoking **gparallel**. |

---

## License

MIT © 2025 combinatrix-ai

