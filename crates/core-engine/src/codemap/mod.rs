//! The vulnerability heatmap wedge: walks a project's source files, scores
//! each one, and cross-references Trivy/GitLeaks findings so the HUD can
//! render the "neural network of files, yellow/red/green" visualization
//! from the spec.
//!
//! Two languages are wired up (Rust, JavaScript/TypeScript) to keep this
//! scaffold's compile time and dependency count sane; adding another
//! language is one `tree-sitter-<lang>` dependency plus one match arm in
//! [`grammar_for`].
//!
//! The score itself is intentionally simple - a proxy for "this file is
//! worth a human's attention," not a rigorous complexity metric:
//! - +1 per branching construct (if/match/for/while/loop) as a cheap
//!   cyclomatic-complexity stand-in
//! - +2 per TODO/FIXME/XXX/HACK comment
//! - +10 per Trivy/GitLeaks finding attributed to the file
//! Tune the weights once you have real usage data; don't over-index on
//! these numbers before then.

use crate::security::{SecretFinding, Vulnerability};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Parser};

const MAX_SOURCE_FILES: usize = 10_000;
const MAX_SOURCE_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SCAN_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeatCategory {
    Stable,
    Stale,
    Vulnerable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHeat {
    pub path: String,
    pub score: u32,
    pub category: HeatCategory,
    pub branch_points: u32,
    pub todo_markers: u32,
    pub scanner_findings: u32,
}

fn grammar_for(path: &Path) -> Option<Language> {
    match path.extension().and_then(|e| e.to_str())? {
        "rs" => Some(tree_sitter_rust::language()),
        "js" | "jsx" | "mjs" | "ts" | "tsx" => Some(tree_sitter_javascript::language()),
        _ => None,
    }
}

/// Kinds counted as "branching" across the two grammars wired up here.
/// tree-sitter node kind names differ per-grammar, hence the small list
/// rather than a single shared constant.
const BRANCH_KINDS: &[&str] = &[
    "if_expression",
    "match_expression",
    "for_expression",
    "while_expression",
    "loop_expression",
    "if_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "switch_statement",
    "ternary_expression",
];

fn count_branch_points(source: &str, language: Language) -> anyhow::Result<u32> {
    let mut parser = Parser::new();
    parser.set_language(&language)?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("tree-sitter failed to parse source"))?;

    let mut count = 0u32;
    let mut cursor = tree.walk();
    let mut visited_children = false;
    loop {
        if !visited_children && BRANCH_KINDS.contains(&cursor.node().kind()) {
            count += 1;
        }
        if !visited_children && cursor.goto_first_child() {
            continue;
        }
        visited_children = false;
        if cursor.goto_next_sibling() {
            continue;
        }
        if !cursor.goto_parent() {
            break;
        }
        visited_children = true;
    }
    Ok(count)
}

fn count_todo_markers(source: &str) -> u32 {
    let markers = ["TODO", "FIXME", "XXX", "HACK"];
    source
        .lines()
        .filter(|line| markers.iter().any(|m| line.contains(m)))
        .count() as u32
}

/// Scores one file. `findings_for_file` lets the caller pre-filter
/// Trivy/GitLeaks results down to the ones whose `target`/`file` field
/// matches this path (matching is left to the caller since Trivy targets
/// can be relative to different roots depending on how it was invoked).
pub fn score_file(path: &Path, findings_for_file: u32) -> anyhow::Result<FileHeat> {
    let source = std::fs::read_to_string(path)?;

    let branch_points = match grammar_for(path) {
        Some(lang) => count_branch_points(&source, lang).unwrap_or(0),
        None => 0,
    };
    let todo_markers = count_todo_markers(&source);

    let score = branch_points + todo_markers * 2 + findings_for_file * 10;
    let category = if findings_for_file > 0 {
        HeatCategory::Vulnerable
    } else if todo_markers > 0 || branch_points > 15 {
        HeatCategory::Stale
    } else {
        HeatCategory::Stable
    };

    Ok(FileHeat {
        path: path.display().to_string(),
        score,
        category,
        branch_points,
        todo_markers,
        scanner_findings: findings_for_file,
    })
}

/// Walks `root` scoring every recognized source file, folding in scanner
/// findings so files Trivy/GitLeaks flagged come back red regardless of
/// their own branch/TODO count.
pub fn build_heatmap(
    root: &Path,
    vulnerabilities: &[Vulnerability],
    secrets: &[SecretFinding],
) -> anyhow::Result<Vec<FileHeat>> {
    let mut findings_by_file: HashMap<PathBuf, u32> = HashMap::new();
    for v in vulnerabilities {
        *findings_by_file
            .entry(PathBuf::from(&v.target))
            .or_default() += 1;
    }
    for s in secrets {
        *findings_by_file.entry(PathBuf::from(&s.file)).or_default() += 1;
    }

    let mut heatmap = Vec::new();
    for entry in walk_source_files(root)? {
        let findings = findings_by_file
            .iter()
            .find(|(k, _)| entry.ends_with(k) || k.ends_with(&entry))
            .map(|(_, v)| *v)
            .unwrap_or(0);
        if let Ok(heat) = score_file(&entry, findings) {
            heatmap.push(heat);
        }
    }
    heatmap.sort_by(|a, b| b.score.cmp(&a.score));
    Ok(heatmap)
}

fn walk_source_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_SCAN_DEPTH {
            anyhow::bail!("scan directory depth exceeds {MAX_SCAN_DEPTH}");
        }
        let entries = std::fs::read_dir(&dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if file_type.is_dir() {
                stack.push((path, depth + 1));
            } else if file_type.is_file() && grammar_for(&path).is_some() {
                if entry.metadata()?.len() > MAX_SOURCE_FILE_BYTES {
                    continue;
                }
                out.push(path);
                if out.len() > MAX_SOURCE_FILES {
                    anyhow::bail!("scan contains more than {MAX_SOURCE_FILES} source files");
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn scores_branch_points_and_todos() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("sample.rs");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            "fn f(x: i32) -> i32 {{\n    // TODO: handle negatives\n    if x > 0 {{ x }} else {{ -x }}\n}}"
        )
        .unwrap();
        drop(f);

        let heat = score_file(&file_path, 0).unwrap();
        assert_eq!(heat.branch_points, 1);
        assert_eq!(heat.todo_markers, 1);
        assert_eq!(heat.category, HeatCategory::Stale);
    }

    #[test]
    fn scanner_findings_force_vulnerable_category() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("clean.rs");
        std::fs::write(&file_path, "fn f() -> i32 { 1 }").unwrap();

        let heat = score_file(&file_path, 2).unwrap();
        assert_eq!(heat.category, HeatCategory::Vulnerable);
        assert!(heat.score >= 20);
    }
}
