//! Shells out to well-established, permissively-licensed security
//! binaries (Trivy - Apache-2.0, GitLeaks - MIT) instead of reimplementing
//! CVE databases or secret-scanning heuristics. Both licenses allow
//! bundling into a closed commercial product; Apache-2.0 requires
//! preserving the NOTICE/attribution, which is tracked in
//! `THIRD_PARTY_NOTICES.md` at the workspace root.
//!
//! Operational note carried over from the architecture review: if you ship
//! these binaries bundled (rather than requiring the user to install them),
//! verify their checksums/signatures on every update. A tampered scanner
//! binary would silently defeat the entire point of this module.

pub mod gitleaks;
pub mod trivy;

pub use gitleaks::{run_gitleaks, SecretFinding};
pub use trivy::{run_trivy, Vulnerability};

#[derive(Debug, thiserror::Error)]
pub enum ScannerError {
    #[error("`{0}` was not found on PATH - install it or point {1} at the binary")]
    ToolNotFound(&'static str, &'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse scanner output as JSON: {0}")]
    Parse(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ScannerError>;

/// Finds a tool on PATH, or lets an env var override the location (useful
/// for bundling a pinned binary rather than trusting whatever's on PATH).
pub(crate) fn locate(bin_name: &'static str, env_override: &'static str) -> Result<std::path::PathBuf> {
    if let Ok(p) = std::env::var(env_override) {
        return Ok(std::path::PathBuf::from(p));
    }
    which::which(bin_name).map_err(|_| ScannerError::ToolNotFound(bin_name, env_override))
}
