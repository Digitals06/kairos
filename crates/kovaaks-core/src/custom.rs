//! User-defined benchmarks (v0.5.0).
//!
//! A custom benchmark is authored entirely by the player: a scenario list, a
//! threshold ladder per scenario, and a tier-name list. Nothing is inherited
//! from the KovaaK's registry or from a benchmark snapshot — if a custom bench
//! says "Gold starts at 1,200", that number came from its author.
//!
//! Scoring reuses the same primitive every registry benchmark uses
//! ([`crate::rankcalc::rank_of`]): a scenario's tier is the number of its
//! thresholds at or below its score. Two aggregation modes exist:
//!
//! - [`MODE_ALL`] — every scenario counts, so the bench rank is the *lowest*
//!   scenario tier (one lagging scenario holds the whole bench back);
//! - [`MODE_TOP_N`] — the bench rank is the highest tier reached by at least
//!   `top_n` scenarios (the dojo/count family's rule).
//!
//! Custom benchmarks carry negative ids in the rest of the app
//! (`benchmark_id = -store_rowid`), so they can never collide with KovaaK's
//! benchmark ids.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::rankcalc::{calc_count_required, rank_of};
use crate::store::CustomBenchRow;
use crate::types::{
    BenchmarkDef, BenchmarkProgress, CategoryProgress, Difficulty, RankTier, ScenarioEntry,
};

/// Every scenario must reach the tier for the bench to hold it.
pub const MODE_ALL: &str = "all";
/// `top_n` scenarios must reach the tier.
pub const MODE_TOP_N: &str = "top_n";

/// Upper bound on tiers per bench (the builder's palette has this many swatches).
pub const MAX_TIERS: usize = 12;

/// Tier colors, worst → best, drawn from the marble/basalt palette. Indexed by
/// tier position, so a 5-tier bench uses the first five.
pub const TIER_PALETTE: [&str; MAX_TIERS] = [
    "#8a8477", "#a8b8a0", "#b08d57", "#c9a227", "#d9c26a", "#b9b083", "#a8a4c9", "#9a7fb0",
    "#b06f8a", "#c26a5a", "#d9a05a", "#e6d3a3",
];

/// One scenario's resolved state inside a custom bench: score, tier, and the
/// gap to the next rung (what the detail table and the grind planner show).
#[derive(Debug, Clone, PartialEq)]
pub struct CustomScenarioState {
    pub scenario: String,
    pub category: String,
    pub score: f64,
    /// 0 = below the first threshold (unranked), `n` = `tier_names[n-1]` met.
    pub tier: u32,
    /// Next threshold to clear, `None` when the scenario is maxed.
    pub next_threshold: Option<f64>,
    /// Score delta needed for the next threshold, `None` when maxed.
    pub delta: Option<f64>,
}

/// A scored custom benchmark.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomRank {
    /// Bench rank index into `tier_names` (0 = unranked, `len` = top tier met).
    pub rank: u32,
    /// Every scenario sits at the top rung.
    pub complete: bool,
    /// Per-scenario rows in author order.
    pub scenarios: Vec<CustomScenarioState>,
}

