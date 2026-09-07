//! The "Tech-Journal Pipe": a local event-stream interceptor.
//!
//! The original spec described this as "watch /var/log and system stdout
//! streams" as if that were one cross-platform thing. It isn't - macOS
//! moved to unified logging (no plain files; you query `log stream` with
//! predicates) and Windows exposes the Event Log via its own API, not flat
//! files. So instead of one generic file-tailer, this module defines one
//! trait, [`LogSource`], and a per-OS adapter behind it. That's also more
//! in the spirit of "framework-agnostic core" than the original doc's flat
//! description: the *interface* is what's stable, not the mechanism.

mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod stripe_cli;
#[cfg(target_os = "windows")]
mod windows;

pub use linux::LinuxFileTailSource;
pub use stripe_cli::StripeCliSource;

use crate::types::Event;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// One adapter per platform/data source. Implementations push normalized
/// [`Event`]s onto the provided channel; they don't know or care who's on
/// the other end (the hot store, a WebSocket, a Tauri event - whatever the
/// shell layer wires up).
#[async_trait]
pub trait LogSource: Send + Sync {
    /// Human-readable name for diagnostics/config, e.g. "linux:/var/log/syslog".
    fn name(&self) -> String;

    /// Runs until cancelled or the source closes. Implementations should be
    /// resilient to transient errors (a rotated log file, a process that
    /// exits and restarts) rather than returning on the first hiccup.
    async fn run(&self, tx: mpsc::Sender<Event>) -> anyhow::Result<()>;
}

/// Returns the default set of sources for the current platform. The Stripe
/// CLI source is included on every platform since it just shells out to a
/// binary on PATH, not to OS-specific log infrastructure.
pub fn default_sources(watch_paths: Vec<std::path::PathBuf>) -> Vec<Box<dyn LogSource>> {
    let mut sources: Vec<Box<dyn LogSource>> = Vec::new();

    #[cfg(target_os = "linux")]
    for path in &watch_paths {
        sources.push(Box::new(LinuxFileTailSource::new(path.clone())));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = &watch_paths;

    #[cfg(target_os = "macos")]
    sources.push(Box::new(macos::MacUnifiedLogSource::default()));

    #[cfg(target_os = "windows")]
    sources.push(Box::new(windows::WindowsEventLogSource::default()));

    if let Some(stripe) = StripeCliSource::if_available() {
        sources.push(Box::new(stripe));
    }

    sources
}
