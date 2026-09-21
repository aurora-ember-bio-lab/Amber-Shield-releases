//! Scheduled task engine — recurring background jobs with persistence.
//!
//! Each [`ScheduledTask`] has a [`TaskKind`] and either an interval (simple)
//! or a cron expression (advanced). The task definitions live in a JSON file
//! next to the other persisted state; the actual execution is driven by the
//! Tauri app layer (this crate only defines *what* to run, not *how* to
//! schedule it).
//!
//! Cron expressions use standard 6-field syntax: `second minute hour day month dow`.
//! Examples:
//! - `0 */5 * * * *` — every 5 minutes
//! - `0 0 9 * * 1-5` — weekdays at 09:00
//! - `0 30 2 * * 0` — Sundays at 02:30
//! - `0 0 */2 * * *` — every 2 hours

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::str::FromStr;

/// The kind of work a scheduled task performs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Run the code-vulnerability heatmap scan on a directory.
    CodeScan,
    /// Snapshot running processes and flag anomalies.
    ProcessCheck,
    /// Run the log-event collectors for a fresh batch.
    LogCollect,
    /// Embed recent events into the vector store.
    VectorIndex,
}

impl TaskKind {
    pub fn label(&self) -> &'static str {
        match self {
            TaskKind::CodeScan => "Code Scan",
            TaskKind::ProcessCheck => "Process Check",
            TaskKind::LogCollect => "Log Collect",
            TaskKind::VectorIndex => "Vector Index",
        }
    }
}

/// A single scheduled task definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub kind: TaskKind,
    /// Interval in seconds between runs (used when `cron_expr` is None).
    pub interval_secs: u64,
    /// Optional cron expression (5-field: minute hour day month dow).
    /// When set, takes precedence over `interval_secs`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_expr: Option<String>,
    pub enabled: bool,
    /// Optional: path / target the task operates on (e.g. a directory for CodeScan).
    pub target: Option<String>,
    pub last_run: Option<chrono::DateTime<chrono::Utc>>,
    pub next_run: Option<chrono::DateTime<chrono::Utc>>,
}

/// Compute the next run time from a cron expression.
/// Uses 6-field syntax: second minute hour day month dow.
fn cron_next_after(expr: &str, after: chrono::DateTime<chrono::Utc>) -> Option<chrono::DateTime<chrono::Utc>> {
    let schedule = cron::Schedule::from_str(expr).ok()?;
    schedule.after(&after).next()
}

/// Compute the next run time for a task (cron or interval).
pub fn compute_next_run(task: &ScheduledTask, after: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    if let Some(ref expr) = task.cron_expr {
        cron_next_after(expr, after)
            .unwrap_or_else(|| after + chrono::Duration::seconds(task.interval_secs as i64))
    } else {
        after + chrono::Duration::seconds(task.interval_secs as i64)
    }
}

/// Human-readable description of the schedule.
pub fn describe_schedule(task: &ScheduledTask) -> String {
    if let Some(ref expr) = task.cron_expr {
        format!("cron: {expr}")
    } else {
        let secs = task.interval_secs;
        if secs >= 86400 {
            format!("every {}d", secs / 86400)
        } else if secs >= 3600 {
            format!("every {}h", secs / 3600)
        } else if secs >= 60 {
            format!("every {}m", secs / 60)
        } else {
            format!("every {secs}s")
        }
    }
}

impl ScheduledTask {
    pub fn new(
        name: impl Into<String>,
        kind: TaskKind,
        interval_secs: u64,
        target: Option<String>,
    ) -> Self {
        let now = chrono::Utc::now();
        let dummy = Self {
            id: String::new(),
            name: String::new(),
            kind: kind.clone(),
            interval_secs,
            cron_expr: None,
            enabled: true,
            target: None,
            last_run: None,
            next_run: None,
        };
        let next = compute_next_run(&dummy, now);
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            kind,
            interval_secs,
            cron_expr: None,
            enabled: true,
            target,
            last_run: None,
            next_run: Some(next),
        }
    }

    /// Create a task with a cron expression instead of a simple interval.
    pub fn with_cron(
        name: impl Into<String>,
        kind: TaskKind,
        cron_expr: impl Into<String>,
        target: Option<String>,
    ) -> Result<Self, String> {
        let expr = cron_expr.into();
        let now = chrono::Utc::now();
        let next = cron_next_after(&expr, now)
            .ok_or_else(|| format!("invalid cron expression: {expr}"))?;
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            kind,
            interval_secs: 60,
            cron_expr: Some(expr),
            enabled: true,
            target,
            last_run: None,
            next_run: Some(next),
        })
    }

    /// Recompute `next_run` after a successful execution.
    pub fn mark_completed(&mut self) {
        let now = chrono::Utc::now();
        self.last_run = Some(now);
        self.next_run = Some(compute_next_run(self, now));
    }
}

/// Persistent store for scheduled tasks (JSON file).
pub struct TaskStore {
    path: std::path::PathBuf,
    tasks: Vec<ScheduledTask>,
}

impl TaskStore {
    pub fn open(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let tasks = std::fs::read(&path)
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default();
        Self { path, tasks }
    }

    pub fn list(&self) -> &[ScheduledTask] {
        &self.tasks
    }

