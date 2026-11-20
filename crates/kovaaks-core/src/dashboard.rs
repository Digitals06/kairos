//! Dashboard rollups (`v0.2.0` money shot): per-category banking sections,
//! each bundling the user's benchmarks with their overall-rank tier history
//! over time — 100% local snapshot data, no API fallbacks.

use crate::rankdiff::engine_rank_for_snapshot;
use crate::store::Store;

/// One benchmark's rank history inside a category section.
#[derive(Debug, Clone, PartialEq)]
pub struct RankHistoryRow {
    pub benchmark_id: i64,
    pub benchmark_name: String,
    /// Engine ladder rank, 1-based (`ladder_len + 1` = Complete; −1 = unranked).
    pub current_rank: i64,
    pub current_tier: String,
    /// The benchmark's tier ladder names (coloring + next-tier labels).
    pub tier_names: Vec<String>,
    /// (captured_at RFC3339, engine tier name) collapsed to tier changes only.
    pub series: Vec<(String, String)>,
    /// Progress-to-next from the latest snapshot's stored progress (raw).
    pub benchmark_progress: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CategorySection {
    /// Top-level category label as stored (CLICKING / TRACKING / …).
    pub category: String,
    pub benchmarks: Vec<RankHistoryRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardRollup {
    /// Sections sorted by member count (biggest category first).
    pub sections: Vec<CategorySection>,
}

pub fn dashboard_rollup(store: &Store, steam_id: &str) -> crate::Result<DashboardRollup> {
    let registry = crate::registry::registry();
    let bids = store.played_benchmarks(steam_id)?;
    let mut sections: Vec<CategorySection> = Vec::new();
    for bid in bids {
        let Some(latest_snapshot) = store.latest(steam_id, bid)? else {
            continue;
        };
        let Some((bench, diff)) = registry.by_id(bid as u64) else {
            continue;
        };
        let Some((current_rank, current_tier)) =
            engine_rank_for_snapshot(&registry, &latest_snapshot)
        else {
            // Latest snapshot no longer maps to a registry benchmark — there
            // is no engine answer to surface, so the whole row is skipped.
            continue;
        };
        let tier_names: Vec<String> = diff
            .rank_colors
            .iter()
            .map(|r| r.name.to_string())
            .collect();
        let history = store.history(steam_id, bid)?;
        // Mid-history snapshots without an engine answer are skipped from the
        // series; the remaining points still collapse to tier changes.
        let mut series: Vec<(String, String)> = history
            .iter()
            .filter_map(|s| {
                engine_rank_for_snapshot(&registry, s)
                    .map(|(_, name)| (s.captured_at.to_rfc3339(), name))
            })
            .collect();
        series.dedup_by(|a, b| a.1 == b.1);
        // evxl 12-tab taxonomy (BenchmarkDef-driven, same as the card filters).
        let mut cats: Vec<String> = crate::bench_type::benchmark_types_for_def(bench)
            .into_iter()
            .map(String::from)
            .collect();
        cats.sort();
        cats.dedup();
        for cat in cats {
            let section = match sections.iter_mut().find(|s| s.category == cat) {
                Some(s) => s,
                None => {
                    sections.push(CategorySection {
                        category: cat,
                        benchmarks: Vec::new(),
                    });
                    sections.last_mut().unwrap()
                }
            };
            section.benchmarks.push(RankHistoryRow {
                benchmark_id: bid,
                benchmark_name: bench.name.clone(),
                current_rank: current_rank as i64,
                current_tier: current_tier.clone(),
                tier_names: tier_names.clone(),
                series: series.clone(),
                benchmark_progress: latest_snapshot.benchmark_progress,
            });
        }
    }
    sections.sort_by_key(|s| std::cmp::Reverse(s.benchmarks.len()));
    Ok(DashboardRollup { sections })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BenchmarkProgress, CategoryProgress, ScenarioEntry};

    /// One-scenario 644 (`mh`, Easy ladder [100, 200, 300, 400]). The engine
    /// reads only the score + rank_maxes; `stored_rank` is written into the
    /// API `overall_rank` column so tests can make it disagree with the engine.
    fn prog(score: f64, stored_rank: u32) -> BenchmarkProgress {
        BenchmarkProgress {
            benchmark_progress: score,
            overall_rank: stored_rank,
            categories: vec![(
                "Tracking".to_string(),
                CategoryProgress {
                    benchmark_progress: 0.0,
                    category_rank: 0,
                    rank_maxes: Vec::new(),
                    scenarios: vec![(
                        "VT Tracking scenario".to_string(),
                        ScenarioEntry {
                            score,
                            leaderboard_rank: 0,
                            scenario_rank: 0,
                            rank_maxes: vec![100.0, 200.0, 300.0, 400.0],
                            leaderboard_id: 0,
                        },
                    )],
                },
            )],
        }
    }

    #[test]
    fn sections_rank_through_engine_not_stored_column() {
        let dir = std::env::temp_dir().join(format!("dash-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let store = crate::store::Store::open(&dir).unwrap();
        let now = chrono::Utc::now();
        let id = "SID1";
        store.upsert_played(id, 644, true, now).unwrap();
        // Engine: 210/250 → Bronze (rank 2), 500 → Silver (rank 4).
        // Stored column lies on every snapshot (3, 1, 1).
        store
            .record_snapshot(id, 644, &prog(210.0, 3), now - chrono::Duration::days(5))
            .unwrap();
        store
            .record_snapshot(id, 644, &prog(250.0, 1), now - chrono::Duration::days(4))
            .unwrap();
        store
            .record_snapshot(id, 644, &prog(500.0, 1), now - chrono::Duration::days(2))
            .unwrap();
        let roll = dashboard_rollup(&store, id).unwrap();
        assert_eq!(
            roll.sections.len(),
            1,
            "single Tracking section — got {:?}",
            roll.sections
        );
        let tracking = roll
            .sections
            .iter()
            .find(|s| s.category == "Tracking")
            .unwrap();
        assert_eq!(tracking.benchmarks.len(), 1);
        let row = &tracking.benchmarks[0];
        let stored = store.latest(id, 644).unwrap().unwrap().overall_rank;
        assert_eq!(stored, 1, "fixture keeps the stored column lying");
        assert_eq!(
            row.current_rank, 4,
            "current_rank must be the engine rank, not stored {stored}"
        );
        assert_eq!(row.current_tier, "Silver", "engine tier name");
        // First two snapshots share the engine's Bronze tier → one entry.
        assert_eq!(row.series.len(), 2, "series {:?}", row.series);
        assert_eq!(row.series[0].1, "Bronze");
        assert_eq!(row.series[1].1, "Silver");
        let _ = std::fs::remove_file(&dir);
    }
}

/// Per-difficulty (kovaaks id) row inside a family.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyDiff {
    pub difficulty_name: String,
    pub kovaaks_id: i64,
    /// Engine ladder rank, 1-based (`ladder_len + 1` = Complete).
    pub current_rank: i64,
    pub current_tier: String,
    pub tier_names: Vec<String>,
    /// (captured_at RFC3339, engine tier name), collapsed to changes.
    pub series: Vec<(String, String)>,
    pub benchmark_progress: i64,
}

/// One benchmark family (registry BenchmarkDef) with its played difficulties.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyRow {
    pub benchmark_name: String,
    pub color: String,
    /// evxl tab tags (same taxonomy as the card filters).
    pub categories: Vec<String>,
    pub difficulties: Vec<FamilyDiff>,
}

/// Highest engine rank across the family's played difficulties (for the
/// collapsed state). Compares [`FamilyDiff::current_rank`], not the tier name,
/// so ladder-end names outside `rank_colors` (e.g. "Silver Complete") rank
/// correctly.
impl FamilyRow {
    pub fn peak_tier(&self) -> (i64, String) {
        self.difficulties
            .iter()
            .max_by_key(|d| d.current_rank)
            .map(|d| (d.current_rank, d.current_tier.clone()))
            .unwrap_or((-1, "—".to_string()))
    }
}

/// Family-grouped dashboard: every played kovaaks id folds into its benchmark
/// family under the registry difficulty it belongs to.
pub fn dashboard_families(store: &Store, steam_id: &str) -> crate::Result<Vec<FamilyRow>> {
    let registry = crate::registry::registry();
    let bids = store.played_benchmarks(steam_id)?;
    let mut rows: Vec<FamilyRow> = Vec::new();
    for bid in bids {
        let Some(latest) = store.latest(steam_id, bid)? else {
            continue;
        };
        let Some((def, diff)) = registry.by_id(bid as u64) else {
            continue;
        };
        let Some((current_rank, current_tier)) = engine_rank_for_snapshot(&registry, &latest)
        else {
            // Latest snapshot not engine-computable — no rank to surface.
            continue;
        };
        let tier_names: Vec<String> = diff
            .rank_colors
            .iter()
            .map(|r| r.name.to_string())
            .collect();
        let history = store.history(steam_id, bid)?;
        // Skip snapshots the engine cannot rank; keep the tier-change collapse.
        let mut series: Vec<(String, String)> = history
            .iter()
            .filter_map(|s| {
                engine_rank_for_snapshot(&registry, s)
                    .map(|(_, name)| (s.captured_at.to_rfc3339(), name))
            })
            .collect();
        series.dedup_by(|a, b| a.1 == b.1);
        let fdiff = FamilyDiff {
            difficulty_name: diff.name.clone(),
            kovaaks_id: bid,
            current_rank: current_rank as i64,
            current_tier,
            tier_names,
            series,
            benchmark_progress: latest.benchmark_progress,
        };
        if let Some(row) = rows.iter_mut().find(|r| r.benchmark_name == def.name) {
            row.difficulties.push(fdiff);
        } else {
            rows.push(FamilyRow {
                benchmark_name: def.name.clone(),
                color: def.color.clone(),
                categories: crate::bench_type::benchmark_types_for_def(def)
                    .into_iter()
                    .map(String::from)
                    .collect(),
                difficulties: vec![fdiff],
            });
        }
    }
    rows.sort_by(|a, b| a.benchmark_name.cmp(&b.benchmark_name));
    Ok(rows)
}

#[cfg(test)]
mod family_tests {
    use super::*;
    use crate::types::{BenchmarkProgress, CategoryProgress, ScenarioEntry};

