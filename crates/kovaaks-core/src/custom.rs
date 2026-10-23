//! User-defined benchmarks (v0.5.0).
//!
//! A custom benchmark is authored entirely by the player: a scenario list,
//! author-supplied thresholds per scenario, tier names, and a grouping of
//! scenarios into subcategories plus one of three aggregation rules. Nothing is
//! inherited from the KovaaK's registry or from a benchmark snapshot: if a
//! custom bench says "Gold starts at 1,200", that number came from its author.
//!
//! Scoring builds on the primitive every registry benchmark uses
//! ([`crate::rankcalc::rank_of`]): a scenario's tier is the number of its
//! thresholds at or below its score, and `PreciseRank::precise` adds the
//! fraction of the way through the current band.
//!
//! Rules (each subcategory is scored, then the bench takes its weakest one):
//!
//! - [`RULE_ALL`] — every scenario in a subcategory counts, so the subcategory
//!   sits at its **lowest** scenario tier;
//! - [`RULE_TOP_N`] — only the best `top_n` scenarios of each subcategory
//!   count, and the subcategory sits at the weakest of those;
//! - [`RULE_ENERGY`] — Voltaic-style: each subcategory contributes its best
//!   scenario's precise tier and the bench is the **harmonic mean** across
//!   subcategories, so a lagging subcategory drags the whole bench.
//!
//! Custom benchmarks carry negative ids in the rest of the app
//! (`benchmark_id = -store_rowid`), so they can never collide with KovaaK's
//! benchmark ids.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::rankcalc::{harmonic_strict, rank_of};
use crate::store::{CustomBenchRow, CustomScenarioRow};
use crate::types::{
    BenchmarkDef, BenchmarkProgress, CategoryProgress, Difficulty, RankTier, ScenarioEntry,
};

/// Every scenario in the subcategory counts.
pub const RULE_ALL: &str = "all";
/// Only the best `top_n` scenarios per subcategory count.
pub const RULE_TOP_N: &str = "top_n";
/// Harmonic mean of each subcategory's best scenario (Voltaic energy shape).
pub const RULE_ENERGY: &str = "energy";

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
    pub subcategory: String,
    pub score: f64,
    /// 0 = below the first threshold (unranked), `n` = `tier_names[n-1]` met.
    pub tier: u32,
    /// `tier` plus the fraction of the way through the current band, clamped to
    /// `tier_names.len()` — the energy axis and the live progress readout.
    pub precise: f64,
    /// Next threshold to clear, `None` when the scenario is maxed.
    pub next_threshold: Option<f64>,
    /// Score delta needed for the next threshold, `None` when maxed.
    pub delta: Option<f64>,
}

/// How one subcategory scored.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomSubcategoryState {
    pub subcategory: String,
    /// The scenarios that actually counted (all of them in `all` mode, the top
    /// N in `top_n`, the best one in `energy`).
    pub counted: Vec<String>,
    /// Subcategory tier (the floor of `energy` in energy mode).
    pub tier: u32,
    /// Subcategory energy: the best precise tier (energy mode) or the weakest
    /// counted tier (the other rules).
    pub energy: f64,
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
    /// Per-subcategory rows in author order.
    pub subcategories: Vec<CustomSubcategoryState>,
    /// Bench-level energy (harmonic mean of the subcategory energies).
    pub energy: f64,
}

