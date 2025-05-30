# gparallel 🖥️🚀

**gparallel** is a *GPU-aware parallel job scheduler* with a ** tmux-like TUI** for managing GPU workloads on single machines. Perfect for researchers and ML engineers who need to maximize GPU utilization without the complexity of cluster managers.

![gparallel](https://img.shields.io/badge/version-0.2.1-blue.svg)
![License](https://img.shields.io/badge/license-MIT-green.svg)

## Features

- 🚀 **Automatic GPU allocation** - Jobs distributed across available GPUs
- 📊 **Real-time TUI** - Beautiful terminal interface with GPU status, job queue, and live logs
- 📜 **Smart scrolling** - Navigate through large job queues with arrow keys
- 🔄 **Job state tracking** - Visual indicators for QUEUE, RUN, DONE, and FAIL states
- 💾 **GPU memory monitoring** - Real-time memory usage with color-coded indicators
- 🎯 **GPU status indicators** - ● (running) / ○ (idle) status for each GPU
- ⚡ **Non-blocking execution** - Jobs start immediately as GPUs become available
- 🛑 **Graceful shutdown** - Ctrl+C kills all running jobs cleanly

---

## Quick Start

```bash
# Create a command list (one command per line)
cat > commands.txt <<'EOF'
python train.py --model bert --epochs 10
python train.py --model gpt2 --epochs 20
python eval.py --checkpoint model_best.pt
EOF

# Run with TUI (default)
gparallel commands.txt

# Run without TUI for CI/scripting
gparallel commands.txt --no-tui
```

## Terminal UI

When running in TUI mode, gparallel displays a comprehensive interface:

```
┌─ GPUs ───────────────────┐┌─ Job queue ──────────────────────────────────┐
│0 ● RTX4090  20312 MB     ││3c93a4f1 train.py --model bert    RUN  G0    │
│1 ○ RTX4090  24576 MB     ││[f1ed8a92 train.py --model gpt2    QUEUE]    │ 
│2 ● RTX4090  18432 MB     ││a8b2c3d4 eval.py --checkpoint...  RUN  G2    │
└──────────────────────────┘└──────────────────────────────────────────────┘
┌─ Live log : job #f1ed8a92 (tail -f) ─────────────────────────────────┐
│No logs yet for job f1ed8a92 (train.py --model gpt2)                  │
│                                                                       │
└───────────────────────────────────────────────────────────────────────┘
↑/↓ Navigate jobs  q Quit (jobs continue)  Ctrl+C Force quit & stop all jobs  Auto-exit when all jobs complete
```

### UI Components

1. **GPU Panel** (top-left)
   - GPU ID and name
   - Status indicator: ● (running job) / ○ (idle)
   - Available memory in MB with color coding:
     - 🟢 Green: <50% usage
     - 🟡 Yellow: 50-80% usage
     - 🔴 Red: >80% usage

2. **Job Queue Panel** (top-right)
   - Job ID (first 8 chars of UUID)
   - Command (truncated if too long)
   - State: QUEUE, RUN (with GPU), DONE, or FAIL
   - Scrollable with ↑/↓ keys when many jobs exist

3. **Live Log Panel** (bottom)
   - Shows stdout/stderr from selected job
   - Auto-selects first job
   - Updates in real-time
   - Limited to last 1000 lines per job

### Keyboard Controls

- **↑/↓** - Navigate through jobs in the queue
- **q** - Quit gparallel (jobs continue running in background)
- **Ctrl+C** - Force quit and terminate all running jobs

---

## Installation

### From Source (Recommended)

```bash
# Clone the repository
git clone https://github.com/yourusername/gparallel
cd gparallel

# Build with Cargo
cargo build --release

# Copy to your PATH
sudo cp target/release/gparallel /usr/local/bin/
```

### Using Cargo

```bash
# Once published to crates.io
cargo install gparallel
```

### Dependencies

- Rust 1.70+
- NVIDIA drivers and CUDA toolkit
- Terminal with UTF-8 support for UI elements

---

## Usage

### Basic Usage

```bash
# Run jobs from a file
gparallel jobs.txt

# Specify visible GPUs
CUDA_VISIBLE_DEVICES=0,2,4 gparallel jobs.txt

# Disable TUI for scripts/CI
gparallel jobs.txt --no-tui
```

### Command File Format

Create a text file with one command per line:

```bash
# train_jobs.txt
python train.py --config configs/experiment1.yaml
python train.py --config configs/experiment2.yaml
python train.py --config configs/experiment3.yaml
./run_benchmark.sh --gpu-test
jupyter nbconvert --execute notebook.ipynb
```

### Generating Commands Dynamically

```bash
# Generate parameter sweep
for lr in 0.01 0.001 0.0001; do
    for bs in 32 64 128; do
        echo "python train.py --lr $lr --batch-size $bs"
    done
done > sweep_jobs.txt

gparallel sweep_jobs.txt
```

---

## How It Works

1. **GPU Detection**
   - Respects `CUDA_VISIBLE_DEVICES` if set
   - Uses NVML for GPU information and memory monitoring
   - Falls back to `nvidia-smi` if NVML unavailable
   - Assumes single GPU if detection fails

2. **Job Scheduling**
   - Round-robin assignment to available GPUs
   - Jobs queued when all GPUs busy
   - Immediate dispatch when GPU becomes free
   - Each job gets exclusive GPU via `CUDA_VISIBLE_DEVICES`

3. **Process Management**
   - Spawns jobs via `bash -c`
   - Captures stdout/stderr to memory buffers
   - Tracks process IDs for signal handling
   - Updates job states in real-time

4. **Memory Monitoring**
   - Polls GPU memory every 2 seconds
   - Updates display with current free memory
   - Color-codes based on usage percentage

---

## Command Line Options

```
gparallel [OPTIONS] <FILENAME>

Arguments:
  <FILENAME>  File containing commands to execute (one per line)

Options:
      --no-tui                     Disable TUI and use plain text output
  -h, --help                       Print help
  -V, --version                    Print version
```

---

## Troubleshooting

### GPU Detection Issues

If gparallel shows incorrect GPUs:
```bash
# Check NVIDIA driver
nvidia-smi

# Force specific GPUs
export CUDA_VISIBLE_DEVICES=0,1,2
gparallel jobs.txt
```

### TUI Not Displaying

If TUI doesn't appear:
```bash
# Check terminal capabilities
echo $TERM

# Force non-TUI mode
gparallel jobs.txt --no-tui
```

### Jobs Not Starting

Common causes:
- All GPUs busy (check GPU panel)
- Previous job hasn't released GPU yet
- Command syntax error (check failed jobs)

---

## Contributing

Contributions welcome! Please open an issue or submit a pull request.
### Development Setup

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/yourusername/gparallel
cd gparallel
cargo build

# Run tests
cargo test

# Run with debug output
RUST_LOG=debug cargo run -- test_jobs.txt
```

---

## License

MIT License - see [LICENSE](LICENSE) file for details.
