//! Thin Tauri shell over `core-engine`. Everything with actual logic lives
//! in that crate (see its `lib.rs` doc comment for the module map); this
//! file's only job is wiring: build app state, spawn the background
//! collectors, forward what they produce to the frontend.
//!
//! This is the free/lite edition: it wires the code-vulnerability
//! heatmap, the local AI drawer, and the log-event collectors. There is
//! no process-behavior loop here - that's a paid-tier feature that lives
//! only in the private Amber Shield source tree.
//!
//! IPC note (from the architecture review): the original spec proposed a
//! local WebSocket for UI<->core communication. That's skipped here in
//! favor of Tauri's native command/event bridge, which is faster and,
//! unlike an open localhost socket, isn't itself a small local attack
//! surface on a security product. Commands are request/response
//! (`invoke`); the collectors below push updates to the frontend with
//! `app.emit`, which the HUD subscribes to with `listen("amber://event")`.

mod commands;

use core_engine::storage::{HotStore, SqliteColdStore};
use core_engine::types::Event;
use std::sync::Arc;
use tauri::{Emitter, Manager};

pub struct AppState {
    pub hot_store: Arc<HotStore>,
    pub cold_store: Arc<std::sync::Mutex<SqliteColdStore>>,
    pub vector_store: Arc<std::sync::Mutex<core_engine::vectorized::MemoryVectorStore>>,
    pub task_store: Arc<std::sync::Mutex<core_engine::scheduler::TaskStore>>,
    pub data_dir: std::path::PathBuf,
}

const EVENT_CHANNEL: &str = "amber://event";

/// Read watch paths from config.json, returning an empty vec if missing.
fn read_watch_paths(data_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let config_path = data_dir.join("config.json");
    std::fs::read(&config_path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<commands::AppConfig>(&raw).ok())
        .map(|cfg| cfg.watch_paths.into_iter().map(std::path::PathBuf::from).collect())
        .unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("amber-shield-lite"));
            std::fs::create_dir_all(&data_dir).ok();

            let hot_store = Arc::new(
                HotStore::open(data_dir.join("hot.sled")).expect("open hot store"),
            );
            let cold_store = Arc::new(std::sync::Mutex::new(
                SqliteColdStore::open(data_dir.join("events.sqlite")).expect("open cold store"),
            ));

            let vector_store = Arc::new(std::sync::Mutex::new(
                core_engine::vectorized::MemoryVectorStore::new(),
            ));
            let task_store = Arc::new(std::sync::Mutex::new(
                core_engine::scheduler::TaskStore::open(data_dir.join("tasks.json")),
            ));

            // Read watch paths from config and pass to log spawner
            let watch_paths = read_watch_paths(&data_dir);
            tracing::info!("loaded {} watch paths from config", watch_paths.len());

            app.manage(AppState {
                hot_store: hot_store.clone(),
                cold_store: cold_store.clone(),
                vector_store: vector_store.clone(),
                task_store: task_store.clone(),
                data_dir: data_dir.clone(),
            });

            spawn_log_sources(
                app.handle().clone(),
                hot_store,
                cold_store,
                vector_store,
                watch_paths,
            );
            spawn_task_scheduler(app.handle().clone(), task_store);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_recent_events,
            commands::get_processes,
            commands::hardware_fingerprint,
            commands::install_license,
            commands::license_status,
            commands::scan_code_heatmap,
            commands::llm_is_reachable,
            commands::explain_event_with_llm,
            commands::embed_text,
            commands::add_event_to_vector_store,
            commands::vector_search,
            commands::get_config,
            commands::save_config,
            commands::scan_processes,
            commands::upsert_process_to_store,
            commands::list_scheduled_tasks,
            commands::create_scheduled_task,
            commands::delete_scheduled_task,
            commands::toggle_scheduled_task,
            commands::run_task_now,
            commands::mark_task_completed,
            commands::restart_log_sources,
        ])
        .run(tauri::generate_context!())
        .expect("error while running amber-shield-lite");
}

/// Spawns whatever `core_engine::logwatch::default_sources` returns for
/// this platform (Stripe CLI if installed, the Linux file-tail source for
/// any configured paths, etc.) and forwards their output to the HUD.
fn spawn_log_sources(
    app: tauri::AppHandle,
    hot_store: Arc<HotStore>,
    cold_store: Arc<std::sync::Mutex<SqliteColdStore>>,
    vector_store: Arc<std::sync::Mutex<core_engine::vectorized::MemoryVectorStore>>,
    watch_paths: Vec<std::path::PathBuf>,
) {
    let sources = core_engine::logwatch::default_sources(watch_paths);
    tracing::info!("spawning {} log sources", sources.len());

    for source in sources {
        let app = app.clone();
        let hot_store = hot_store.clone();
        let cold_store = cold_store.clone();
        let vector_store = vector_store.clone();
        tauri::async_runtime::spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(256);
            let name = source.name();

            let forward = tauri::async_runtime::spawn(async move {
                while let Some(event) = rx.recv().await {
                    emit_and_persist(&app, &hot_store, &cold_store, &vector_store, event);
                }
            });

            if let Err(e) = source.run(tx).await {
                tracing::warn!("log source {name} exited: {e}");
            }
            forward.abort();
        });
    }
}

