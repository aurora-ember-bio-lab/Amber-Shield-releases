//! Concrete implementation of the doc's "pipes output from the local Stripe
//! CLI daemon" bullet. `stripe listen --print-json` emits one JSON object
//! per webhook event on stdout, which maps cleanly onto our `Event` type -
//! this is the one piece of the original spec that was already
//! well-specified, so it's implemented for real rather than stubbed.

use super::LogSource;
use crate::types::{Event, EventSource, Severity};
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

pub struct StripeCliSource {
    /// Extra args, e.g. `["--events", "payment_intent.*"]`.
    extra_args: Vec<String>,
}

impl StripeCliSource {
    /// Only constructs a source if the `stripe` binary is actually on
    /// PATH - callers shouldn't have to know or care whether the user has
    /// it installed.
    pub fn if_available() -> Option<Self> {
        which::which("stripe").ok().map(|_| Self {
            extra_args: Vec::new(),
        })
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }
}

#[async_trait]
impl LogSource for StripeCliSource {
    fn name(&self) -> String {
        "stripe-cli:listen".to_string()
    }

    async fn run(&self, tx: mpsc::Sender<Event>) -> anyhow::Result<()> {
        let mut cmd = Command::new("stripe");
        cmd.arg("listen").arg("--print-json");
        cmd.args(&self.extra_args);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let mut lines = BufReader::new(stdout).lines();

        while let Some(line) = lines.next_line().await? {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(payload) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                continue;
            };

            let event_type = payload
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let severity = if event_type.ends_with(".failed") || event_type.contains("dispute") {
                Severity::Critical
            } else if event_type.contains("requires_action") {
                Severity::Warn
            } else {
                Severity::Info
            };

            let event = Event::new(EventSource::LogWatch, severity, format!("stripe: {event_type}"))
                .with_metadata(payload);
            if tx.send(event).await.is_err() {
                break;
            }
        }

        Ok(())
    }
}
