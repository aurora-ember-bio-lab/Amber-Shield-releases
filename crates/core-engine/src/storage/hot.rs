//! Live state: the process/behavior graph and a bounded ring buffer of the
//! most recent events, backed by `sled`. This is what the HUD polls (or
//! subscribes to) many times a second, so it deliberately never touches
//! disk-backed SQL.

use crate::types::Event;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum HotStoreError {
    #[error(transparent)]
    Sled(#[from] sled::Error),
    #[error(transparent)]
    Encode(#[from] bincode::Error),
}

pub type Result<T> = std::result::Result<T, HotStoreError>;

/// A node in the live process-behavior graph. See [`crate::behavior`] for
/// how these get populated and scored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessNode {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub exe_path: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Set by the behavior heuristics when a node looks like the source of
    /// a suspicious edge (see `behavior::flag_suspicious_spawn`).
    pub flagged: bool,
}

const MAX_RING_BUFFER: usize = 5_000;

pub struct HotStore {
    processes: sled::Tree,
    events: sled::Tree,
    db: sled::Db,
}

impl HotStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = sled::open(path)?;
        let processes = db.open_tree("processes")?;
        let events = db.open_tree("events")?;
        Ok(Self {
            processes,
            events,
            db,
        })
    }

    /// In-memory variant, handy for tests and for a first run before the
    /// user has picked a data directory.
    pub fn open_temporary() -> Result<Self> {
        let db = sled::Config::new().temporary(true).open()?;
        let processes = db.open_tree("processes")?;
        let events = db.open_tree("events")?;
        Ok(Self {
            processes,
            events,
            db,
        })
    }

    pub fn upsert_process(&self, node: &ProcessNode) -> Result<()> {
        let key = node.pid.to_be_bytes();
        let value = bincode::serialize(node)?;
        self.processes.insert(key, value)?;
        Ok(())
    }

    pub fn get_process(&self, pid: u32) -> Result<Option<ProcessNode>> {
        match self.processes.get(pid.to_be_bytes())? {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    pub fn remove_process(&self, pid: u32) -> Result<()> {
        self.processes.remove(pid.to_be_bytes())?;
        Ok(())
    }

    pub fn all_processes(&self) -> Result<Vec<ProcessNode>> {
        self.processes
            .iter()
            .values()
            .map(|v| v.map_err(HotStoreError::from).and_then(|bytes| {
                bincode::deserialize(&bytes).map_err(HotStoreError::from)
            }))
            .collect()
    }

    /// Pushes an event onto the ring buffer, trimming the oldest entries
    /// once `MAX_RING_BUFFER` is exceeded so this tree stays bounded no
    /// matter how long the app has been running.
    pub fn push_event(&self, event: &Event) -> Result<()> {
        // Keys are timestamp-ordered so `iter()` naturally yields oldest
        // first, which is what the trim step below relies on.
        let key = format!("{:020}_{}", event.timestamp.timestamp_nanos_opt().unwrap_or(0), event.id);
        let value = serde_json::to_vec(event).map_err(|e| {
            HotStoreError::Encode(bincode::ErrorKind::Custom(e.to_string()).into())
        })?;
        self.events.insert(key.as_bytes(), value)?;

        if self.events.len() > MAX_RING_BUFFER {
            if let Some(Ok((oldest_key, _))) = self.events.iter().next() {
                self.events.remove(oldest_key)?;
            }
        }
        Ok(())
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<Event>> {
        let mut out: Vec<Event> = self
            .events
            .iter()
            .values()
            .rev()
            .take(limit)
            .filter_map(|v| v.ok())
            .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
            .collect();
        out.reverse();
        Ok(out)
    }

    pub fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EventSource, Severity};

    #[test]
    fn round_trips_a_process_node() {
        let store = HotStore::open_temporary().unwrap();
        let node = ProcessNode {
            pid: 1234,
            parent_pid: Some(1),
            name: "firefox".into(),
            exe_path: Some("/usr/bin/firefox".into()),
            started_at: chrono::Utc::now(),
            flagged: false,
        };
        store.upsert_process(&node).unwrap();
        let fetched = store.get_process(1234).unwrap().unwrap();
        assert_eq!(fetched.name, "firefox");
        assert_eq!(fetched.parent_pid, Some(1));
    }

    #[test]
    fn ring_buffer_trims_oldest_first() {
        let store = HotStore::open_temporary().unwrap();
        for i in 0..10 {
            let e = Event::new(EventSource::Behavior, Severity::Info, format!("event {i}"));
            store.push_event(&e).unwrap();
        }
        let recent = store.recent_events(3).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent.last().unwrap().message, "event 9");
    }
}
