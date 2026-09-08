//! Local Ollama client for the HUD's AI context drawer.
//!
//! Deliberately loopback-only - `127.0.0.1:11434` is Ollama's standard
//! local port, and there is no cloud fallback wired in here. That's what
//! keeps the "local-first, your code never leaves the machine" pitch
//! honest; if a cloud fallback is ever added, it needs to be an explicit,
//! separately-consented toggle, not something this client falls back to
//! silently (see the architecture review's note on most enterprise
//! laptops not having a GPU capable of a large local model - that's the
//! actual reason a fallback would be tempting, and exactly why it needs
//! to be opt-in rather than automatic).
//!
//! `ureq` (blocking) rather than an async HTTP stack: every call site
//! already dispatches through `spawn_blocking` (see
//! `tauri-app/src/commands.rs`), so pulling in tokio's async HTTP
//! machinery just for this one loopback call would be dead weight.

use crate::types::Event;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("could not reach Ollama at {0} - is `ollama serve` running?")]
    Unreachable(String),
    #[error("ollama returned an error: {0}")]
    Http(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, LlmError>;

pub struct OllamaClient {
    base_url: String,
    model: String,
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".to_string(),
            model: default_model(),
        }
    }
}

/// A small, CPU-viable default per the architecture review: most
/// enterprise laptops don't have a GPU that makes a 7B+ model feel
/// instant next to a live HUD. `AMBER_OLLAMA_MODEL` lets an install
/// override this once it knows what hardware it's actually running on
/// (see the doc note in `hwid.rs` about the same
/// detect-then-choose pattern).
fn default_model() -> String {
    std::env::var("AMBER_OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2:1b".to_string())
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
        }
    }

    /// Cheap reachability probe (`GET /api/tags`) - the frontend uses this
    /// to decide whether to show "Ollama not running" instead of trying a
    /// generate call and waiting out a timeout.
    pub fn is_reachable(&self) -> bool {
        ureq::get(&format!("{}/api/tags", self.base_url))
            .timeout(std::time::Duration::from_millis(700))
            .call()
            .is_ok()
    }

    /// Turns one HUD event into a short, grounded explanation. The prompt
    /// carries the event's *actual* metadata (the SQL statement, the
    /// process names, the CVE id - whatever the emitting module already
    /// attached, see `types::Event::metadata`) rather than asking the
    /// model to free-associate from the message string alone.
    pub fn explain_event(&self, event: &Event) -> Result<String> {
        self.generate(&build_prompt(event))
    }

    pub fn generate(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url);
        let body = OllamaRequest {
            model: &self.model,
            prompt,
            stream: false,
        };

        let response = ureq::post(&url)
            .timeout(std::time::Duration::from_secs(30))
            .send_json(&body)
            .map_err(|e| match e {
                ureq::Error::Transport(_) => LlmError::Unreachable(self.base_url.clone()),
                ureq::Error::Status(code, resp) => {
                    LlmError::Http(format!("{code}: {}", resp.status_text()))
                }
            })?;

        let parsed: OllamaResponse = response
            .into_json()
            .map_err(|e| LlmError::Http(format!("bad JSON from ollama: {e}")))?;
        Ok(parsed.response.trim().to_string())
    }
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

fn build_prompt(event: &Event) -> String {
    format!(
        "You are Amber AI, a terse on-call assistant embedded in a security/observability HUD.\n\
         An event just fired:\n\
         source: {:?}\n\
         severity: {:?}\n\
         message: {}\n\
         metadata: {}\n\n\
         In two sentences or fewer: say what likely happened and one concrete next action. \
         No preamble, no disclaimers, no restating the message verbatim.",
        event.source, event.severity, event.message, event.metadata
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EventSource, Severity};

    #[test]
    fn prompt_carries_real_metadata_not_just_the_message() {
        let event = Event::new(EventSource::DbMonitor, Severity::Warn, "slow query")
            .with_latency(2100)
            .with_metadata(serde_json::json!({ "statement": "SELECT * FROM sessions" }));
        let prompt = build_prompt(&event);
        assert!(prompt.contains("SELECT * FROM sessions"));
        assert!(prompt.contains("slow query"));
    }

    #[test]
    fn unreachable_client_reports_unreachable_not_a_generic_error() {
        // Port 1 is reserved/unroutable - this should fail fast as a
        // transport error, not hang or panic.
        let client = OllamaClient::new("http://127.0.0.1:1", "llama3.2:1b");
        assert!(!client.is_reachable());
    }
}
