//! SQLite snapshot store (plan Task 1.5).
//!
//! Single embedded database file at a caller-supplied path. Schema v1 matches
//! the plan block exactly; `captured_at`/`played_at` are stored as RFC3339 UTC
//! TEXT, and numeric score columns declared INTEGER hold rounded values
//! (input scores are f64).
//!
//! `rank_maxes` is serialized per row with serde_json as a JSON array.

use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::types::{BenchmarkProgress, PlayRecord, PlayerProfile};

/// Schema version this build writes; `PRAGMA user_version` gates migrations.
const SCHEMA_VERSION: i64 = 3;

/// Result of [`Store::record_snapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotWrite {
    /// A new snapshot row (+ scenario_scores) was inserted.
    Inserted { id: i64 },
    /// The newest snapshot was identical, so only its `captured_at` moved.
    Deduplicated { id: i64 },
}

/// One stored scenario row inside a snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredScenario {
    pub scenario: String,
    pub category: String,
    /// Score in in-game display units, rounded to the nearest integer for
    /// storage (input is f64; the UI shows decimals from the raw plays table).
    pub score: i64,
    pub leaderboard_rank: i64,
    /// 1-based tier index into the difficulty's `rank_colors` (0 = unplayed).
    pub scenario_rank: i64,
    /// 1-based tier index of the scenario's category in the same ladder
    /// (0 when the payload omitted it) — ranks come from the API's own
    /// per-benchmark rules, not threshold recomputation.
    pub category_rank: i64,
    /// API document order (author/evxl ordering): category index × 10 000 +
    /// scenario index. Ordering for tables/pickers must sort by this.
    pub api_order: i64,
    /// Scenario rank thresholds (JSON array column round-tripped via serde_json).
    pub rank_maxes: Vec<f64>,
}

/// One stored snapshot with its scenario rows.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredSnapshot {
    pub id: i64,
    pub steam_id: String,
    pub benchmark_id: i64,
    pub captured_at: DateTime<Utc>,
    pub benchmark_progress: i64,
    pub overall_rank: i64,
    /// Scenario rows ordered by (category, scenario).
    pub scenarios: Vec<StoredScenario>,
}

/// The SQLite store. Cheap to clone; every method takes `&self` (the rusqlite
/// connection sits behind a mutex, so a store handle can be shared freely).
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

/// Internal (category, scenario, score, leaderboard_rank, scenario_rank,
/// category_rank, rank_maxes-json) tuple used for dedup comparison; sorted so
/// HashMap iteration order never affects equality.
type ScenarioRow = (String, String, i64, i64, i64, i64, i64, String);

fn scenario_rows_from_progress(progress: &BenchmarkProgress) -> Result<Vec<ScenarioRow>> {
    let mut rows: Vec<ScenarioRow> = Vec::new();
    for (cat_idx, (cat_name, cat)) in progress.categories.iter().enumerate() {
        for (scen_idx, (scen_name, scen)) in cat.scenarios.iter().enumerate() {
            rows.push((
                cat_name.clone(),
                scen_name.clone(),
                scen.score.round() as i64,
                scen.leaderboard_rank.min(i64::MAX as u64) as i64,
                scen.scenario_rank as i64,
                cat.category_rank as i64,
                (cat_idx * 10_000 + scen_idx) as i64, // API document order
                serde_json::to_string(&scen.rank_maxes)?,
            ));
        }
    }
    Ok(rows)
}