/// Validate author input. Errors are written for the user (they surface
/// verbatim in the builder and on import).
pub fn validate(bench: &CustomBenchRow) -> Result<()> {
    let bad = |msg: String| Err(Error::InvalidBenchmark(msg));
    if bench.name.trim().is_empty() {
        return bad("Give the bench a name.".into());
    }
    if bench.rule != RULE_ALL && bench.rule != RULE_TOP_N && bench.rule != RULE_ENERGY {
        return bad(format!(
            "Unknown scoring rule \"{}\" (expected \"{RULE_ALL}\", \"{RULE_TOP_N}\" or \"{RULE_ENERGY}\").",
            bench.rule
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
    if bench.rule == RULE_TOP_N {
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
    for CustomScenarioRow {
        scenario,
        subcategory,
        thresholds,
        ..
    } in &bench.scenarios
    {
        if scenario.trim().is_empty() {
            return bad("A scenario row has no name.".into());
        }
        if subcategory.trim().is_empty() {
            return bad(format!("\"{scenario}\" needs a subcategory."));
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

/// Subcategory names in first-appearance order (the author's order).
pub fn subcategory_order(bench: &CustomBenchRow) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for row in &bench.scenarios {
        if !out.iter().any(|s| s == &row.subcategory) {
            out.push(row.subcategory.clone());
        }
    }
    out
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
        .map(|row| {
            let score = scores.get(&row.scenario).copied().unwrap_or(0.0);
            let precise_rank = rank_of(score, &row.thresholds);
            let tier = precise_rank.base;
            let next = row.thresholds.get(tier as usize).copied();
            let clamp = bench.tier_names.len() as f64;
            CustomScenarioState {
                scenario: row.scenario.clone(),
                category: row.category.clone(),
                subcategory: row.subcategory.clone(),
                score,
                tier,
                precise: precise_rank.precise.min(clamp),
                next_threshold: next,
                delta: next.map(|t| t - score),
            }
        })
        .collect();

    let complete = states
        .iter()
        .all(|s| s.tier as usize == bench.tier_names.len());

    let mut subcategories: Vec<CustomSubcategoryState> = Vec::new();
    for name in subcategory_order(bench) {
        let members: Vec<&CustomScenarioState> =
            states.iter().filter(|s| s.subcategory == name).collect();
        if members.is_empty() {
            continue;
        }
        let (tier, energy, counted) = match bench.rule.as_str() {
            RULE_ENERGY => {
                // Voltaic shape: the subcategory is carried by its best
                // scenario, and the bench harmonic-means across subcategories.
                let best = members
                    .iter()
                    .max_by(|a, b| {
                        a.precise
                            .partial_cmp(&b.precise)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .expect("non-empty subcategory");
                (
                    best.precise.floor() as u32,
                    best.precise,
                    vec![best.scenario.clone()],
                )
            }
            RULE_TOP_N => {
                // Only the best N of the subcategory count; the weakest of
                // those holds the subcategory back (a 1-scenario subcategory
                // with N=2 simply uses its only scenario).
                let mut tiers: Vec<u32> = members.iter().map(|s| s.tier).collect();
                tiers.sort_unstable_by(|a, b| b.cmp(a));
                let take = (bench.top_n as usize).min(tiers.len());
                let held = tiers[take - 1];
                let counted: Vec<String> = members
                    .iter()
                    .filter(|s| s.tier >= held)
                    .take(take)
                    .map(|s| s.scenario.clone())
                    .collect();
                (held, held as f64, counted)
            }
            _ => {
                // Every scenario counts: the subcategory sits at its floor.
                let held = members.iter().map(|s| s.tier).min().unwrap_or(0);
                (
                    held,
                    held as f64,
                    members.iter().map(|s| s.scenario.clone()).collect(),
                )
            }
        };
        subcategories.push(CustomSubcategoryState {
            subcategory: name,
            counted,
            tier,
            energy,
        });
    }

    // Bench rank: its weakest subcategory, except energy which harmonic-means.
    let (rank, energy) = if bench.rule == RULE_ENERGY {
        let energies: Vec<f64> = subcategories.iter().map(|s| s.energy).collect();
        let mean = harmonic_strict(&energies).clamp(0.0, bench.tier_names.len() as f64);
        (mean.floor() as u32, mean)
    } else {
        let weakest = subcategories.iter().map(|s| s.tier).min().unwrap_or(0);
        (weakest, weakest as f64)
    };

    Ok(CustomRank {
        rank,
        complete,
        scenarios: states,
        subcategories,
        energy,
    })
}

/// Synthetic API-shaped progress so shared engine helpers and the detail view
/// can treat a custom bench like any registry benchmark: one category per
/// subcategory, scenarios in author order.
pub fn to_progress(bench: &CustomBenchRow, states: &[CustomScenarioState]) -> BenchmarkProgress {
    let mut categories: Vec<(String, CategoryProgress)> = Vec::new();
    for sub in subcategory_order(bench) {
        let scenarios: Vec<(String, ScenarioEntry)> = states
            .iter()
            .filter(|s| s.subcategory == sub)
            .map(|s| {
                let thresholds = bench
                    .scenarios
                    .iter()
                    .find(|row| row.scenario == s.scenario)
                    .map(|row| row.thresholds.clone())
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
        categories.push((
            sub,
            CategoryProgress {
                benchmark_progress: top,
                category_rank: 0,
                rank_maxes: Vec::new(),
                scenarios,
            },
        ));
    }
    BenchmarkProgress {
        benchmark_progress: 0.0,
        overall_rank: 0,
        categories,
    }
}

/// Synthetic registry definition, so detail/grind code paths can treat a custom
/// bench like any registry benchmark. `kovaaks_benchmark_id` is 0: there is no
/// server benchmark behind it.
pub fn to_def(bench: &CustomBenchRow) -> BenchmarkDef {
    let subcategories: Vec<serde_json::Value> = subcategory_order(bench)
        .into_iter()
        .map(|name| {
            let count = bench
                .scenarios
                .iter()
                .filter(|row| row.subcategory == name)
                .count();
            serde_json::json!({
                "subcategoryName": name,
                "scenarioCount": count,
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
        rank_calculation: format!("custom-{}", bench.rule),
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

    fn row(scenario: &str, subcategory: &str, thresholds: Vec<f64>) -> CustomScenarioRow {
        CustomScenarioRow {
            scenario: scenario.into(),
            category: "Custom".into(),
            subcategory: subcategory.into(),
            thresholds,
        }
    }

    /// Four scenarios in two subcategories: A/B in "Clicking", C/D in "Tracking".
    fn bench(rule: &str, top_n: u32) -> CustomBenchRow {
        CustomBenchRow {
            id: 1,
            name: "Daily Five".into(),
            abbreviation: "D5".into(),
            color: "#b08d57".into(),
            rule: rule.into(),
            top_n,
            tier_names: vec!["Bronze".into(), "Silver".into(), "Gold".into()],
            scenarios: vec![
                row("A", "Clicking", vec![100.0, 200.0, 300.0]),
                row("B", "Clicking", vec![100.0, 200.0, 300.0]),
                row("C", "Tracking", vec![100.0, 200.0, 300.0]),
                row("D", "Tracking", vec![100.0, 200.0, 300.0]),
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
    fn all_rule_takes_the_weakest_scenario_of_each_subcategory() {
        let r = rank_custom(
            &bench(RULE_ALL, 0),
            &scores(&[("A", 350.0), ("B", 150.0), ("C", 250.0), ("D", 250.0)]),
        )
        .expect("valid bench");
        // Clicking floor = Bronze (B), Tracking floor = Silver (C/D).
        assert_eq!(r.rank, 1);
        assert!(!r.complete);
        let clicking = r
            .subcategories
            .iter()
            .find(|s| s.subcategory == "Clicking")
            .expect("clicking");
        assert_eq!(clicking.tier, 1);
        assert_eq!(clicking.counted.len(), 2);
    }

    #[test]
    fn all_rule_is_zero_when_a_subcategory_has_no_scores() {
        let r = rank_custom(&bench(RULE_ALL, 0), &scores(&[("A", 350.0), ("B", 350.0)]))
            .expect("valid bench");
        assert_eq!(r.rank, 0, "the Tracking subcategory has no scores");
    }

    #[test]
    fn top_n_per_subcategory_uses_the_nth_best() {
        // Clicking: A=350 (Gold), B=150 (Bronze) → top 1 = Gold.
        // Tracking: C=250 (Silver), D=50 (0) → top 1 = Silver.
        let r = rank_custom(
            &bench(RULE_TOP_N, 1),
            &scores(&[("A", 350.0), ("B", 150.0), ("C", 250.0), ("D", 50.0)]),
        )
        .expect("valid bench");
        assert_eq!(
            r.rank, 2,
            "the weakest subcategory (Tracking, Silver) decides"
        );
        assert_eq!(r.subcategories[0].tier, 3, "Clicking's best is Gold");
        assert_eq!(r.subcategories[0].counted, vec!["A".to_string()]);
    }

    #[test]
    fn top_n_uses_the_weakest_of_the_counted_scenarios() {
        // Clicking with N=2: A=350 (Gold) and B=250 (Silver) → Silver.
        let r = rank_custom(
            &bench(RULE_TOP_N, 2),
            &scores(&[("A", 350.0), ("B", 250.0), ("C", 250.0), ("D", 250.0)]),
        )
        .expect("valid bench");
        assert_eq!(r.subcategories[0].tier, 2);
        assert_eq!(r.rank, 2);
    }

    #[test]
    fn energy_is_harmonic_across_subcategories() {
        // Clicking best = A at 350 → maxed, precise clamped to 3.0;
        // Tracking best = C at 250 → 2 + (250-200)/(300-200) = 2.5.
        // harmonic_strict([3, 2.5]) = 2/(1/3 + 1/2.5) = 2.727… → floor 2.
        let r = rank_custom(
            &bench(RULE_ENERGY, 0),
            &scores(&[("A", 350.0), ("B", 10.0), ("C", 250.0), ("D", 10.0)]),
        )
        .expect("valid bench");
        assert_eq!(r.rank, 2);
        assert!(
            (r.energy - 2.727272727272727).abs() < 1e-9,
            "energy was {}",
            r.energy
        );
        assert_eq!(r.subcategories[0].tier, 3, "Clicking carried by its best");
    }

    #[test]
    fn energy_ignores_weak_scenarios_inside_a_subcategory() {
        let r = rank_custom(
            &bench(RULE_ENERGY, 0),
            &scores(&[("A", 350.0), ("B", 0.0), ("C", 350.0), ("D", 0.0)]),
        )
        .expect("valid bench");
        assert_eq!(r.rank, 3, "the best scenario per subcategory is enough");
    }

    #[test]
    fn a_partial_band_lifts_energy_but_not_the_tier() {
        // A sits halfway between Silver and Gold (250 on a 200/300 band).
        let r = rank_custom(
            &bench(RULE_ENERGY, 0),
            &scores(&[("A", 250.0), ("B", 0.0), ("C", 250.0), ("D", 0.0)]),
        )
        .expect("valid bench");
        assert_eq!(r.subcategories[0].tier, 2);
        assert!((r.energy - 2.5).abs() < 1e-9);
        assert_eq!(r.rank, 2);
    }

    #[test]
    fn a_score_above_the_top_threshold_is_complete() {
        let r = rank_custom(
            &bench(RULE_ALL, 0),
            &scores(&[("A", 400.0), ("B", 400.0), ("C", 400.0), ("D", 400.0)]),
        )
        .expect("valid bench");
        assert_eq!(r.rank, 3);
        assert!(r.complete);
        assert!(r.scenarios.iter().all(|s| s.next_threshold.is_none()));
    }

    #[test]
    fn unplayed_scenarios_report_zero_and_a_full_gap() {
        let r = rank_custom(&bench(RULE_ALL, 0), &scores(&[("A", 150.0)])).expect("valid bench");
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
        let mut b = bench(RULE_ALL, 0);
        b.scenarios[0].thresholds = vec![100.0, 200.0];
        let err = validate(&b).expect_err("count mismatch must be rejected");
        assert!(matches!(err, Error::InvalidBenchmark(ref m) if m.contains("thresholds")));
    }

    #[test]
    fn validation_rejects_non_ascending_thresholds() {
        let mut b = bench(RULE_ALL, 0);
        b.scenarios[1].thresholds = vec![200.0, 100.0, 300.0];
        let err = validate(&b).expect_err("descending ladder must be rejected");
        assert!(matches!(err, Error::InvalidBenchmark(ref m) if m.contains("ascending")));
    }

    #[test]
    fn validation_rejects_duplicates_unknown_rules_and_missing_subcategories() {
        let mut dup = bench(RULE_ALL, 0);
        dup.scenarios[1].scenario = "A".into();
        assert!(validate(&dup).is_err());

        let mut unknown = bench("harmonic", 0);
        unknown.rule = "harmonic".into();
        assert!(validate(&unknown).is_err());

        let mut no_sub = bench(RULE_ALL, 0);
        no_sub.scenarios[0].subcategory = "  ".into();
        let err = validate(&no_sub).expect_err("blank subcategory must be rejected");
        assert!(matches!(err, Error::InvalidBenchmark(ref m) if m.contains("subcategory")));
    }

    #[test]
    fn validation_rejects_top_n_above_the_pool() {
        let b = bench(RULE_TOP_N, 9);
        let err = validate(&b).expect_err("N > pool must be rejected");
        assert!(matches!(err, Error::InvalidBenchmark(ref m) if m.contains("only has")));
    }

    #[test]
    fn subcategory_order_follows_first_appearance() {
        let b = bench(RULE_ALL, 0);
        assert_eq!(subcategory_order(&b), vec!["Clicking", "Tracking"]);
    }

    #[test]
    fn to_def_groups_scenarios_under_their_subcategories() {
        let b = bench(RULE_TOP_N, 2);
        let def = to_def(&b);
        assert_eq!(def.rank_calculation, "custom-top_n");
        assert_eq!(def.difficulties.len(), 1);
        let diff = &def.difficulties[0];
        assert_eq!(diff.rank_colors.len(), b.tier_names.len());
        assert_eq!(diff.rank_colors[0].name, "Bronze");
        assert_eq!(diff.kovaaks_benchmark_id, 0);
        let subs = diff.categories[0]["subcategories"].as_array().unwrap();
        assert_eq!(subs.len(), 2, "two subcategories, not four scenarios");
        assert_eq!(subs[0]["subcategoryName"], "Clicking");
        assert_eq!(subs[0]["scenarioCount"], 2);
    }

    #[test]
    fn to_progress_emits_one_category_per_subcategory() {
        let b = bench(RULE_TOP_N, 2);
        let r = rank_custom(
            &b,
            &scores(&[("A", 350.0), ("B", 250.0), ("C", 150.0), ("D", 50.0)]),
        )
        .expect("valid bench");
        let progress = to_progress(&b, &r.scenarios);
        assert_eq!(progress.categories.len(), 2);
        assert_eq!(progress.categories[0].0, "Clicking");
        assert_eq!(progress.categories[0].1.scenarios.len(), 2);
        assert_eq!(progress.categories[1].0, "Tracking");
        assert_eq!(progress.categories[1].1.scenarios[0].1.scenario_rank, 1);
    }
}
