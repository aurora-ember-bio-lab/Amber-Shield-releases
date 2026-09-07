//! Two embedded stores, on purpose, instead of one database trying to be
//! both: see the architecture review — DuckDB-shaped columnar engines and
//! SurrealDB-shaped graph engines solve different problems, and forcing one
//! engine to do both jobs was the weakest part of the original spec.
//!
//! - [`hot`]: `sled`, a small embedded KV store, holds *live* state - the
//!   process/behavior graph, the most recent N events for the HUD. It's
//!   cheap to read/write at high frequency.
//! - [`cold`]: a SQL store holds the historical event log for analytical
//!   queries ("show me every critical event last week"). The default
//!   backend is SQLite via `rusqlite` (fast to compile, zero footprint,
//!   extremely well understood). A DuckDB-backed implementation of the same
//!   [`ColdStore`] trait is available behind the `duckdb-backend` feature
//!   for when genuinely columnar/analytical queries over large histories
//!   are needed - same interface, swap the backend, nothing above this
//!   module has to change.

pub mod cold;
pub mod hot;

pub use cold::{ColdStore, SqliteColdStore};
pub use hot::HotStore;

#[cfg(feature = "duckdb-backend")]
pub use cold::DuckDbColdStore;
