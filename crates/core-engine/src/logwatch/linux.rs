//! Linux adapter: tail a plain log file (e.g. `/var/log/syslog`,
//! `/var/log/nginx/access.log`, or any app log the user points us at) and
//! turn new lines into [`Event`]s. This is the one platform where "just
//! tail a file" is actually representative of how logging works.

use super::LogSource;
use crate::types::{Event, EventSource, Severity};
use async_trait::async_trait;
use notify::{Event as NotifyEvent, RecursiveMode, Watcher};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub struct LinuxFileTailSource {
    path: PathBuf,
}

impl LinuxFileTailSource {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn classify(line: &str) -> Severity {
        let lower = line.to_ascii_lowercase();
        if lower.contains("panic") || lower.contains("fatal") || lower.contains("critical") {
            Severity::Critical
        } else if lower.contains("error") || lower.contains("warn") {
            Severity::Warn
        } else {
            Severity::Info
        }
    }
}

#[async_trait]
impl LogSource for LinuxFileTailSource {
    fn name(&self) -> String {
        format!("linux:{}", self.path.display())
    }

    async fn run(&self, tx: mpsc::Sender<Event>) -> anyhow::Result<()> {
        use std::fs::File;

        let path = self.path.clone();
        let (fs_tx, mut fs_rx) = mpsc::channel::<()>(16);

        // notify's callback runs on its own thread; bounce a "something
        // changed" signal onto a tokio channel so the async loop below can
        // react without blocking notify's thread.
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<NotifyEvent>| {
            if res.is_ok() {
                let _ = fs_tx.blocking_send(());
            }
        })?;
        watcher.watch(&path, RecursiveMode::NonRecursive)?;

        let mut file = File::open(&path)?;
        // Start at EOF: we only care about new activity, not replaying the
        // whole history on startup.
        let mut offset = file.seek(SeekFrom::End(0))?;

        loop {
            if fs_rx.recv().await.is_none() {
                break; // watcher dropped
            }

            let mut f = File::open(&path)?;
            let len = f.metadata()?.len();
            if len < offset {
                // File was truncated/rotated; start over from the top.
                offset = 0;
            }
            f.seek(SeekFrom::Start(offset))?;
            let mut buf = String::new();
            f.read_to_string(&mut buf)?;
            offset = f.stream_position()?;

            for line in buf.lines().filter(|l| !l.trim().is_empty()) {
                let event = Event::new(EventSource::LogWatch, Self::classify(line), line)
                    .with_metadata(serde_json::json!({ "path": path.display().to_string() }));
                if tx.send(event).await.is_err() {
                    return Ok(()); // receiver gone, shut down quietly
                }
            }
        }

        Ok(())
    }
}