    pub fn get(&self, id: &str) -> Option<&ScheduledTask> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ScheduledTask> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn add(&mut self, task: ScheduledTask) {
        self.tasks.push(task);
        self.persist();
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let len_before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        let removed = self.tasks.len() < len_before;
        if removed {
            self.persist();
        }
        removed
    }

    pub fn toggle(&mut self, id: &str, enabled: bool) -> bool {
        if let Some(task) = self.get_mut(id) {
            task.enabled = enabled;
            self.persist();
            true
        } else {
            false
        }
    }

    pub fn mark_completed(&mut self, id: &str) -> bool {
        if let Some(task) = self.get_mut(id) {
            task.mark_completed();
            self.persist();
            true
        } else {
            false
        }
    }

    /// Return all enabled tasks whose `next_run` is due (<= now).
    pub fn due_tasks(&self) -> Vec<&ScheduledTask> {
        let now = chrono::Utc::now();
        self.tasks
            .iter()
            .filter(|t| t.enabled && t.next_run.map_or(false, |nr| nr <= now))
            .collect()
    }

    fn persist(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.tasks) {
            let _ = std::fs::write(&self.path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_has_future_next_run() {
        let task = ScheduledTask::new("test", TaskKind::CodeScan, 300, None);
        assert!(task.next_run.is_some());
        assert!(task.enabled);
        assert!(task.last_run.is_none());
        assert!(task.cron_expr.is_none());
    }

    #[test]
    fn mark_completed_updates_times() {
        let mut task = ScheduledTask::new("test", TaskKind::ProcessCheck, 60, None);
        task.mark_completed();
        assert!(task.last_run.is_some());
        assert!(task.next_run.is_some());
    }

    #[test]
    fn cron_task_computes_next_run() {
        let task = ScheduledTask::with_cron(
            "every-5m",
            TaskKind::CodeScan,
            "0 */5 * * * *",
            None,
        ).unwrap();
        assert!(task.cron_expr.is_some());
        assert!(task.next_run.is_some());
        let next = task.next_run.unwrap();
        let now = chrono::Utc::now();
        // Next run should be within the next 5 minutes
        let diff = (next - now).num_seconds().abs();
        assert!(diff <= 300, "next run should be within 5 minutes, diff={diff}s");
    }

    #[test]
    fn cron_task_mark_completed_uses_cron() {
        let mut task = ScheduledTask::with_cron(
            "daily-9am",
            TaskKind::ProcessCheck,
            "0 0 9 * * *",
            None,
        ).unwrap();
        task.mark_completed();
        assert!(task.last_run.is_some());
        assert!(task.next_run.is_some());
    }

    #[test]
    fn invalid_cron_returns_error() {
        let result = ScheduledTask::with_cron(
            "bad",
            TaskKind::CodeScan,
            "not a cron expr",
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn describe_schedule_shows_correct_format() {
        let interval_task = ScheduledTask::new("s", TaskKind::CodeScan, 7200, None);
        assert_eq!(describe_schedule(&interval_task), "every 2h");

        let cron_task = ScheduledTask::with_cron("c", TaskKind::CodeScan, "0 */5 * * * *", None).unwrap();
        assert_eq!(describe_schedule(&cron_task), "cron: 0 */5 * * * *");
    }

    #[test]
    fn task_store_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        {
            let mut store = TaskStore::open(&path);
            store.add(ScheduledTask::new("scan", TaskKind::CodeScan, 300, Some("/src".into())));
            assert_eq!(store.list().len(), 1);
        }
        let store2 = TaskStore::open(&path);
        assert_eq!(store2.list().len(), 1);
        assert_eq!(store2.list()[0].name, "scan");
    }

    #[test]
    fn cron_task_store_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        {
            let mut store = TaskStore::open(&path);
            let task = ScheduledTask::with_cron(
                "cron-scan", TaskKind::CodeScan, "0 0 9 * * 1-5", Some("/src".into()),
            ).unwrap();
            store.add(task);
        }
        let store2 = TaskStore::open(&path);
        assert_eq!(store2.list().len(), 1);
        assert_eq!(store2.list()[0].cron_expr.as_deref(), Some("0 0 9 * * 1-5"));
    }

    #[test]
    fn due_tasks_returns_only_overdue() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = TaskStore::open(&path);

        // Task due in the past
        let mut past = ScheduledTask::new("past", TaskKind::CodeScan, 10, None);
        past.next_run = Some(chrono::Utc::now() - chrono::Duration::seconds(30));
        store.add(past);

        // Task due in the future
        let mut future = ScheduledTask::new("future", TaskKind::CodeScan, 10, None);
        future.next_run = Some(chrono::Utc::now() + chrono::Duration::hours(1));
        store.add(future);

        // Disabled task due in the past
        let mut disabled = ScheduledTask::new("disabled", TaskKind::CodeScan, 10, None);
        disabled.next_run = Some(chrono::Utc::now() - chrono::Duration::seconds(30));
        disabled.enabled = false;
        store.add(disabled);

        let due = store.due_tasks();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "past");
    }

    #[test]
    fn remove_task() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = TaskStore::open(&path);
        let task = ScheduledTask::new("remove-me", TaskKind::LogCollect, 120, None);
        let id = task.id.clone();
        store.add(task);
        assert!(store.remove(&id));
        assert!(store.list().is_empty());
        assert!(!store.remove(&id));
    }
}
