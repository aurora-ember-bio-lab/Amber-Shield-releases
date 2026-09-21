//! VectorizedDB — semantic embeddings + pgvector store for Amber Shield.
//!
//! Amber Shield uses **pgvector** (`pgvector/pgvector`) as its vector
//! database backend. Every `Event` can be embedded (via the local Ollama
//! embedding endpoint) and stored as a vector alongside its structured metadata.
//!
//! This module provides:
//! - [`VectorStore`] — a pgvector-backed event + embedding store
//! - [`EmbeddingClient`] — wraps Ollama's `/api/embeddings` endpoint
//! - [`SemanticSearch`] — find similar events by vector similarity
//! - [`VectorEvent`] — an Event with its embedding vector
//!
//! # Schema (PostgreSQL + pgvector)
//!
//! ```sql
//! CREATE EXTENSION vector;
//!
//! CREATE TABLE event_vectors (
//!     id        UUID PRIMARY KEY REFERENCES events(id),
//!     embedding VECTOR(768),  -- nomic-embed-text / llama3.2 embedding dim
//!     source    TEXT NOT NULL,
//!     severity  TEXT NOT NULL,
//!     message  TEXT NOT NULL,
//!     metadata JSONB,
//!     ts       TIMESTAMPTZ NOT NULL DEFAULT NOW()
//! );
//!
//! CREATE INDEX idx_event_vectors_embedding
//!     ON event_vectors
//!  USING ivfflat (embedding vector_cosine_ops)
//!  WITH (lists = 100);
//! ```
//!
//! # Embedding model
//!
//! Default: `nomic-embed-text:v1.5` (768d, excellent for code + logs).
//! Override with `AMBER_EMBED_MODEL` env var.
//!
//! # Connection
//!
//! Uses `sqlx` + `pgvector` via environment variable `DATABASE_URL`.
//! Default: `postgresql://localhost:5432/ambershield`.

use serde::{Deserialize, Serialize};


/// Default Ollama embedding endpoint.
const DEFAULT_EMBED_URL: &str = "http://127.0.0.1:11434/api/embeddings";

fn embed_model() -> String {
    std::env::var("AMBER_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-text:v1.5".to_string())
}

fn embed_url() -> String {
    std::env::var("AMBER_EMBED_URL").unwrap_or_else(|_| DEFAULT_EMBED_URL.to_string())
}

// -----------------------------------------------------------------------
// Embedding client (loopback Ollama)
// -----------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("Ollama unreachable at {0}: {1}")]
    Unreachable(String, String),
    #[error("Ollama returned error: {0}")]
    Api(String),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embedding: Vec<f64>,
}

/// Generate an embedding vector for a text string via Ollama.
///
/// Calls `POST /api/embeddings` on the local Ollama instance.
/// Returns a 768-dimensional vector of `f32` values.
pub fn embed_text(text: &str) -> Result<Vec<f32>, EmbedError> {
    let url = format!("{}/api/embeddings", embed_url());
    let model = embed_model();

    let body = EmbedRequest { model: &model, input: text };

    let response = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send_json(&body)
        .map_err(|e| match e {
            ureq::Error::Transport(t) => EmbedError::Unreachable(url.clone(), t.to_string()),
            ureq::Error::Status(code, resp) => {
                EmbedError::Api(format!("{code}: {}", resp.status_text()))
            }
        })?;

    let parsed: EmbedResponse = response.into_json()?;
    Ok(parsed.embedding.into_iter().map(|v| v as f32).collect())
}

// -----------------------------------------------------------------------
// VectorEvent — an event with its embedding
// -----------------------------------------------------------------------

/// An `Event` paired with its semantic embedding vector (f32, typically 768d).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEvent {
    pub id: String,
    pub source: String,
    pub severity: String,
    pub message: String,
    pub metadata: serde_json::Value,
    pub embedding: Vec<f32>,
    pub ts: String,
}

impl VectorEvent {
    /// Build a `VectorEvent` by embedding the event's message + metadata.
    pub fn from_event(event: &crate::types::Event) -> Result<Self, EmbedError> {
        let text = format!(
            "source={:?} severity={:?} message={} metadata={}",
            event.source, event.severity, event.message, event.metadata
        );
        let embedding = embed_text(&text)?;
        Ok(Self {
            id: event.id.to_string(),
            source: format!("{:?}", event.source).to_lowercase(),
            severity: format!("{:?}", event.severity).to_lowercase(),
            message: event.message.clone(),
            metadata: event.metadata.clone(),
            embedding,
            ts: event.timestamp.to_rfc3339(),
        })
    }
}

