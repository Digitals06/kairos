//! Rank tier resolution (plan Task 1.8).
//!
//! Maps progress values and 1-based scenario rank indices onto the
//! difficulty's `rank_colors` ladder (worst tier first, e.g. VT S5 Novice:
//! Iron #999999 → Bronze #FF9900 → Silver #CBD9E6 → Gold #CAB148).
//!
//! `rank_for` intentionally *ignores* any `live_rank_maxes` entries beyond
//! the ladder length (warn + clamp) and clamps progress beyond the last
//! threshold to the last tier.

use crate::types::{Difficulty, RankTier};

/// Resolve the rank tier for `progress` against a live ascending-threshold
/// ladder (`live_rank_maxes`) zipped by index into the difficulty's
/// `rank_colors`.
///
/// - `None` when the difficulty has no tiers or the ladder is empty and the
///   difficulty has no tiers.
/// - Below the first threshold → the first tier (e.g. Iron).
/// - Beyond the last threshold → the last tier.
/// - `live_rank_maxes` longer than `rank_colors` logs a warning (once per
///   call) and clamps to the last color; extra thresholds still count toward
///   tier selection (they can only push progress deeper into the ladder).
pub fn rank_for(
    progress: i64,
    live_rank_maxes: &[i64],
    difficulty: &Difficulty,
) -> Option<RankTier> {
    let tiers = &difficulty.rank_colors;
    if tiers.is_empty() {
        return None;
    }
    if live_rank_maxes.len() > tiers.len() {
        eprintln!(
            "rank ladder mismatch: {} thresholds > {} tiers for difficulty '{}' — clamping to last tier",
            live_rank_maxes.len(),
            tiers.len(),
            difficulty.name
        );
    }
    if live_rank_maxes.is_empty() {
        // No thresholds at all: progress maps onto the base tier.
        return Some(tiers[0].clone());
    }
    // Largest i with progress >= live_rank_maxes[i]; thresholds are ascending.
    let mut idx = 0usize;
    for (i, &max) in live_rank_maxes.iter().enumerate() {
        if progress >= max && i < tiers.len() {
            idx = i;
        }
    }
    Some(tiers[idx].clone())
}

/// Resolve the tier for a 1-based `scenario_rank` index (as reported by the
/// webapp-backend payloads; `0` means unplayed). Returns `None` for `0`,
/// out-of-range indices, or a difficulty without tiers.
pub fn scenario_rank_tier(scenario_rank_idx: usize, difficulty: &Difficulty) -> Option<RankTier> {
    if scenario_rank_idx == 0 {
        return None;
    }
    difficulty.rank_colors.get(scenario_rank_idx - 1).cloned()
}

