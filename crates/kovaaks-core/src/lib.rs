//! kovaaks-core — domain, storage, HTTP and CSV ingest for the KovaaK's
//! companion app.
//!
//! No UI dependencies; consumed by the Tauri app crate. Offline-first: the
//! evxl benchmark registry is embedded at compile time.

pub mod error;
pub mod http;
pub mod kovaaks;
pub mod registry;
pub mod steam;
pub mod store;
pub mod types;

pub use error::{Error, Result};
pub use http::USER_AGENT;
pub use kovaaks::KovaaksClient;
pub use registry::Registry;
pub use steam::EvxlClient;
pub use store::{SnapshotWrite, Store, StoredScenario, StoredSnapshot};
pub use types::{
    BenchmarkDef, BenchmarkProgress, CategoryProgress, Difficulty, PlayRecord, PlayerProfile,
    RankTier, ScenarioEntry,
};