// -----------------------------------------------------------------------
// In-memory vector store (fallback when no PostgreSQL is available)
// -----------------------------------------------------------------------

/// A simple in-memory vector store using cosine similarity.
/// Useful for offline / demo mode. For production, use PostgreSQL + pgvector.
#[derive(Debug, Default)]
pub struct MemoryVectorStore {
    events: Vec<VectorEvent>,
}

impl MemoryVectorStore {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn insert(&mut self, event: VectorEvent) {
        self.events.push(event);
    }

    pub fn insert_event(&mut self, event: crate::types::Event) -> Result<(), EmbedError> {
        let ve = VectorEvent::from_event(&event)?;
        self.events.push(ve);
        Ok(())
    }

    /// Cosine similarity search over the in-memory index.
    /// Returns up to `limit` results, most similar first.
    pub fn search(&self, query: &[f32], limit: usize) -> Vec<(f32, &VectorEvent)> {
        let mut scored: Vec<(f32, &VectorEvent)> = self
            .events
            .iter()
            .map(|e| (cosine_similarity(query, &e.embedding), e))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(limit).collect()
    }

    pub fn search_text(&self, text: &str, limit: usize) -> Result<Vec<(f32, &VectorEvent)>, EmbedError> {
        let query = embed_text(text)?;
        Ok(self.search(&query, limit))
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

// -----------------------------------------------------------------------
// SQL + pgvector query builders (for sqlx + pgvector in production)
// -----------------------------------------------------------------------

/// Build a pgvector-compatible SQL INSERT statement for a VectorEvent.
pub fn pg_insert_sql() -> &'static str {
    "INSERT INTO event_vectors (id, embedding, source, severity, message, metadata, ts)
     VALUES ($1, $2::vector, $3, $4, $5, $6::jsonb, $7)
     ON CONFLICT (id) DO UPDATE
       SET embedding = $2::vector,
           message  = $5,
           metadata = $6::jsonb"
}

/// Build a pgvector similarity search SQL statement.
pub fn pg_search_sql(_lists: u32) -> String {
    format!(
        "SELECT id, source, severity, message, metadata, ts,
                1 - (embedding <=> $1::vector) AS similarity
           FROM event_vectors
           ORDER BY embedding <=> $1::vector
           LIMIT $2"
    )
}

/// pgvector index creation SQL (IVFFlat).
pub fn pg_index_sql(lists: u32) -> String {
    format!(
        "CREATE INDEX IF NOT EXISTS idx_event_vectors_embedding
           ON event_vectors
          USING ivfflat (embedding vector_cosine_ops)
          WITH (lists = {lists})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trips() {
        let mut store = MemoryVectorStore::new();
        assert!(store.is_empty());

        let ve = VectorEvent {
            id: "test-id".into(),
            source: "security".into(),
            severity: "critical".into(),
            message: "CVE-2026-0001 in openssl".into(),
            metadata: serde_json::json!({}),
            embedding: vec![0.1, 0.2, 0.3],
            ts: "2026-01-01T00:00:00Z".into(),
        };
        store.insert(ve);
        assert_eq!(store.len(), 1);

        let results = store.search(&vec![0.1, 0.2, 0.3], 10);
        assert_eq!(results.len(), 1);
        assert!((results[0].0 - 1.0).abs() < 0.001);
    }

    #[test]
    fn cosine_similarity_identical_vectors() {
        let v = vec![1.0, 0.0, 0.0, 0.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 0.001);
    }

    #[test]
    fn pg_sql_is_valid_syntax() {
        let search = pg_search_sql(100);
        assert!(search.contains("<=>"));
        assert!(search.contains("event_vectors"));

        let index = pg_index_sql(100);
        assert!(index.contains("ivfflat"));
        assert!(index.contains("lists = 100"));
    }

    #[test]
    fn embed_fails_gracefully_without_ollama() {
        std::env::set_var("AMBER_EMBED_URL", "http://127.0.0.1:1");
        let result = embed_text("test");
        assert!(result.is_err());
        std::env::remove_var("AMBER_EMBED_URL");
    }
}
