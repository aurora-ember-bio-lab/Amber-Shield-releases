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
