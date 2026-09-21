//! Tauri command surface: the only place this crate calls into
//! `core-engine`. Every command here is a thin adapter - argument
//! marshalling and error-string conversion - with zero business logic of
//! its own, which is what keeps `core-engine` genuinely portable (see the
//! crate doc in `core-engine/src/lib.rs`).

use crate::AppState;
use core_engine::codemap::FileHeat;
use core_engine::llm::OllamaClient;
use core_engine::storage::hot::ProcessNode;
use core_engine::types::Event;
use core_engine::{hwid, security};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, serde::Serialize)]
pub struct LicenseStatus {
    pub active: bool,
    pub seat_id: Option<String>,
    pub expires_at_unix: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ScanResult {
    pub heatmap: Vec<FileHeat>,
    pub trivy_findings: usize,
    pub gitleaks_findings: usize,
    pub scanner_errors: Vec<String>,
}

fn to_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_recent_events(state: State<'_, AppState>, limit: usize) -> Result<Vec<Event>, String> {
    state.hot_store.recent_events(limit.min(500)).map_err(to_err)
}

#[tauri::command]
pub fn get_processes(state: State<'_, AppState>) -> Result<Vec<ProcessNode>, String> {
    state.hot_store.all_processes().map_err(to_err)
}

#[tauri::command]
pub fn hardware_fingerprint() -> Result<String, String> {
    hwid::fingerprint().map(|fp| fp.to_string()).map_err(to_err)
}

fn license_public_key() -> Result<[u8; 32], String> {
    let encoded = option_env!("AMBER_SHIELD_LICENSE_PUBLIC_KEY_HEX")
        .ok_or_else(|| "license public key is not configured in this build".to_string())?;
    hex::decode(encoded)
        .map_err(|_| "license public key is invalid".to_string())?
        .try_into()
        .map_err(|_| "license public key must be 32 bytes".to_string())
}

fn license_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("license.json"))
        .map_err(to_err)
}

#[tauri::command]
pub fn install_license(app: AppHandle, license_json: String) -> Result<LicenseStatus, String> {
    if license_json.len() > 16 * 1024 {
        return Err("license payload is too large".into());
    }
    hwid::parse_license_json(license_json.as_bytes()).map_err(to_err)?;
    let path = license_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(to_err)?;
    }
    std::fs::write(&path, license_json).map_err(to_err)?;
    license_status(app)
}

#[tauri::command]
pub fn license_status(app: AppHandle) -> Result<LicenseStatus, String> {
    let path = license_path(&app)?;
    let raw = match std::fs::read(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LicenseStatus { active: false, seat_id: None, expires_at_unix: None, error: Some("no license installed".into()) });
        }
        Err(error) => return Err(error.to_string()),
    };
    let license = hwid::parse_license_json(&raw).map_err(to_err)?;
    let fingerprint = hwid::fingerprint().map_err(to_err)?;
    let public_key = license_public_key()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(to_err)?
        .as_secs();
    hwid::verify_license(&public_key, &license, &fingerprint, now).map_err(to_err)?;
    Ok(LicenseStatus { active: true, seat_id: Some(license.claims.seat_id), expires_at_unix: Some(license.claims.expires_at_unix), error: None })
}

/// Runs Trivy + GitLeaks against `project_path` and folds the results into
/// a per-file heatmap. This is I/O- and CPU-bound (shells out to external
/// processes, parses source with tree-sitter), so it's dispatched with
/// `spawn_blocking` rather than run directly on the async command handler.
#[tauri::command]
pub async fn scan_code_heatmap(project_path: String) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let input = PathBuf::from(&project_path);
        if !input.exists() || !input.is_dir() {
            return Err("scan path must be an existing directory".to_string());
        }
        if std::fs::symlink_metadata(&input)
            .map_err(to_err)?
            .file_type()
            .is_symlink()
        {
            return Err("scan path must not be a symbolic link".into());
        }
        let root = input.canonicalize().map_err(to_err)?;

        let mut scanner_errors = Vec::new();
        let vulnerabilities = match security::run_trivy(&root) {
            Ok(findings) => findings,
            Err(error) => {
                scanner_errors.push(format!("trivy: {error}"));
                Vec::new()
            }
        };
        let secrets = match security::run_gitleaks(&root) {
            Ok(findings) => findings,
            Err(error) => {
                scanner_errors.push(format!("gitleaks: {error}"));
                Vec::new()
            }
        };

        let heatmap = core_engine::codemap::build_heatmap(&root, &vulnerabilities, &secrets)
            .map_err(|e| e.to_string())?;
        Ok(ScanResult {
            heatmap,
            trivy_findings: vulnerabilities.len(),
            gitleaks_findings: secrets.len(),
            scanner_errors,
        })
    })
    .await
    .map_err(to_err)?
}