/// Validate author input. Errors are written for the user (they surface
/// verbatim in the builder and on import).
pub fn validate(bench: &CustomBenchRow) -> Result<()> {
    let bad = |msg: String| Err(Error::InvalidBenchmark(msg));
    if bench.name.trim().is_empty() {
        return bad("Give the bench a name.".into());
    }
    if bench.mode != MODE_ALL && bench.mode != MODE_TOP_N {
        return bad(format!(
            "Unknown scoring mode \"{}\" (expected \"{MODE_ALL}\" or \"{MODE_TOP_N}\").",
            bench.mode
        ));
    }
    if bench.tier_names.is_empty() {
        return bad("Add at least one tier name.".into());
    }
    if bench.tier_names.len() > MAX_TIERS {
        return bad(format!("A bench can have at most {MAX_TIERS} tiers."));
    }
    if bench.tier_names.iter().any(|n| n.trim().is_empty()) {
        return bad("Every tier needs a name.".into());
    }
    if bench.scenarios.is_empty() {
        return bad("Add at least one scenario.".into());
    }
    if bench.mode == MODE_TOP_N {
        if bench.top_n == 0 {
            return bad("In top-N mode, N must be at least 1.".into());
        }
        if bench.top_n as usize > bench.scenarios.len() {
            return bad(format!(
                "N is {} but the bench only has {} scenarios.",
                bench.top_n,
                bench.scenarios.len()
            ));
        }
    }
    let mut seen: Vec<&str> = Vec::with_capacity(bench.scenarios.len());
    for (scenario, _, thresholds) in &bench.scenarios {
        if scenario.trim().is_empty() {
            return bad("A scenario row has no name.".into());
        }
        if seen.contains(&scenario.as_str()) {
            return bad(format!("\"{scenario}\" is listed twice."));
        }
        seen.push(scenario);
        if thresholds.len() != bench.tier_names.len() {
            return bad(format!(
                "\"{scenario}\" has {} thresholds but the bench has {} tiers.",
                thresholds.len(),
                bench.tier_names.len()
            ));
        }
        if thresholds.windows(2).any(|w| w[1] <= w[0]) {
            return bad(format!(
                "\"{scenario}\" thresholds must be strictly ascending."
            ));
        }
        if thresholds.iter().any(|t| *t < 0.0) {
            return bad(format!("\"{scenario}\" has a negative threshold."));
        }
    }
    Ok(())
}

/// Score a validated custom bench against the player's current scores.
///
/// Missing scores count as 0 (the scenario simply never helps the bench), so a
/// bench can be saved before every scenario has been played.
pub fn rank_custom(bench: &CustomBenchRow, scores: &HashMap<String, f64>) -> Result<CustomRank> {
    validate(bench)?;
    let states: Vec<CustomScenarioState> = bench
        .scenarios
        .iter()
        .map(|(scenario, category, thresholds)| {
            let score = scores.get(scenario).copied().unwrap_or(0.0);
            let tier = rank_of(score, thresholds).base;
            let next = thresholds.get(tier as usize).copied();
            CustomScenarioState {
                scenario: scenario.clone(),
                category: category.clone(),
                score,
                tier,
                next_threshold: next,
                delta: next.map(|t| t - score),
            }
        })
        .collect();

    let complete = states
        .iter()
        .all(|s| s.tier as usize == bench.tier_names.len());

    let rank = if bench.mode == MODE_TOP_N {
        let progress = to_progress(bench, &states);
        calc_count_required(&progress, bench.top_n as usize)
    } else {
        // `all` mode: the bench sits at its weakest scenario.
        states.iter().map(|s| s.tier).min().unwrap_or(0)
    };

    Ok(CustomRank {
        rank,
        complete,
        scenarios: states,
    })
}

/// Synthetic API-shaped progress so shared engine helpers (`calc_count_required`,
/// and the grind planner) can work on a custom bench unchanged.
pub fn to_progress(bench: &CustomBenchRow, states: &[CustomScenarioState]) -> BenchmarkProgress {
    let scenarios: Vec<(String, ScenarioEntry)> = states
        .iter()
        .map(|s| {
            let thresholds = bench
                .scenarios
                .iter()
                .find(|(name, _, _)| name == &s.scenario)
                .map(|(_, _, t)| t.clone())
                .unwrap_or_default();
            (
                s.scenario.clone(),
                ScenarioEntry {
                    score: s.score,
                    leaderboard_rank: 0,
                    scenario_rank: s.tier,
                    rank_maxes: thresholds,
                    leaderboard_id: 0,
                },
            )
        })
        .collect();
    let top = scenarios
        .iter()
        .map(|(_, e)| e.score)
        .fold(0.0f64, f64::max);
    BenchmarkProgress {
        benchmark_progress: top,
        overall_rank: 0,
        categories: vec![(
            "Custom".to_string(),
            CategoryProgress {
                benchmark_progress: top,
                category_rank: 0,
                rank_maxes: Vec::new(),
                scenarios,
            },
        )],
    }
}

