//! Amber Shield Lite - core engine.
//!
//! This crate is the framework-agnostic core described in the
//! architecture review: it knows nothing about Tauri, WebViews, or any
//! particular UI. `crates/tauri-app` is a thin shell on top of the public
//! API here. If the shell ever changes (a different desktop framework, a
//! CLI-only mode, a headless agent), this crate shouldn't need to change.
//!
//! This is the free/lite edition (MIT licensed): it ships the
//! code-vulnerability heatmap and the local AI explain-drawer. The
//! process-behavior monitor and database-query monitor are paid-tier
//! features that live only in the private Amber Shield source tree, not
//! in this repo.
//!
//! - [`codemap`] + [`security`]: the code-vulnerability heatmap (Trivy +
//!   GitLeaks + tree-sitter). The most differentiated, most buildable
//!   wedge purely from existing OSS parts - see the architecture review
//!   for why this is the recommended first slice to actually ship.
//! - [`logwatch`]: the cross-platform log/event interceptor, plus the
//!   Stripe CLI event source for local payment-event telemetry.
//!
//! Cutting across both: [`storage`] (hot/cold split), [`hwid`] (an
//! optional license check, kept here for forward compatibility with a
//! future paid upsell - the free build runs fully without a license),
//! [`llm`] (the local-Ollama client behind the HUD's AI context drawer),
//! and [`types::Event`] (the one shape every wedge emits).

pub mod codemap;
pub mod hwid;
pub mod llm;
pub mod logwatch;
pub mod scheduler;
pub mod security;
pub mod storage;
pub mod types;
pub mod vectorized;

pub use types::{Event, EventSource, Severity};