/// Cheap probe the frontend calls before offering the AI drawer's "ask
/// Ollama" path, so it can show "Ollama not running" instead of a
/// generate call timing out on every click.
#[tauri::command]
pub async fn llm_is_reachable() -> bool {
    tauri::async_runtime::spawn_blocking(|| OllamaClient::default().is_reachable())
        .await
        .unwrap_or(false)
}

/// Asks the local Ollama instance to explain one HUD event. No cloud
/// fallback - see the module doc in `core_engine::llm` for why that's a
/// deliberate choice, not an oversight.
#[tauri::command]
pub async fn explain_event_with_llm(event: Event) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        OllamaClient::default().explain_event(&event).map_err(|e| e.to_string())
    })
    .await
    .map_err(to_err)?
}

/// Generate an embedding vector for a text string via Ollama.
#[tauri::command]
pub async fn embed_text(text: String) -> Result<Vec<f32>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        core_engine::vectorized::embed_text(&text).map_err(|e| e.to_string())
    })
    .await
    .map_err(to_err)?
}

/// Add an event to the in-memory vector store for semantic search.
#[tauri::command]
pub fn add_event_to_vector_store(
    state: State<'_, AppState>,
    event: Event,
) -> Result<(), String> {
    let ve = core_engine::vectorized::VectorEvent::from_event(&event)
        .map_err(|e| e.to_string())?;
    state.vector_store.lock().map_err(to_err)?.insert(ve);
    Ok(())
}

/// Semantic similarity search over the in-memory vector store.
/// Returns up to `limit` results, most similar first.
#[tauri::command]
pub async fn vector_search(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<Vec<core_engine::vectorized::VectorEvent>, String> {
    let store = state.vector_store.lock().map_err(to_err)?;
    let query_vec = core_engine::vectorized::embed_text(&query)
        .map_err(|e| e.to_string())?;
    let results = store.search(&query_vec, limit.min(50));
    Ok(results.into_iter().map(|(_, ve)| ve.clone()).collect())
}

// -----------------------------------------------------------------------
// Config: log watch paths
// -----------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppConfig {
    pub watch_paths: Vec<String>,
}

fn config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("config.json"))
        .map_err(to_err)
}

#[tauri::command]
pub fn get_config(app: AppHandle) -> Result<AppConfig, String> {
    let path = config_path(&app)?;
    match std::fs::read(&path) {
        Ok(raw) => serde_json::from_slice(&raw).map_err(to_err),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(AppConfig { watch_paths: Vec::new() })
        }
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub fn save_config(app: AppHandle, config: AppConfig) -> Result<(), String> {
    let path = config_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(to_err)?;
    }
    let json = serde_json::to_string_pretty(&config).map_err(to_err)?;
    std::fs::write(&path, json).map_err(to_err)?;
    Ok(())
}

// -----------------------------------------------------------------------
// Process behavior monitor (lite)
// -----------------------------------------------------------------------

#[derive(Debug, serde::Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub exe_path: Option<String>,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
}

#[tauri::command]
pub fn scan_processes() -> Result<Vec<ProcessInfo>, String> {
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_processes();

    Ok(sys
        .processes()
        .iter()
        .map(|(pid, proc_info)| ProcessInfo {
            pid: pid.as_u32(),
            name: proc_info.name().to_string(),
            exe_path: proc_info.exe().map(|p| p.to_string_lossy().into_owned()),
            cpu_usage: proc_info.cpu_usage(),
            memory_bytes: proc_info.memory(),
        })
        .collect())
}

#[tauri::command]
pub fn upsert_process_to_store(
    state: State<'_, AppState>,
    pid: u32,
    name: String,
    exe_path: Option<String>,
) -> Result<(), String> {
    use core_engine::storage::hot::ProcessNode;
    let node = ProcessNode {
        pid,
        parent_pid: None,
        name,
        exe_path,
        started_at: chrono::Utc::now(),
        flagged: false,
    };
    state.hot_store.upsert_process(&node).map_err(to_err)
}