/// Synthetic registry definition, so detail/grind code paths can treat a custom
/// bench like any registry benchmark. `kovaaks_benchmark_id` is 0: there is no
/// server benchmark behind it.
pub fn to_def(bench: &CustomBenchRow) -> BenchmarkDef {
    let subcategories: Vec<serde_json::Value> = bench
        .scenarios
        .iter()
        .map(|(scenario, _, _)| {
            serde_json::json!({
                "subcategoryName": scenario,
                "scenarioCount": 1,
                "color": bench.color,
            })
        })
        .collect();
    let categories = vec![serde_json::json!({
        "categoryName": "Custom",
        "color": bench.color,
        "subcategories": subcategories,
    })];
    let rank_colors = bench
        .tier_names
        .iter()
        .enumerate()
        .map(|(i, name)| RankTier {
            name: name.clone(),
            color: TIER_PALETTE
                .get(i)
                .copied()
                .unwrap_or(TIER_PALETTE[MAX_TIERS - 1])
                .to_string(),
        })
        .collect();
    BenchmarkDef {
        name: bench.name.clone(),
        abbreviation: bench.abbreviation.clone(),
        color: bench.color.clone(),
        rank_calculation: if bench.mode == MODE_TOP_N {
            "custom-top-n".to_string()
        } else {
            "custom-all".to_string()
        },
        spreadsheet_url: String::new(),
        difficulties: vec![Difficulty {
            name: "Custom".to_string(),
            kovaaks_benchmark_id: 0,
            sharecode: String::new(),
            rank_colors,
            categories,
            scenario_selection: None,
        }],
        hidden: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench(mode: &str, top_n: u32) -> CustomBenchRow {
        CustomBenchRow {
            id: 1,
            name: "Daily Five".into(),
            abbreviation: "D5".into(),
            color: "#b08d57".into(),
            mode: mode.into(),
            top_n,
            tier_names: vec!["Bronze".into(), "Silver".into(), "Gold".into()],
            scenarios: vec![
                ("A".into(), "Custom".into(), vec![100.0, 200.0, 300.0]),
                ("B".into(), "Custom".into(), vec![100.0, 200.0, 300.0]),
                ("C".into(), "Custom".into(), vec![100.0, 200.0, 300.0]),
                ("D".into(), "Custom".into(), vec![100.0, 200.0, 300.0]),
            ],
        }
    }

    fn scores(values: &[(&str, f64)]) -> HashMap<String, f64> {
        values
            .iter()
            .map(|(name, score)| ((*name).to_string(), *score))
            .collect()
    }

    #[test]
    fn all_mode_sits_at_the_weakest_scenario() {
        let r = rank_custom(
            &bench(MODE_ALL, 0),
            &scores(&[("A", 150.0), ("B", 250.0), ("C", 350.0), ("D", 50.0)]),
        )
        .expect("valid bench");
        assert_eq!(r.rank, 0, "D is below every threshold");
        assert!(!r.complete);
    }

    #[test]
    fn all_mode_ranks_by_the_floor_scenario() {
        let r = rank_custom(
            &bench(MODE_ALL, 0),
            &scores(&[("A", 150.0), ("B", 250.0), ("C", 350.0), ("D", 120.0)]),
        )
        .expect("valid bench");
        assert_eq!(r.rank, 1, "D clears Bronze only");
    }

    #[test]
    fn top_n_needs_n_scenarios_at_the_tier() {
        let r = rank_custom(
            &bench(MODE_TOP_N, 3),
            &scores(&[("A", 150.0), ("B", 250.0), ("C", 350.0)]),
        )
        .expect("valid bench");
        assert_eq!(
            r.rank, 1,
            "three scenarios clear Bronze, only two clear Silver"
        );
    }

    #[test]
    fn top_n_rank_rises_when_enough_scenarios_climb() {
        let r = rank_custom(
            &bench(MODE_TOP_N, 3),
            &scores(&[("A", 250.0), ("B", 250.0), ("C", 250.0), ("D", 50.0)]),
        )
        .expect("valid bench");
        assert_eq!(r.rank, 2, "three scenarios clear Silver");
    }

    #[test]
    fn a_score_above_the_top_threshold_is_complete() {
        let r = rank_custom(
            &bench(MODE_ALL, 0),
            &scores(&[("A", 400.0), ("B", 400.0), ("C", 400.0), ("D", 400.0)]),
        )
        .expect("valid bench");
        assert_eq!(r.rank, 3);
        assert!(r.complete);
        assert!(r.scenarios.iter().all(|s| s.next_threshold.is_none()));
    }

    #[test]
    fn unplayed_scenarios_report_zero_and_a_full_gap() {
        let r = rank_custom(&bench(MODE_ALL, 0), &scores(&[("A", 150.0)])).expect("valid bench");
        let d = r
            .scenarios
            .iter()
            .find(|s| s.scenario == "D")
            .expect("row d");
        assert_eq!(d.score, 0.0);
        assert_eq!(d.tier, 0);
        assert_eq!(d.next_threshold, Some(100.0));
        assert_eq!(d.delta, Some(100.0));
    }

    #[test]
    fn validation_rejects_mismatched_threshold_counts() {
        let mut b = bench(MODE_ALL, 0);
        b.scenarios[0].2 = vec![100.0, 200.0];
        let err = validate(&b).expect_err("count mismatch must be rejected");
        assert!(matches!(err, Error::InvalidBenchmark(ref m) if m.contains("thresholds")));
    }

    #[test]
    fn validation_rejects_non_ascending_thresholds() {
        let mut b = bench(MODE_ALL, 0);
        b.scenarios[1].2 = vec![200.0, 100.0, 300.0];
        let err = validate(&b).expect_err("descending ladder must be rejected");
        assert!(matches!(err, Error::InvalidBenchmark(ref m) if m.contains("ascending")));
    }

    #[test]
    fn validation_rejects_duplicates_and_unknown_modes() {
        let mut dup = bench(MODE_ALL, 0);
        dup.scenarios[1].0 = "A".into();
        assert!(validate(&dup).is_err());

        let mut mode = bench("harmonic", 0);
        mode.mode = "harmonic".into();
        assert!(validate(&mode).is_err());
    }

    #[test]
    fn validation_rejects_top_n_above_the_pool() {
        let b = bench(MODE_TOP_N, 9);
        let err = validate(&b).expect_err("N > pool must be rejected");
        assert!(matches!(err, Error::InvalidBenchmark(ref m) if m.contains("only has")));
    }

    #[test]
    fn to_def_mirrors_tier_names_into_rank_colors() {
        let b = bench(MODE_TOP_N, 2);
        let def = to_def(&b);
        assert_eq!(def.rank_calculation, "custom-top-n");
        assert_eq!(def.difficulties.len(), 1);
        let diff = &def.difficulties[0];
        assert_eq!(diff.rank_colors.len(), b.tier_names.len());
        assert_eq!(diff.rank_colors[0].name, "Bronze");
        assert_eq!(diff.kovaaks_benchmark_id, 0);
        assert_eq!(
            diff.categories[0]["subcategories"]
                .as_array()
                .unwrap()
                .len(),
            b.scenarios.len()
        );
    }

    #[test]
    fn to_progress_feeds_the_shared_count_rule() {
        let b = bench(MODE_TOP_N, 3);
        let r = rank_custom(
            &b,
            &scores(&[("A", 150.0), ("B", 250.0), ("C", 350.0), ("D", 50.0)]),
        )
        .expect("valid bench");
        let progress = to_progress(&b, &r.scenarios);
        let scenarios: Vec<_> = progress.categories[0].1.scenarios.iter().collect();
        assert_eq!(scenarios.len(), 4);
        assert_eq!(scenarios[0].1.scenario_rank, 1);
        assert_eq!(scenarios[0].1.rank_maxes, vec![100.0, 200.0, 300.0]);
    }
}
