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
}

const EVENT_CHANNEL: &str = "amber://event";

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

            app.manage(AppState {
                hot_store: hot_store.clone(),
                cold_store: cold_store.clone(),
            });

            spawn_log_sources(app.handle().clone(), hot_store, cold_store);

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running amber-shield-lite");
}

/// Spawns whatever `core_engine::logwatch::default_sources` returns for
/// this platform (Stripe CLI if installed, the Linux file-tail source for
/// any configured paths, etc.) and forwards their output to the HUD.
///
/// No paths are configured by default in this scaffold - wiring up a
/// settings screen to let the user pick which logs to watch is the natural
/// next step, not something to hardcode here.
fn spawn_log_sources(
    app: tauri::AppHandle,
    hot_store: Arc<HotStore>,
    cold_store: Arc<std::sync::Mutex<SqliteColdStore>>,
) {
    let watch_paths: Vec<std::path::PathBuf> = Vec::new();
    let sources = core_engine::logwatch::default_sources(watch_paths);

    for source in sources {
        let app = app.clone();
        let hot_store = hot_store.clone();
        let cold_store = cold_store.clone();
        tauri::async_runtime::spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(256);
            let name = source.name();

            let forward = tauri::async_runtime::spawn(async move {
                while let Some(event) = rx.recv().await {
                    emit_and_persist(&app, &hot_store, &cold_store, event);
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
    event: Event,
) {
    let _ = hot_store.push_event(&event);
    if let Ok(store) = cold_store.lock() {
        use core_engine::storage::ColdStore;
        let _ = store.insert_event(&event);
    }
    let _ = app.emit(EVENT_CHANNEL, &event);
}