// -----------------------------------------------------------------------
// Scheduled Tasks
// -----------------------------------------------------------------------

#[tauri::command]
pub fn list_scheduled_tasks(state: State<'_, AppState>) -> Result<Vec<core_engine::scheduler::ScheduledTask>, String> {
    Ok(state.task_store.lock().map_err(to_err)?.list().to_vec())
}

#[derive(serde::Deserialize)]
pub struct NewTaskRequest {
    pub name: String,
    pub kind: String,
    pub interval_secs: Option<u64>,
    pub cron_expr: Option<String>,
    pub target: Option<String>,
}

#[tauri::command]
pub fn create_scheduled_task(
    state: State<'_, AppState>,
    request: NewTaskRequest,
) -> Result<core_engine::scheduler::ScheduledTask, String> {
    use core_engine::scheduler::{ScheduledTask, TaskKind};
    let kind = match request.kind.as_str() {
        "code_scan" => TaskKind::CodeScan,
        "process_check" => TaskKind::ProcessCheck,
        "log_collect" => TaskKind::LogCollect,
        "vector_index" => TaskKind::VectorIndex,
        other => return Err(format!("unknown task kind: {other}")),
    };
    let task = if let Some(expr) = request.cron_expr {
        ScheduledTask::with_cron(request.name, kind, expr, request.target)?
    } else {
        let interval = request.interval_secs.unwrap_or(300).max(30);
        ScheduledTask::new(request.name, kind, interval, request.target)
    };
    let created = task.clone();
    state.task_store.lock().map_err(to_err)?.add(task);
    Ok(created)
}

#[tauri::command]
pub fn delete_scheduled_task(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let removed = state.task_store.lock().map_err(to_err)?.remove(&id);
    if removed { Ok(()) } else { Err("task not found".into()) }
}

#[tauri::command]
pub fn toggle_scheduled_task(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let ok = state.task_store.lock().map_err(to_err)?.toggle(&id, enabled);
    if ok { Ok(()) } else { Err("task not found".into()) }
}

#[tauri::command]
pub fn run_task_now(
    state: State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    use core_engine::scheduler::TaskKind;
    let task = state.task_store.lock().map_err(to_err)?
        .get(&id).cloned().ok_or("task not found")?;

    match task.kind {
        TaskKind::ProcessCheck => {
            let procs = scan_processes()?;
            // Persist processes to hot store
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
            Ok(format!("scanned and persisted {} processes", procs.len()))
        }
        TaskKind::CodeScan => {
            let path = task.target.as_deref().unwrap_or(".");
            let pb = std::path::PathBuf::from(path);
            if !pb.exists() || !pb.is_dir() {
                return Err(format!("target path does not exist: {path}"));
            }
            let root = pb.canonicalize().map_err(to_err)?;
            let vulns = security::run_trivy(&root).unwrap_or_default();
            let secrets = security::run_gitleaks(&root).unwrap_or_default();
            let heatmap = core_engine::codemap::build_heatmap(&root, &vulns, &secrets)
                .map_err(|e| e.to_string())?;
            Ok(format!("heatmap: {} files scored, {} vulns, {} secrets",
                heatmap.len(), vulns.len(), secrets.len()))
        }
        TaskKind::LogCollect => {
            let count = state.hot_store.recent_events(500).map(|e| e.len()).unwrap_or(0);
            Ok(format!("log collectors running, {} events in ring buffer", count))
        }
        TaskKind::VectorIndex => {
            let count = state.vector_store.lock().map(|vs| vs.len()).unwrap_or(0);
            Ok(format!("vector store contains {} indexed events", count))
        }
    }
}

#[tauri::command]
pub fn mark_task_completed(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let ok = state.task_store.lock().map_err(to_err)?.mark_completed(&id);
    if ok { Ok(()) } else { Err("task not found".into()) }
}

/// Read the saved config and return the watch paths so the frontend can
/// display them. A real restart would require respawning log sources,
/// which is handled by re-reading config on the next app launch.
#[tauri::command]
pub fn restart_log_sources(app: AppHandle) -> Result<Vec<String>, String> {
    let cfg = get_config(app)?;
    Ok(cfg.watch_paths)
}
