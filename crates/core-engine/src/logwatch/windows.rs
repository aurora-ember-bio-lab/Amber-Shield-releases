//! Windows adapter: the equivalent of "tail a log file" is subscribing to
//! the Event Log (or ETW for higher-frequency telemetry), not reading a
//! flat file. A real implementation would use the `windows` crate's
//! `Win32::System::EventLog` bindings (or the `win_etw_provider` /
//! `ferrisetw` crates for ETW); this scaffold documents the shape via a
//! `wevtutil query-events` shell-out, which is enough to prove the trait
//! boundary without pulling in the full Win32 binding surface for a crate
//! that can't be compiled or tested from this Linux sandbox anyway.
//! `#[cfg(target_os = "windows")]` in `mod.rs` keeps it out of non-Windows
//! builds entirely.

use super::LogSource;
use crate::types::{Event, EventSource, Severity};
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

#[derive(Default)]
pub struct WindowsEventLogSource {
    /// e.g. "Application", "System".
    pub channel: Option<String>,
}

#[async_trait]
impl LogSource for WindowsEventLogSource {
    fn name(&self) -> String {
        "windows:event-log".to_string()
    }

    async fn run(&self, tx: mpsc::Sender<Event>) -> anyhow::Result<()> {
        let channel = self.channel.clone().unwrap_or_else(|| "Application".into());

        let mut cmd = Command::new("wevtutil");
        cmd.args(["qe", &channel, "/f:text", "/rd:true"]);
        cmd.stdout(std::process::Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let mut lines = BufReader::new(stdout).lines();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let lower = line.to_ascii_lowercase();
            let severity = if lower.contains("error") || lower.contains("critical") {
                Severity::Critical
            } else if lower.contains("warning") {
                Severity::Warn
            } else {
                Severity::Info
            };
            let event = Event::new(EventSource::LogWatch, severity, line)
                .with_metadata(serde_json::json!({ "channel": channel }));
            if tx.send(event).await.is_err() {
                break;
            }
        }

        Ok(())
    }
}