/// Resolve the rank tier for a 1-based API rank index (the payload's
/// `overall_rank` / `category_rank` / `scenario_rank` fields, `0` = unplayed).
///
/// The KovaaK's webapp-backend computes ranks with each benchmark's own
/// rules server-side (`rankCalculation`: basic average, vt-energy, ...), so
/// the index is authoritative — tiers map by position into the difficulty's
/// `rank_colors` ladder. Threshold recomputation is deliberately NOT used:
/// ladders differ per benchmark and per difficulty.
pub fn rank_from_index(api_rank: u32, difficulty: &Difficulty) -> Option<RankTier> {
    if api_rank == 0 {
        return None;
    }
    difficulty.rank_colors.get(api_rank as usize - 1).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RankTier;

    fn tier(name: &str, color: &str) -> RankTier {
        RankTier {
            name: name.to_string(),
            color: color.to_string(),
        }
    }

    /// Verified VT S5 Novice ladder (registry ground truth).
    fn vt_novice() -> Difficulty {
        Difficulty {
            name: "Novice".into(),
            kovaaks_benchmark_id: 459,
            sharecode: "KovaaKsExample".into(),
            rank_colors: vec![
                tier("Iron", "#999999"),
                tier("Bronze", "#FF9900"),
                tier("Silver", "#CBD9E6"),
                tier("Gold", "#CAB148"),
            ],
            categories: vec![],
        }
    }

    /// Fixture ladder for the plan's sample mapping (0→Iron, 15_000→Bronze,
    /// 60_000→Gold under "largest i where progress >= maxes[i] → tier[i]":
    /// 15_000 must sit at index 1, so the ladder starts at 10_000).
    fn vt_thresholds() -> Vec<i64> {
        vec![10_000, 15_000, 30_000, 60_000]
    }

    #[test]
    fn zero_progress_is_iron() {
        let d = vt_novice();
        let t = rank_for(0, &vt_thresholds(), &d).expect("tier");
        assert_eq!(t.name, "Iron");
        assert_eq!(t.color, "#999999");
    }

    #[test]
    fn fifteen_thousand_is_bronze() {
        let d = vt_novice();
        // 15,000 >= thresholds[1] → 2nd tier = Bronze.
        let t = rank_for(15_000, &vt_thresholds(), &d).expect("tier");
        assert_eq!(t.name, "Bronze");
        assert_eq!(t.color, "#FF9900");
        // Just below the threshold stays Iron.
        assert_eq!(rank_for(14_999, &vt_thresholds(), &d).unwrap().name, "Iron");
        // Between threshold[1] and threshold[2] → still Bronze.
        assert_eq!(
            rank_for(15_100, &vt_thresholds(), &d).unwrap().name,
            "Bronze"
        );
        assert_eq!(
            rank_for(29_999, &vt_thresholds(), &d).unwrap().name,
            "Bronze"
        );
    }

    #[test]
    fn sixty_thousand_is_gold() {
        let d = vt_novice();
        let t = rank_for(60_000, &vt_thresholds(), &d).expect("tier");
        assert_eq!(t.name, "Gold");
        assert_eq!(t.color, "#CAB148");
        // Mid-ladder boundary: exactly 30,000 → Silver (threshold[2]).
        assert_eq!(
            rank_for(30_000, &vt_thresholds(), &d).unwrap().name,
            "Silver"
        );
        assert_eq!(
            rank_for(59_999, &vt_thresholds(), &d).unwrap().name,
            "Silver"
        );
    }

    #[test]
    fn beyond_ladder_clamps_to_last_tier() {
        let d = vt_novice();
        let t = rank_for(999_999, &vt_thresholds(), &d).expect("tier");
        assert_eq!(t.name, "Gold");
        assert_eq!(t.color, "#CAB148");
    }

    #[test]
    fn below_first_threshold_is_first_tier() {
        let d = vt_novice();
        for progress in [-100i64, 1, 9_999] {
            let t = rank_for(progress, &vt_thresholds(), &d).expect("tier");
            assert_eq!(t.name, "Iron", "progress {progress} must be Iron");
        }
    }

    #[test]
    fn more_thresholds_than_tiers_clamps_without_panic() {
        let d = vt_novice();
        let long_ladder = [1_000i64, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000];
        let t = rank_for(6_500, &long_ladder, &d).expect("tier");
        // Indices 4+ are clamped onto the last color (Gold).
        assert_eq!(t.name, "Gold");
        assert_eq!(t.color, "#CAB148");
        // Indices inside the 4-tier window still resolve normally:
        // 3_500 >= thresholds[2] → 3rd tier = Silver.
        assert_eq!(rank_for(3_500, &long_ladder, &d).unwrap().name, "Silver");
        assert_eq!(rank_for(2_500, &long_ladder, &d).unwrap().name, "Bronze");
        assert_eq!(rank_for(1_500, &long_ladder, &d).unwrap().name, "Iron");
    }

    #[test]
    fn fewer_thresholds_than_tiers_resolves_within_range() {
        let d = vt_novice();
        let short_ladder = [10_000i64, 20_000];
        assert_eq!(rank_for(5_000, &short_ladder, &d).unwrap().name, "Iron");
        assert_eq!(rank_for(15_000, &short_ladder, &d).unwrap().name, "Iron");
        assert_eq!(rank_for(20_000, &short_ladder, &d).unwrap().name, "Bronze");
        assert_eq!(rank_for(99_999, &short_ladder, &d).unwrap().name, "Bronze");
    }

    #[test]
    fn empty_inputs_yield_none() {
        let d = vt_novice();
        let no_tiers = Difficulty {
            rank_colors: vec![],
            ..vt_novice()
        };
        assert!(rank_for(1_000, &vt_thresholds(), &no_tiers).is_none());
        assert!(scenario_rank_tier(1, &no_tiers).is_none());
        // Empty ladder: below-first-threshold semantics → first tier.
        assert_eq!(rank_for(0, &[], &d).unwrap().name, "Iron");
    }

    #[test]
    fn scenario_rank_tier_maps_one_based_indices() {
        let d = vt_novice();
        assert!(scenario_rank_tier(0, &d).is_none(), "0 = unplayed");
        assert_eq!(scenario_rank_tier(1, &d).unwrap(), tier("Iron", "#999999"));
        assert_eq!(
            scenario_rank_tier(2, &d).unwrap(),
            tier("Bronze", "#FF9900")
        );
        assert_eq!(scenario_rank_tier(4, &d).unwrap(), tier("Gold", "#CAB148"));
        assert!(scenario_rank_tier(5, &d).is_none(), "beyond ladder");
        assert!(scenario_rank_tier(99, &d).is_none(), "way beyond ladder");
    }

    #[test]
    fn rank_from_index_matches_api_semantics() {
        let d = vt_novice();
        assert!(rank_from_index(0, &d).is_none(), "0 = unplayed");
        assert_eq!(rank_from_index(1, &d).unwrap().name, "Iron");
        assert_eq!(rank_from_index(4, &d).unwrap(), tier("Gold", "#CAB148"));
        assert!(rank_from_index(5, &d).is_none(), "beyond ladder");
    }

    #[test]
    fn normalize_scale_converts_centi_to_display() {
        use crate::types::{BenchmarkProgress, CategoryProgress, ScenarioEntry};
        let cats = vec![(
            "Clicking".to_string(),
            CategoryProgress {
                benchmark_progress: 60000.0,
                category_rank: 4,
                rank_maxes: vec![15000.0, 30000.0, 45000.0, 60000.0],
                scenarios: vec![(
                    "VT Pasu Novice S5".to_string(),
                    ScenarioEntry {
                        score: 128161.0,
                        leaderboard_rank: 169,
                        scenario_rank: 4,
                        // Scenario ladders are ALREADY display-scale.
                        rank_maxes: vec![555.0, 660.0, 745.0, 800.0],
                        leaderboard_id: 98059,
                    },
                )],
            },
        )];
        let mut p = BenchmarkProgress {
            benchmark_progress: 180000.0,
            overall_rank: 4,
            categories: cats,
        };
        p.normalize_scale();
        assert_eq!(p.benchmark_progress, 1800.0);
        let cat = &p
            .categories
            .iter()
            .find(|(n, _)| n == "Clicking")
            .unwrap()
            .1;
        assert_eq!(cat.benchmark_progress, 600.0);
        assert_eq!(cat.rank_maxes, vec![150.0, 300.0, 450.0, 600.0]);
        let scen = &cat
            .scenarios
            .iter()
            .find(|(n, _)| n == "VT Pasu Novice S5")
            .unwrap()
            .1;
        assert_eq!(scen.score, 1281.61);
        // Scenario thresholds untouched.
        assert_eq!(scen.rank_maxes, vec![555.0, 660.0, 745.0, 800.0]);
    }
}
