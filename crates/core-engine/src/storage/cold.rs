//! Historical event log for analytical queries, behind one trait so the
//! backend is swappable (see the module doc in `storage::mod`).

use crate::types::{Event, EventSource, Severity};
use chrono::{DateTime, Utc};

#[derive(Debug, thiserror::Error)]
pub enum ColdStoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[cfg(feature = "duckdb-backend")]
    #[error(transparent)]
    DuckDb(#[from] duckdb::Error),
}

pub type Result<T> = std::result::Result<T, ColdStoreError>;

pub trait ColdStore {
    fn insert_event(&self, event: &Event) -> Result<()>;
    fn recent_since(&self, since: DateTime<Utc>, limit: usize) -> Result<Vec<Event>>;
    fn count_by_severity(&self, since: DateTime<Utc>) -> Result<Vec<(Severity, i64)>>;
}

fn severity_to_str(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warn => "warn",
        Severity::Critical => "critical",
    }
}

fn severity_from_str(s: &str) -> Severity {
    match s {
        "warn" => Severity::Warn,
        "critical" => Severity::Critical,
        _ => Severity::Info,
    }
}

fn source_to_str(s: EventSource) -> &'static str {
    match s {
        EventSource::LogWatch => "log_watch",
        EventSource::Security => "security",
        EventSource::CodeMap => "code_map",
        EventSource::DbMonitor => "db_monitor",
        EventSource::Behavior => "behavior",
        EventSource::License => "license",
    }
}

// ---------------------------------------------------------------------
// SQLite backend (default)
// ---------------------------------------------------------------------

pub struct SqliteColdStore {
    conn: rusqlite::Connection,
}

impl SqliteColdStore {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        Self::init(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        Self::init(&conn)?;
        Ok(Self { conn })
    }

    fn init(conn: &rusqlite::Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                ts TEXT NOT NULL,
                source TEXT NOT NULL,
                severity TEXT NOT NULL,
                message TEXT NOT NULL,
                latency_ms INTEGER,
                metadata TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);
            CREATE INDEX IF NOT EXISTS idx_events_severity ON events(severity);",
        )?;
        Ok(())
    }
}

impl ColdStore for SqliteColdStore {
    fn insert_event(&self, event: &Event) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO events (id, ts, source, severity, message, latency_ms, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                event.id.to_string(),
                event.timestamp.to_rfc3339(),
                source_to_str(event.source),
                severity_to_str(event.severity),
                event.message,
                event.latency_ms,
                serde_json::to_string(&event.metadata)?,
            ],
        )?;
        Ok(())
    }

    fn recent_since(&self, since: DateTime<Utc>, limit: usize) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, source, severity, message, latency_ms, metadata
             FROM events WHERE ts >= ?1 ORDER BY ts DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![since.to_rfc3339(), limit as i64],
            |row| {
                let id: String = row.get(0)?;
                let ts: String = row.get(1)?;
                let source: String = row.get(2)?;
                let severity: String = row.get(3)?;
                let message: String = row.get(4)?;
                let latency_ms: Option<i64> = row.get(5)?;
                let metadata: String = row.get(6)?;
                Ok((id, ts, source, severity, message, latency_ms, metadata))
            },
        )?;

        let mut out = Vec::new();
        for row in rows {
            let (id, ts, source, severity, message, latency_ms, metadata) = row?;
            out.push(Event {
                id: uuid::Uuid::parse_str(&id).unwrap_or_else(|_| uuid::Uuid::new_v4()),
                timestamp: DateTime::parse_from_rfc3339(&ts)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                source: match source.as_str() {
                    "security" => EventSource::Security,
                    "code_map" => EventSource::CodeMap,
                    "db_monitor" => EventSource::DbMonitor,
                    "behavior" => EventSource::Behavior,
                    "license" => EventSource::License,
                    _ => EventSource::LogWatch,
                },
                severity: severity_from_str(&severity),
                message,
                latency_ms: latency_ms.map(|v| v as u64),
                metadata: serde_json::from_str(&metadata)?,
            });
        }
        Ok(out)
    }

    fn count_by_severity(&self, since: DateTime<Utc>) -> Result<Vec<(Severity, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT severity, COUNT(*) FROM events WHERE ts >= ?1 GROUP BY severity",
        )?;
        let rows = stmt.query_map(rusqlite::params![since.to_rfc3339()], |row| {
            let severity: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((severity_from_str(&severity), count))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(ColdStoreError::from)
    }
}

// ---------------------------------------------------------------------
// DuckDB backend (optional, --features duckdb-backend)
//
// Same trait, same table shape. Reach for this when the event history is
// large enough that genuinely columnar/analytical queries pay off; until
// then SQLite is the pragmatic default (much lighter to compile, no
// bundled C++ engine).
// ---------------------------------------------------------------------

#[cfg(feature = "duckdb-backend")]
pub struct DuckDbColdStore {
    conn: duckdb::Connection,
}

#[cfg(feature = "duckdb-backend")]
impl DuckDbColdStore {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let conn = duckdb::Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id VARCHAR PRIMARY KEY,
                ts TIMESTAMP,
                source VARCHAR,
                severity VARCHAR,
                message VARCHAR,
                latency_ms BIGINT,
                metadata VARCHAR
            );",
        )?;
        Ok(Self { conn })
    }
}

#[cfg(feature = "duckdb-backend")]
impl ColdStore for DuckDbColdStore {
    fn insert_event(&self, event: &Event) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO events VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            duckdb::params![
                event.id.to_string(),
                event.timestamp.to_rfc3339(),
                source_to_str(event.source),
                severity_to_str(event.severity),
                event.message,
                event.latency_ms.map(|v| v as i64),
                serde_json::to_string(&event.metadata)?,
            ],
        )?;
        Ok(())
    }

    fn recent_since(&self, _since: DateTime<Utc>, _limit: usize) -> Result<Vec<Event>> {
        // Left as an exercise wired to the same query shape as
        // SqliteColdStore::recent_since - included primarily to prove the
        // trait boundary holds across a genuinely different backend.
        unimplemented!("DuckDB read path: mirror SqliteColdStore::recent_since")
    }

    fn count_by_severity(&self, _since: DateTime<Utc>) -> Result<Vec<(Severity, i64)>> {
        unimplemented!("DuckDB read path: mirror SqliteColdStore::count_by_severity")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventSource;

    #[test]
    fn inserts_and_reads_back_events() {
        let store = SqliteColdStore::open_in_memory().unwrap();
        let e = Event::new(EventSource::Security, Severity::Critical, "CVE found");
        store.insert_event(&e).unwrap();

        let since = Utc::now() - chrono::Duration::minutes(1);
        let rows = store.recent_since(since, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].message, "CVE found");

        let counts = store.count_by_severity(since).unwrap();
        assert_eq!(counts, vec![(Severity::Critical, 1)]);
    }
}
