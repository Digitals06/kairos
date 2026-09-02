//! kovaaks-core — domain, storage, HTTP and CSV ingest for the KovaaK's
//! companion app.
//!
//! No UI dependencies; consumed by the Tauri app crate. Offline-first: the
//! evxl benchmark registry is embedded at compile time.

pub mod error;
pub mod types;

pub use error::{Error, Result};
pub use types::{
    BenchmarkDef, BenchmarkProgress, CategoryProgress, Difficulty, PlayRecord, PlayerProfile,
    RankTier, ScenarioEntry,
};