fn scenario_rows_from_conn(conn: &Connection, snapshot_id: i64) -> Result<Vec<StoredScenario>> {
    let mut stmt = conn.prepare(
        "SELECT scenario, category, score, leaderboard_rank, scenario_rank, category_rank, order_idx, rank_maxes
         FROM scenario_scores WHERE snapshot_id = ?1
         ORDER BY order_idx, scenario",
    )?;
    let rows = stmt
        .query_map(params![snapshot_id], |row| {
            let maxes: String = row.get(7)?;
            Ok(StoredScenario {
                scenario: row.get(0)?,
                category: row.get(1)?,
                score: row.get(2)?,
                leaderboard_rank: row.get(3)?,
                scenario_rank: row.get(4)?,
                category_rank: row.get(5)?,
                api_order: row.get(6)?,
                rank_maxes: serde_json::from_str(&maxes).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn scenario_row_tuples(stored: &[StoredScenario]) -> Result<Vec<ScenarioRow>> {
    let mut rows: Vec<ScenarioRow> = Vec::with_capacity(stored.len());
    for s in stored {
        rows.push((
            s.category.clone(),
            s.scenario.clone(),
            s.score,
            s.leaderboard_rank,
            s.scenario_rank,
            s.category_rank,
            s.api_order,
            serde_json::to_string(&s.rank_maxes)?,
        ));
    }
    rows.sort();
    Ok(rows)
}

/// Parse an RFC3339 TEXT column into a UTC timestamp, surfacing failures as
/// rusqlite conversion errors (never panics).
fn parse_rfc3339_column(col: usize, text: &str) -> rusqlite::Result<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(e))
        })
}

impl Store {
    /// Open (creating + migrating as needed) the database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Self::migrate(conn)
    }

    /// Run pending migrations gated on `PRAGMA user_version`.
    fn migrate(conn: Connection) -> Result<Self> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
        }
        if version < 2 {
            // v2: centi→display unit rescale + API-rank columns. Snapshot data
            // is a re-fetchable cache (dedup keeps only the newest state), so
            // the correct migration is a rebuild; plays + meta are preserved.
            conn.execute_batch(
                "BEGIN;
                 DROP TABLE scenario_scores;
                 DROP TABLE snapshots;
                 CREATE TABLE snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    steam_id TEXT NOT NULL,
                    benchmark_id INTEGER NOT NULL,
                    captured_at TEXT NOT NULL,
                    benchmark_progress INTEGER NOT NULL,
                    overall_rank INTEGER NOT NULL,
                    UNIQUE (steam_id, benchmark_id, captured_at)
                 );
                 CREATE TABLE scenario_scores (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
                    scenario TEXT NOT NULL,
                    category TEXT NOT NULL,
                    score INTEGER NOT NULL,
                    leaderboard_rank INTEGER NOT NULL,
                    scenario_rank INTEGER NOT NULL,
                    category_rank INTEGER NOT NULL DEFAULT 0,
                    rank_maxes TEXT NOT NULL,
                    UNIQUE (snapshot_id, scenario, category)
                 );
                 CREATE INDEX idx_snapshots_player ON snapshots(steam_id, benchmark_id, captured_at);
                 CREATE INDEX idx_scenario_scores_snapshot ON scenario_scores(snapshot_id);
                 COMMIT;",
            )?;
            // Snapshots are gone; force the next sync to re-pull everything.
            conn.execute("DELETE FROM benchmarks_playing", [])?;
        }
        if version < 3 {
            // v3: preserve the API's scenario ordering (author/evxl order).
            // Snapshot cache is re-fetchable — same rebuild strategy as v2.
            conn.execute_batch(
                "BEGIN;
                 DROP TABLE scenario_scores;
                 DROP TABLE snapshots;
                 CREATE TABLE snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    steam_id TEXT NOT NULL,
                    benchmark_id INTEGER NOT NULL,
                    captured_at TEXT NOT NULL,
                    benchmark_progress INTEGER NOT NULL,
                    overall_rank INTEGER NOT NULL,
                    UNIQUE (steam_id, benchmark_id, captured_at)
                 );
                 CREATE TABLE scenario_scores (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
                    scenario TEXT NOT NULL,
                    category TEXT NOT NULL,
                    score INTEGER NOT NULL,
                    leaderboard_rank INTEGER NOT NULL,
                    scenario_rank INTEGER NOT NULL,
                    category_rank INTEGER NOT NULL DEFAULT 0,
                    order_idx INTEGER NOT NULL DEFAULT 0,
                    rank_maxes TEXT NOT NULL,
                    UNIQUE (snapshot_id, scenario, category)
                 );
                 CREATE INDEX idx_snapshots_player ON snapshots(steam_id, benchmark_id, captured_at);
                 CREATE INDEX idx_scenario_scores_snapshot ON scenario_scores(snapshot_id);
                 COMMIT;",
            )?;
            conn.execute("DELETE FROM benchmarks_playing", [])?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Lock the connection (poisoning only happens if a callback panicked).
    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Record one benchmark snapshot. Dedup: when the newest snapshot for
    /// `(steam_id, benchmark_id)` has an identical `benchmark_progress` AND an
    /// identical set of scenario scores, its `captured_at` is updated instead
    /// of inserting a duplicate row.
    pub fn record_snapshot(
        &self,
        steam_id: &str,
        benchmark_id: i64,
        progress: &BenchmarkProgress,
        captured_at: DateTime<Utc>,
    ) -> Result<SnapshotWrite> {
        let conn = self.lock();
        let new_rows = scenario_rows_from_progress(progress)?;

        let tx = conn.unchecked_transaction()?;
        let newest: Option<i64> = conn
            .query_row(
                "SELECT id FROM snapshots
                 WHERE steam_id = ?1 AND benchmark_id = ?2
                 ORDER BY captured_at DESC, id DESC LIMIT 1",
                params![steam_id, benchmark_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(newest_id) = newest {
            let stored_progress: i64 = conn.query_row(
                "SELECT benchmark_progress FROM snapshots WHERE id = ?1",
                params![newest_id],
                |r| r.get(0),
            )?;
            let stored = scenario_rows_from_conn(&conn, newest_id)?;
            let stored_rows = scenario_row_tuples(&stored)?;
            let mut new_sorted = new_rows.clone();
            new_sorted.sort();
            if stored_progress == progress.benchmark_progress.round() as i64
                && stored_rows == new_sorted
            {
                conn.execute(
                    "UPDATE snapshots SET captured_at = ?1 WHERE id = ?2",
                    params![captured_at.to_rfc3339(), newest_id],
                )?;
                tx.commit()?;
                return Ok(SnapshotWrite::Deduplicated { id: newest_id });
            }
        }

        conn.execute(
            "INSERT INTO snapshots
             (steam_id, benchmark_id, captured_at, benchmark_progress, overall_rank)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                steam_id,
                benchmark_id,
                captured_at.to_rfc3339(),
                progress.benchmark_progress.round() as i64,
                progress.overall_rank as i64,
            ],
        )?;
        let snapshot_id = conn.last_insert_rowid();
        for (category, scenario, score, lrank, srank, crank, oidx, maxes) in &new_rows {
            conn.execute(
                "INSERT INTO scenario_scores
                 (snapshot_id, scenario, category, score, leaderboard_rank, scenario_rank, category_rank, order_idx, rank_maxes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![snapshot_id, scenario, category, score, lrank, srank, crank, oidx, maxes],
            )?;
        }
        tx.commit()?;
        Ok(SnapshotWrite::Inserted { id: snapshot_id })
    }

    /// All snapshots for (steam_id, benchmark_id), ascending captured_at.
    pub fn history(&self, steam_id: &str, benchmark_id: i64) -> Result<Vec<StoredSnapshot>> {
        let conn = self.lock();
        history_inner(&conn, steam_id, benchmark_id)
    }

    /// Newest snapshot for (steam_id, benchmark_id) if any.
    pub fn latest(&self, steam_id: &str, benchmark_id: i64) -> Result<Option<StoredSnapshot>> {
        let conn = self.lock();
        Ok(history_inner(&conn, steam_id, benchmark_id)?.pop())
    }

    /// Insert one CSV play; idempotent on `csv_path` (INSERT OR IGNORE
    /// semantics with an explicit no-op signal). Returns `true` when a new row
    /// was written, `false` when the file had already been ingested.
    pub fn record_play(&self, steam_id: &str, record: &PlayRecord, csv_path: &str) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute(
            "INSERT OR IGNORE INTO plays
             (steam_id, scenario, played_at, score, hit_count, avg_fps, csv_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                steam_id,
                record.scenario,
                record.played_at.to_rfc3339(),
                record.score,
                record.hit_count as i64,
                record.avg_fps,
                csv_path,
            ],
        )?;
        Ok(n == 1)
    }

    /// All plays of one scenario for a player, ascending played_at.
    pub fn plays_history(&self, steam_id: &str, scenario: &str) -> Result<Vec<PlayRecord>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT scenario, played_at, score, hit_count, avg_fps
             FROM plays WHERE steam_id = ?1 AND scenario = ?2
             ORDER BY played_at, id",
        )?;
        let rows = stmt
            .query_map(params![steam_id, scenario], |row| {
                let played: String = row.get(1)?;
                let played_at = parse_rfc3339_column(1, &played)?;
                Ok(PlayRecord {
                    scenario: row.get(0)?,
                    played_at,
                    score: row.get(2)?,
                    hit_count: row.get::<_, i64>(3)? as u64,
                    avg_fps: row.get(4)?,
                    source: "csv".to_string(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Insert or update a meta key/value pair.
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Read a meta key (None when absent).
    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let v: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(v)
    }

    /// Insert or update a `benchmarks_playing` row (idempotent).
    pub fn upsert_played(
        &self,
        steam_id: &str,
        benchmark_id: i64,
        played: bool,
        last_checked: DateTime<Utc>,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO benchmarks_playing (steam_id, benchmark_id, played, last_checked)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(steam_id, benchmark_id)
             DO UPDATE SET played = excluded.played, last_checked = excluded.last_checked",
            params![
                steam_id,
                benchmark_id,
                played as i64,
                last_checked.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Every benchmark id flagged played for a player, ascending.
    pub fn played_benchmarks(&self, steam_id: &str) -> Result<Vec<i64>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT benchmark_id FROM benchmarks_playing
             WHERE steam_id = ?1 AND played = 1 ORDER BY benchmark_id",
        )?;
        let ids = stmt
            .query_map(params![steam_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// `benchmarks_playing` rows for a player ordered by last_checked (used by
    /// the sync engine's stale sweep).
    pub fn benchmarks_playing_rows(
        &self,
        steam_id: &str,
    ) -> Result<Vec<(i64, bool, DateTime<Utc>)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT benchmark_id, played, last_checked FROM benchmarks_playing
             WHERE steam_id = ?1 ORDER BY benchmark_id",
        )?;
        let rows = stmt
            .query_map(params![steam_id], |row| {
                let checked: String = row.get(2)?;
                let last_checked = parse_rfc3339_column(2, &checked)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    last_checked,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Insert or update the player profile; `first_seen` is set on first
    /// insert and left untouched by updates.
    pub fn upsert_player(&self, profile: &PlayerProfile) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO players (steam_id, persona, avatar_url, country, first_seen)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(steam_id) DO UPDATE SET
                persona = excluded.persona,
                avatar_url = excluded.avatar_url,
                country = excluded.country",
            params![
                profile.steam_id,
                profile.persona,
                profile.avatar_url,
                profile.country,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Stored player profile if known.
    pub fn player(&self, steam_id: &str) -> Result<Option<PlayerProfile>> {
        let conn = self.lock();
        let result = conn
            .query_row(
                "SELECT steam_id, persona, avatar_url, country FROM players WHERE steam_id = ?1",
                params![steam_id],
                |r| {
                    Ok(PlayerProfile {
                        steam_id: r.get(0)?,
                        persona: r.get(1)?,
                        avatar_url: r.get(2)?,
                        country: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }
}

fn history_inner(
    conn: &Connection,
    steam_id: &str,
    benchmark_id: i64,
) -> Result<Vec<StoredSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT id, captured_at, benchmark_progress, overall_rank
         FROM snapshots WHERE steam_id = ?1 AND benchmark_id = ?2
         ORDER BY captured_at, id",
    )?;
    let mut snaps = stmt
        .query_map(params![steam_id, benchmark_id], |row| {
            let captured: String = row.get(1)?;
            let captured_at = parse_rfc3339_column(1, &captured)?;
            Ok((
                row.get::<_, i64>(0)?,
                captured_at,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(snaps.len());
    for (id, captured_at, benchmark_progress, overall_rank) in snaps.drain(..) {
        out.push(StoredSnapshot {
            id,
            steam_id: steam_id.to_string(),
            benchmark_id,
            captured_at,
            benchmark_progress,
            overall_rank,
            scenarios: scenario_rows_from_conn(conn, id)?,
        });
    }
    Ok(out)
}

const SCHEMA_V1: &str = r#"BEGIN;
CREATE TABLE players (
    steam_id TEXT PRIMARY KEY,
    persona TEXT NOT NULL,
    avatar_url TEXT NOT NULL,
    country TEXT NOT NULL,
    first_seen TEXT NOT NULL
);
CREATE TABLE benchmarks_playing (
    steam_id TEXT NOT NULL,
    benchmark_id INTEGER NOT NULL,
    played INTEGER NOT NULL,
    last_checked TEXT NOT NULL,
    PRIMARY KEY (steam_id, benchmark_id)
);
CREATE TABLE snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    steam_id TEXT NOT NULL,
    benchmark_id INTEGER NOT NULL,
    captured_at TEXT NOT NULL /* RFC3339 */,
    benchmark_progress INTEGER NOT NULL,
    overall_rank INTEGER NOT NULL,
    UNIQUE (steam_id, benchmark_id, captured_at)
);
CREATE TABLE scenario_scores (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    scenario TEXT NOT NULL,
    category TEXT NOT NULL,
    score INTEGER NOT NULL,
    leaderboard_rank INTEGER NOT NULL,
    scenario_rank INTEGER NOT NULL,
    rank_maxes TEXT NOT NULL /* JSON array */,
    UNIQUE (snapshot_id, scenario, category)
);
CREATE TABLE plays (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    steam_id TEXT NOT NULL,
    scenario TEXT NOT NULL,
    played_at TEXT NOT NULL,
    score REAL NOT NULL,
    hit_count INTEGER NOT NULL,
    avg_fps REAL NOT NULL,
    csv_path TEXT NOT NULL UNIQUE,
    UNIQUE (steam_id, csv_path)
);
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE INDEX idx_snapshots_player ON snapshots(steam_id, benchmark_id, captured_at);
CREATE INDEX idx_scenario_scores_snapshot ON scenario_scores(snapshot_id);
CREATE INDEX idx_plays_player_scenario ON plays(steam_id, scenario);
COMMIT;"#;
