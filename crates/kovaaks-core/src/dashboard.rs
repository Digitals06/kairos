//! Dashboard rollups (`v0.2.0` money shot): per-category banking sections,
//! each bundling the user's benchmarks with their overall-rank tier history
//! over time — 100% local snapshot data, no API fallbacks.

use crate::store::Store;

/// One benchmark's rank history inside a category section.
#[derive(Debug, Clone, PartialEq)]
pub struct RankHistoryRow {
    pub benchmark_id: i64,
    pub benchmark_name: String,
    /// 0-based tier index of the overall rank (−1 = unranked).
    pub current_rank: i64,
    pub current_tier: String,
    /// The benchmark's tier ladder names (coloring + next-tier labels).
    pub tier_names: Vec<String>,
    /// (captured_at RFC3339, tier name) collapsed to tier changes only.
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
        let tier_names: Vec<String> = diff
            .rank_colors
            .iter()
            .map(|r| r.name.to_string())
            .collect();
        let tier_at = |idx: i64| -> String {
            usize::try_from(idx)
                .ok()
                .and_then(|i| tier_names.get(i).cloned())
                .unwrap_or_else(|| "—".to_string())
        };
        let history = store.history(steam_id, bid)?;
        let mut series: Vec<(String, String)> = history
            .iter()
            .map(|s| (s.captured_at.to_rfc3339(), tier_at(s.overall_rank)))
            .collect();
        series.dedup_by(|a, b| a.1 == b.1);
        let current_tier = tier_at(latest_snapshot.overall_rank);
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
                current_rank: latest_snapshot.overall_rank,
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

    fn prog(bids_scen: &[(&str, &str)]) -> BenchmarkProgress {
        let mut categories: Vec<(String, CategoryProgress)> = Vec::new();
        for (cat, scen) in bids_scen {
            let entry = categories.iter_mut().find(|(c, _)| c == cat);
            match entry {
                Some((_, cp)) => cp.scenarios.push((
                    scen.to_string(),
                    ScenarioEntry {
                        score: 0.0,
                        leaderboard_rank: 0,
                        scenario_rank: 0,
                        rank_maxes: Vec::new(),
                        leaderboard_id: 0,
                    },
                )),
                None => categories.push((
                    cat.to_string(),
                    CategoryProgress {
                        benchmark_progress: 0.0,
                        category_rank: 1,
                        rank_maxes: Vec::new(),
                        scenarios: vec![(
                            scen.to_string(),
                            ScenarioEntry {
                                score: 0.0,
                                leaderboard_rank: 0,
                                scenario_rank: 0,
                                rank_maxes: Vec::new(),
                                leaderboard_id: 0,
                            },
                        )],
                    },
                )),
            }
        }
        BenchmarkProgress {
            benchmark_progress: 0.0,
            overall_rank: 2,
            categories,
        }
    }

    #[test]
    fn sections_group_by_category_and_collapse_tiers() {
        let dir = std::env::temp_dir().join(format!("dash-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let store = crate::store::Store::open(&dir).unwrap();
        let now = chrono::Utc::now();
        let id = "SID1";
        store.upsert_played(id, 644, true, now).unwrap();
        // bench 460: two snapshots same rank then a change; categories TRACKING.
        store
            .record_snapshot(
                id,
                644,
                &prog(&[("TRACKING", "s1"), ("CLICKING", "click")]),
                now - chrono::Duration::days(5),
            )
            .unwrap();
        store
            .record_snapshot(
                id,
                644,
                &prog(&[("TRACKING", "s1"), ("CLICKING", "click")]),
                now - chrono::Duration::days(4),
            )
            .unwrap();
        store
            .record_snapshot(
                id,
                644,
                &prog_rank(&[("TRACKING", "s1"), ("CLICKING", "click")], 3),
                now - chrono::Duration::days(2),
            )
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
        // tiers collapse: first two snapshots share rank 2 → one entry; then a change.
        assert!(row.series.len() <= 2, "series {:?}", row.series);
        assert!(
            row.current_tier != "—",
            "tier must resolve, got {}",
            row.current_tier
        );
        let _ = std::fs::remove_file(&dir);
    }

    fn prog_rank(cats: &[(&str, &str)], rank: u32) -> BenchmarkProgress {
        let mut p = prog(cats);
        p.overall_rank = rank;
        p
    }
}
