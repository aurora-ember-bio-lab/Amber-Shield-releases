# Changelog

## 0.2.0 (2026-09-21)

### Added

- **Vectorized search**: Embedding client, in-memory cosine similarity store, pgvector SQL builders, auto-index events on arrival
- **Scheduled tasks**: Cron (6-field) and interval-based scheduling, JSON persistence, background executor (10s polling)
- **Process monitor**: Snapshot all processes via sysinfo, persist to hot store behavior graph
- **Settings UI**: Configure log watch paths, saved to config.json
- **License flow**: Install/verify Ed25519 licenses via UI, hardware fingerprint display
- **Semantic search UI**: Text input + cosine similarity results display
- **CSP tightened**: Replaced `null` with explicit content security policy
- **rust-toolchain.toml**: Pinned to stable channel with clippy + rustfmt
- **AGENTS.md**: Build/test/lint commands and architecture documentation

### Fixed

- Watch paths config now actually passed to log source spawner on startup
- `ProcessCheck` scheduled task now persists results to hot store
- `LogCollect` and `VectorIndex` scheduled tasks now report real status instead of hardcoded strings
- Removed dead code in `security/gitleaks.rs` (unreachable `!output.status.success()` check)
- Fixed dangling doc link to non-existent `crate::behavior` module
- Removed unused `HashMap` import in vectorized module

### Changed

- `emit_and_persist` now auto-indexes every event into `MemoryVectorStore`
- Background task executor passes `AppState` to `execute_task` for direct store access
- `restart_log_sources` command added for config reload support

## 0.1.0 (2026-09-20)

Initial release.

- Code heatmap (Trivy + GitLeaks + tree-sitter)
- Event stream (hot/cold storage)
- Canvas2D HUD with severity-based visualization
- Local AI explain-drawer (Ollama)
- Cross-platform log interceptors (Linux/macOS/Windows)
- Hardware fingerprinting + Ed25519 license verification
- Tauri 2 desktop shell
