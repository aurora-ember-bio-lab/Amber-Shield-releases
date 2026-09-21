//! Shared event vocabulary. Every module in this crate — the log watcher,
//! the security scanners, the DB monitor, the process-behavior graph —
//! ultimately emits `Event` values. The HUD on the frontend only ever has
//! to understand this one shape, no matter which subsystem produced it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Maps to the doc's "Emerald" state: steady, healthy, 1Hz pulse.
    Info,
    /// Maps to "Electric Amber": degraded / slow, 3Hz jitter.
    Warn,
    /// Maps to "Crimson": broken / dropped, breathing fade pulse.
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    LogWatch,
    Security,
    CodeMap,
    /// Database query monitor (paid-tier feature, not in open-source edition).
    DbMonitor,
    /// Process behavior monitor (paid-tier feature, not in open-source edition).
    Behavior,
    /// License activation / deactivation events (paid-tier feature, not in open-source edition).
    License,
}

/// A single normalized fact for the HUD / tech-journal timeline.
///
/// `latency_ms` is optional and only meaningful for events that represent a
/// request/response or a query (HTTP calls, DB queries); the frontend state
/// machine described in the spec (green / amber / crimson by latency) reads
/// this field directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub source: EventSource,
    pub severity: Severity,
    pub message: String,
    pub latency_ms: Option<u64>,
    /// Free-form structured payload (parsed log line, scanner finding,
    /// process metadata, ...). Kept as JSON so each module can evolve its
    /// own schema without touching this struct.
    pub metadata: serde_json::Value,
}

impl Event {
    pub fn new(source: EventSource, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            source,
            severity,
            message: message.into(),
            latency_ms: None,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.latency_ms = Some(latency_ms);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Implements the doc's HUD state-machine mapping directly on the event
    /// so the frontend doesn't have to re-derive it: 200-class -> Info,
    /// >1500ms -> Warn, 5xx/dropped -> Critical.
    pub fn from_http_like(
        source: EventSource,
        status: u16,
        latency_ms: u64,
        message: impl Into<String>,
    ) -> Self {
        let severity = if status >= 500 {
            Severity::Critical
        } else if latency_ms > 1500 {
            Severity::Warn
        } else {
            Severity::Info
        };
        Self::new(source, severity, message).with_latency(latency_ms)
    }
}
