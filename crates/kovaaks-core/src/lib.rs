//! kovaaks-core — domain, storage, HTTP and CSV ingest for the KovaaK's
//! companion app.
//!
//! No UI dependencies; consumed by the Tauri app crate. Offline-first: the
//! evxl benchmark registry is embedded at compile time.

pub mod csv_ingest;
pub mod error;
pub mod http;
pub mod kovaaks;
pub mod metrics;
pub mod ranks;
pub mod registry;
pub mod steam;
pub mod store;
pub mod sync;
pub mod types;

pub use error::{Error, Result};
pub use http::USER_AGENT;
pub use kovaaks::KovaaksClient;
pub use metrics::{
    compute, compute_trailing_30d, compute_window, metrics_for_benchmark,
    metrics_for_scenario_plays, Metrics,
};
pub use ranks::{rank_for, scenario_rank_tier};
pub use registry::Registry;
pub use steam::EvxlClient;
pub use store::{SnapshotWrite, Store, StoredScenario, StoredSnapshot};
pub use sync::{ProgressSource, SyncEngine, SyncReport};
pub use types::{
    BenchmarkDef, BenchmarkProgress, CategoryProgress, Difficulty, PlayRecord, PlayerProfile,
    RankTier, ScenarioEntry,
};