fn emit_and_persist(
    app: &tauri::AppHandle,
    hot_store: &Arc<HotStore>,
    cold_store: &Arc<std::sync::Mutex<SqliteColdStore>>,
    vector_store: &Arc<std::sync::Mutex<core_engine::vectorized::MemoryVectorStore>>,
    event: Event,
) {
    let _ = hot_store.push_event(&event);
    if let Ok(store) = cold_store.lock() {
        use core_engine::storage::ColdStore;
        let _ = store.insert_event(&event);
    }
    if let Ok(mut vs) = vector_store.lock() {
        if let Ok(ve) = core_engine::vectorized::VectorEvent::from_event(&event) {
            vs.insert(ve);
        }
    }
    let _ = app.emit(EVENT_CHANNEL, &event);
}

/// Background loop that checks for due scheduled tasks every 10 seconds
/// and executes them.
fn spawn_task_scheduler(
    app: tauri::AppHandle,
    task_store: Arc<std::sync::Mutex<core_engine::scheduler::TaskStore>>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;

            let due: Vec<String> = {
                match task_store.lock() {
                    Ok(store) => store.due_tasks().into_iter().map(|t| t.id.clone()).collect(),
                    Err(_) => continue,
                }
            };

            for task_id in due {
                let task = {
                    match task_store.lock() {
                        Ok(store) => store.get(&task_id).cloned(),
                        Err(_) => continue,
                    }
                };
                let Some(task) = task else { continue };

                let app = app.clone();
                let task_store = task_store.clone();
                let task_id = task_id.clone();

                tauri::async_runtime::spawn(async move {
                    let result = execute_task(&app, &task).await;
                    match result {
                        Ok(msg) => {
                            tracing::info!("scheduled task '{}' completed: {msg}", task.name);
                        }
                        Err(e) => {
                            tracing::warn!("scheduled task '{}' failed: {e}", task.name);
                        }
                    }
                    if let Ok(mut store) = task_store.lock() {
                        store.mark_completed(&task_id);
                    }
                });
            }
        }
    });
}

async fn execute_task(
    app: &tauri::AppHandle,
    task: &core_engine::scheduler::ScheduledTask,
) -> Result<String, String> {
    use core_engine::scheduler::TaskKind;
    match &task.kind {
        TaskKind::ProcessCheck => {
            let procs = commands::scan_processes()?;
            // Persist processes to hot store for the behavior graph
            if let Some(state) = app.try_state::<AppState>() {
                for p in &procs {
                    use core_engine::storage::hot::ProcessNode;
                    let node = ProcessNode {
                        pid: p.pid,
                        parent_pid: None,
                        name: p.name.clone(),
                        exe_path: p.exe_path.clone(),
                        started_at: chrono::Utc::now(),
                        flagged: false,
                    };
                    let _ = state.hot_store.upsert_process(&node);
                }
            }
            Ok(format!("scanned and persisted {} processes", procs.len()))
        }
        TaskKind::CodeScan => {
            let path = task.target.as_deref().unwrap_or(".");
            let pb = std::path::PathBuf::from(path);
            if !pb.exists() || !pb.is_dir() {
                return Err(format!("target path does not exist: {path}"));
            }
            let root = pb.canonicalize().map_err(|e| e.to_string())?;
            let vulns = core_engine::security::run_trivy(&root).unwrap_or_default();
            let secrets = core_engine::security::run_gitleaks(&root).unwrap_or_default();
            let heatmap = core_engine::codemap::build_heatmap(&root, &vulns, &secrets)
                .map_err(|e| e.to_string())?;
            Ok(format!("heatmap: {} files, {} vulns, {} secrets",
                heatmap.len(), vulns.len(), secrets.len()))
        }
        TaskKind::LogCollect => {
            // Log sources run continuously in background; this task is a
            // health-check that reports how many events have been collected.
            if let Some(state) = app.try_state::<AppState>() {
                let count = state.hot_store.recent_events(500).map(|e| e.len()).unwrap_or(0);
                Ok(format!("log collectors running, {} events in ring buffer", count))
            } else {
                Ok("log collectors running (state unavailable)".into())
            }
        }
        TaskKind::VectorIndex => {
            // Events are auto-indexed on arrival; this reports the store size.
            if let Some(state) = app.try_state::<AppState>() {
                let count = state.vector_store.lock().map(|vs| vs.len()).unwrap_or(0);
                Ok(format!("vector store contains {} indexed events", count))
            } else {
                Ok("vector index running (state unavailable)".into())
            }
        }
    }
}
