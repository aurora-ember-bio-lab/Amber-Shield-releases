# Amber Shield Lite — Agent Instructions

## Build

```bash
# Full workspace build
cargo build

# Release build (optimized, LTO, stripped)
cargo build --release

# Core engine only
cargo build -p core-engine
```

## Test

```bash
# All tests
cargo test

# Core engine tests only
cargo test -p core-engine

# With output
cargo test -- --nocapture
```

## Lint & Format

```bash
# Clippy (no warnings)
cargo clippy -- -D warnings

# Format check
cargo fmt --check

# Auto-format
cargo fmt
```

## Frontend

The frontend is static HTML/JS/CSS in `frontend/`. No build step required.

```bash
# Preview locally
cd frontend && python -m http.server 8080

# Deploy to Vercel
cd frontend && vercel --prod
```

## Architecture

```
crates/
  core-engine/    # Framework-agnostic Rust library (no Tauri deps)
  tauri-app/      # Thin Tauri shell, IPC commands, background tasks
frontend/         # Static HTML/JS/CSS, Canvas2D HUD
```

### Core Engine Modules

| Module | Purpose |
|---|---|
| `types` | Event, Severity, EventSource — shared vocabulary |
| `storage::hot` | Sled-backed process graph + event ring buffer |
| `storage::cold` | SQLite (default) + DuckDB (optional) historical store |
| `vectorized` | Ollama embeddings + in-memory cosine similarity search |
| `scheduler` | Cron/interval task definitions + JSON persistence |
| `security` | Trivy + GitLeaks wrappers |
| `codemap` | Tree-sitter file heat-scoring |
| `llm` | Local Ollama client |
| `logwatch` | Cross-platform log interceptors (Linux/macOS/Windows) |
| `hwid` | Hardware fingerprint + Ed25519 license verification |

### Tauri IPC Commands

| Command | Purpose |
|---|---|
| `get_recent_events` | Read from hot store ring buffer |
| `get_processes` | Read process graph from hot store |
| `scan_processes` | Snapshot all running processes via sysinfo |
| `scan_code_heatmap` | Run Trivy + GitLeaks + codemap |
| `explain_event_with_llm` | Ask Ollama to explain an event |
| `embed_text` | Generate embedding via Ollama |
| `vector_search` | Cosine similarity search |
| `get_config` / `save_config` | Read/write settings (watch paths) |
| `list_scheduled_tasks` | List all scheduled tasks |
| `create_scheduled_task` | Create with interval or cron |
| `run_task_now` | Execute a task immediately |
| `install_license` / `license_status` | License management |

## Key Design Decisions

- **Framework-agnostic core**: `core-engine` has zero Tauri dependencies. The shell can be swapped.
- **Hot/cold/vector split**: Hot = live state (sled), Cold = history (SQLite), Vector = embeddings (in-memory).
- **Cron uses 6-field syntax**: `second minute hour day month dow` (via `cron` crate v0.15).
- **Ollama only**: No cloud AI. All embeddings and explanations run on `127.0.0.1:11434`.
- **Graceful degradation**: Missing Trivy/GitLeaks/Ollama = empty results, not crashes.

## Environment Variables

| Variable | Default | Purpose |
|---|---|---|
| `AMBER_EMBED_MODEL` | `nomic-embed-text:v1.5` | Ollama embedding model |
| `AMBER_EMBED_URL` | `http://127.0.0.1:11434/api/embeddings` | Ollama embedding endpoint |
| `AMBER_OLLAMA_MODEL` | `llama3.2:1b` | Ollama generation model |
| `AMBER_TRIVY_BIN` | (auto-detect) | Path to trivy binary |
| `AMBER_GITLEAKS_BIN` | (auto-detect) | Path to gitleaks binary |
| `AMBER_SHIELD_LICENSE_PUBLIC_KEY_HEX` | (none) | Ed25519 public key for license verification |
