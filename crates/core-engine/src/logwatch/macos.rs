//! macOS adapter: unified logging has no plain files to tail, so this
//! shells out to `log stream --style ndjson` with a predicate and parses
//! its NDJSON output. This file only compiles on macOS
//! (`#[cfg(target_os = "macos")]` in `mod.rs`); it's included in the
//! scaffold so the trait boundary is exercised by more than one platform,
//! even though it can't be built or tested from this Linux sandbox.

use super::LogSource;
use crate::types::{Event, EventSource, Severity};
use async_trait::async_trait;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

#[derive(Default)]
pub struct MacUnifiedLogSource {
    /// e.g. `"process == \"nginx\""`; empty means "everything", which is
    /// noisy - callers should scope this.
    pub predicate: Option<String>,
}

#[derive(Deserialize)]
struct NdjsonLine {
    #[serde(rename = "eventMessage")]
    message: Option<String>,
    #[serde(rename = "messageType")]
    message_type: Option<String>,
}

#[async_trait]
impl LogSource for MacUnifiedLogSource {
    fn name(&self) -> String {
        "macos:unified-log".to_string()
    }

    async fn run(&self, tx: mpsc::Sender<Event>) -> anyhow::Result<()> {
        let mut cmd = Command::new("log");
        cmd.args(["stream", "--style", "ndjson"]);
        if let Some(pred) = &self.predicate {
            cmd.args(["--predicate", pred]);
        }
        cmd.stdout(std::process::Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let mut lines = BufReader::new(stdout).lines();

        while let Some(line) = lines.next_line().await? {
            let Ok(parsed) = serde_json::from_str::<NdjsonLine>(&line) else {
                continue;
            };
            let Some(message) = parsed.message else {
                continue;
            };
            let severity = match parsed.message_type.as_deref() {
                Some("Fault") => Severity::Critical,
                Some("Error") => Severity::Warn,
                _ => Severity::Info,
            };
            let event = Event::new(EventSource::LogWatch, severity, message);
            if tx.send(event).await.is_err() {
                break;
            }
        }

        Ok(())
    }
}