    /// One-scenario fixture for `bid`; `score` drives the engine, `stored_rank`
    /// fills the API `overall_rank` column so it can lie.
    fn mk_prog(score: f64, stored_rank: u32) -> BenchmarkProgress {
        BenchmarkProgress {
            benchmark_progress: score,
            overall_rank: stored_rank,
            categories: vec![(
                "Tracking".to_string(),
                CategoryProgress {
                    benchmark_progress: 0.0,
                    category_rank: 0,
                    rank_maxes: Vec::new(),
                    scenarios: vec![(
                        "s".to_string(),
                        ScenarioEntry {
                            score,
                            leaderboard_rank: 0,
                            scenario_rank: 0,
                            rank_maxes: vec![100.0, 200.0, 300.0, 400.0],
                            leaderboard_id: 0,
                        },
                    )],
                },
            )],
        }
    }

    #[test]
    fn families_group_difficulties_under_one_name() {
        let dir = std::env::temp_dir().join(format!("dashfam-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let store = crate::store::Store::open(&dir).unwrap();
        let id = "FAM1";
        let now = chrono::Utc::now();

        let registry = crate::registry::registry();
        let (def, _) = registry.by_id(644).unwrap();
        let family_name = def.name.clone();
        let ids: Vec<u64> = registry
            .all()
            .iter()
            .find(|b| b.name == def.name)
            .map(|b| {
                b.difficulties
                    .iter()
                    .map(|d| d.kovaaks_benchmark_id)
                    .collect()
            })
            .unwrap_or_default();
        assert!(ids.len() >= 2, "family has multiple difficulties");

        // 644 Easy: 500 → engine rank 4 (Silver); 645 Medium: 150 → rank 1.
        // Stored columns lie: 1 and 3 respectively.
        for (kid, score, stored) in [(ids[0], 500.0, 1u32), (ids[1], 150.0, 3u32)] {
            store.upsert_played(id, kid as i64, true, now).unwrap();
            store
                .record_snapshot(id, kid as i64, &mk_prog(score, stored), now)
                .unwrap();
        }
        let fams = dashboard_families(&store, id).unwrap();
        assert_eq!(
            fams.len(),
            1,
            "one family — got {:?}",
            fams.iter()
                .map(|f| f.benchmark_name.clone())
                .collect::<Vec<_>>()
        );
        let fam = fams
            .iter()
            .find(|f| f.benchmark_name == family_name)
            .unwrap();
        assert_eq!(fam.difficulties.len(), 2);
        let easy = fam
            .difficulties
            .iter()
            .find(|d| d.kovaaks_id == ids[0] as i64)
            .unwrap();
        assert_eq!(
            store
                .latest(id, ids[0] as i64)
                .unwrap()
                .unwrap()
                .overall_rank,
            1,
            "stored column lies"
        );
        assert_eq!(easy.current_rank, 4, "engine rank wins over stored 1");
        assert_eq!(easy.current_tier, "Silver");
        // Peak is the max engine rank across the family (4, not the tier-name
        // index lookup that would also have to know "Silver").
        assert_eq!(fam.peak_tier(), (4, "Silver".to_string()));
    }

    #[test]
    fn peak_tier_uses_max_engine_rank_not_name_lookup() {
        let tier_names: Vec<String> = ["Copper", "Bronze", "Iron", "Silver"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let diff = |rank: i64, tier: &str| FamilyDiff {
            difficulty_name: "Easy".into(),
            kovaaks_id: 1,
            current_rank: rank,
            current_tier: tier.to_string(),
            tier_names: tier_names.clone(),
            series: Vec::new(),
            benchmark_progress: 0,
        };
        let row = FamilyRow {
            benchmark_name: "Family".into(),
            color: "#000".into(),
            categories: Vec::new(),
            difficulties: vec![diff(1, "Copper"), diff(5, "Silver Complete")],
        };
        // "Silver Complete" is not in rank_colors; the old string-position
        // lookup dropped it and answered Copper.
        assert_eq!(row.peak_tier(), (5, "Silver Complete".to_string()));
    }
}
