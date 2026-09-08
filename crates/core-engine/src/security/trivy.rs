//! Wraps `trivy fs --format json <path>`. Trivy's JSON schema is large;
//! this only pulls the fields the heatmap actually needs rather than
//! modeling the whole schema, and ignores fields it doesn't recognize
//! (`serde(default)` + no `deny_unknown_fields`) so a Trivy version bump
//! doesn't break parsing.

use super::{locate, Result, ScannerError};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vulnerability {
    pub id: String,
    pub pkg_name: String,
    pub installed_version: String,
    pub fixed_version: Option<String>,
    pub severity: String,
    pub title: Option<String>,
    /// File the vulnerable dependency/config was found in, when Trivy
    /// reports one - this is what lets `codemap` cross-reference a finding
    /// back onto a specific file for the heatmap.
    pub target: String,
}

#[derive(Deserialize)]
struct TrivyReport {
    #[serde(default)]
    #[serde(rename = "Results")]
    results: Vec<TrivyResult>,
}

#[derive(Deserialize)]
struct TrivyResult {
    #[serde(default)]
    #[serde(rename = "Target")]
    target: String,
    #[serde(default)]
    #[serde(rename = "Vulnerabilities")]
    vulnerabilities: Vec<TrivyVuln>,
}

#[derive(Deserialize)]
struct TrivyVuln {
    #[serde(rename = "VulnerabilityID")]
    id: String,
    #[serde(rename = "PkgName")]
    pkg_name: String,
    #[serde(rename = "InstalledVersion")]
    installed_version: String,
    #[serde(rename = "FixedVersion")]
    fixed_version: Option<String>,
    #[serde(rename = "Severity")]
    severity: String,
    #[serde(rename = "Title")]
    title: Option<String>,
}

pub fn run_trivy(project_path: &Path) -> Result<Vec<Vulnerability>> {
    let bin = locate("trivy", "AMBER_TRIVY_BIN")?;

    let output = Command::new(bin)
        .args(["fs", "--format", "json", "--quiet"])
        .arg(project_path)
        .output()?;

    if !output.status.success() {
        return Err(ScannerError::Io(std::io::Error::other(format!(
            "trivy exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ))));
    }

    let report: TrivyReport = serde_json::from_slice(&output.stdout)?;

    let mut findings = Vec::new();
    for result in report.results {
        for v in result.vulnerabilities {
            findings.push(Vulnerability {
                id: v.id,
                pkg_name: v.pkg_name,
                installed_version: v.installed_version,
                fixed_version: v.fixed_version,
                severity: v.severity,
                title: v.title,
                target: result.target.clone(),
            });
        }
    }
    Ok(findings)
}
