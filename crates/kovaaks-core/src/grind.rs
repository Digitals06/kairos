//! "What to grind next": per-scenario score targets that push the benchmark's
//! overall rank to the next tier, found by probing the rank engine (bisection).
//!
//! The engine is deterministic and monotone non-decreasing in every scenario
//! score across all families, so bisection converges exactly. Note a real
//! engine property: floor/harmonic families may have NO single-scenario path
//! to the next rank (several scenarios must rise together) — in that state
//! `targets` is empty and the UI says so.

use crate::rankcalc::compute_rank;
use crate::types::{BenchmarkDef, BenchmarkProgress, Difficulty};

/// Result for one scenario: the score needed on that scenario (all others held)
/// to reach the next rank.
#[derive(Debug, Clone, PartialEq)]
pub struct GrindTarget {
    pub scenario: String,
    pub current_score: i64,
    /// Minimal score on this scenario (display units) that reaches the next
    /// rank with everything else unchanged.
    pub target_score: i64,
    pub delta: i64,
}

/// Overall result for one benchmark difficulty.
#[derive(Debug, Clone, PartialEq)]
pub struct GrindResult {
    pub current_rank: String,
    pub next_rank: String,
    pub next_rank_index: u32,
    /// Per-scenario candidates, sorted by delta ascending (cheapest first).
    /// Empty when no single-scenario path to the next rank exists.
    pub targets: Vec<GrindTarget>,
    /// When no single scenario can flip the rank, a step-by-step plan that
    /// does: each step raises one scenario (within its ladder) and the steps
    /// together reach the next rank. Empty when `targets` is non-empty, the
    /// benchmark is complete, or no reachable plan exists.
    pub plan: Vec<GrindTarget>,
}

/// Set a scenario's probe score AND its derived tier: the engine's basic
/// families read `scenario_rank` (KovaaK's own tier), so a faithful probe must
/// move the tier along with the score. Tier = number of ladder rungs strictly
/// below the score (0 = unplayed).
fn set_scenario_score(progress: &mut BenchmarkProgress, scenario: &str, score: f64) {
    for (_, cat) in &mut progress.categories {
        for (name, entry) in &mut cat.scenarios {
            if name == scenario {
                entry.score = score;
                entry.scenario_rank = if score > 0.0 {
                    entry.rank_maxes.iter().filter(|m| **m < score).count() as u32
                } else {
                    0
                };
            }
        }
    }
}

fn reaches(
    base: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
    target_scenario: &str,
    score: i64,
    next_rank_index: u32,
) -> bool {
    let mut probe = base.clone();
    set_scenario_score(&mut probe, target_scenario, score as f64);
    compute_rank(&probe, benchmark, difficulty).rank >= next_rank_index
}

fn minimal_score_for_rank(
    base: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
    target_scenario: &str,
    next_rank_index: u32,
    lo: i64,
    hi: i64,
) -> Option<i64> {
    if reaches(
        base,
        benchmark,
        difficulty,
        target_scenario,
        lo,
        next_rank_index,
    ) {
        return Some(lo);
    }
    if !reaches(
        base,
        benchmark,
        difficulty,
        target_scenario,
        hi,
        next_rank_index,
    ) {
        return None; // even a maxed scenario can't flip it (harmonic drag / cap)
    }
    let mut lo = lo;
    let mut hi = hi;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if reaches(
            base,
            benchmark,
            difficulty,
            target_scenario,
            mid,
            next_rank_index,
        ) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(lo)
}

/// Build a step-by-step plan for benchmarks where no single scenario can flip
/// the rank (floor/harmonic families): each round finds, for every scenario
/// with headroom, its minimal score that maximizes the engine's rank given the
/// current probe state, applies the single most-advancing (then cheapest)
/// step, and repeats until the next rank is reached or no step advances it.
/// Every step stays within the scenario's ladder, so the plan is achievable.
fn combined_plan(
    base: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
    scenarios: &[(String, i64, i64)],
    next_index: u32,
) -> Vec<GrindTarget> {
    let mut probe = base.clone();
    let mut plan: Vec<GrindTarget> = Vec::new();
    let mut prev_rank = compute_rank(&probe, benchmark, difficulty).rank;
    // Mutable step state: `scenarios` holds the ORIGINAL scores; `live` tracks
    // each scenario's current score as steps apply.
    let mut live: Vec<(String, i64, i64)> = scenarios.to_vec();

    for _round in 0..64 {
        if prev_rank >= next_index {
            break;
        }
        // Best step this round: maximize the resulting rank, then minimize delta.
        let mut best: Option<(u32, i64, String, i64, i64)> = None; // (rank, delta, name, cur, tgt)
        for (name, cur, cap) in &live {
            if cur >= cap {
                continue;
            }
            // What rank does this scenario's ladder top reach (given the
            // current probe state)? If it advances, bisect the minimal score
            // achieving that rank; otherwise raising it partially is still a
            // useful step only when the rank advances — skip pure partials.
            let (mut lo, mut hi) = (*cur, *cap);
            set_scenario_score(&mut probe, name, *cap as f64);
            let top_rank = compute_rank(&probe, benchmark, difficulty).rank;
            set_scenario_score(&mut probe, name, *cur as f64);
            let mut best_rank = prev_rank;
            let mut best_score = *cur;
            if top_rank > prev_rank {
                best_rank = top_rank;
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    set_scenario_score(&mut probe, name, mid as f64);
                    let r = compute_rank(&probe, benchmark, difficulty).rank;
                    set_scenario_score(&mut probe, name, *cur as f64);
                    if r >= top_rank {
                        hi = mid;
                    } else {
                        lo = mid + 1;
                    }
                }
                best_score = lo;
            }
            if best_rank > prev_rank {
                let delta = best_score - cur;
                let cand = (best_rank, delta, name.clone(), *cur, best_score);
                let replace = match &best {
                    None => true,
                    Some((br, bd, _, _, _)) => best_rank > *br || (best_rank == *br && delta < *bd),
                };
                if replace {
                    best = Some(cand);
                }
            }
        }
        match best {
            Some((new_rank, _, name, cur, tgt)) => {
                set_scenario_score(&mut probe, &name, tgt as f64);
                plan.push(GrindTarget {
                    scenario: name.clone(),
                    current_score: cur,
                    target_score: tgt,
                    delta: tgt - cur,
                });
                if let Some(e) = live.iter_mut().find(|(n, _, _)| *n == name) {
                    e.1 = tgt;
                }
                if new_rank > prev_rank {
                    prev_rank = new_rank;
                }
            }
            None => {
                // No single step advances the rank: floor/harmonic drag. Prep
                // step — raise the binding scenario (lowest scenario_rank, then
                // lowest score) to its next ladder rung. Each prep either lifts
                // a scenario a full rung or maxes it out, so rounds terminate.
                let mut binding: Option<(usize, i64, u32)> = None; // (idx, next_rung, srank)
                for (idx, (name, cur, cap)) in live.iter().enumerate() {
                    if cur >= cap {
                        continue;
                    }
                    let entry = base
                        .categories
                        .iter()
                        .find_map(|(_, c)| c.scenarios.iter().find(|(n, _)| n == name))
                        .map(|(_, e)| e);
                    let srank = entry.map(|e| e.scenario_rank).unwrap_or(0);
                    let rungs = entry.map(|e| e.rank_maxes.clone()).unwrap_or_default();
                    let next_rung = rungs
                        .iter()
                        .copied()
                        .map(|m| m as i64)
                        .find(|m| *m > *cur)
                        .unwrap_or(*cap)
                        .min(*cap);
                    let better = match binding {
                        None => true,
                        Some((_, _, bs)) => srank < bs,
                    };
                    if better {
                        binding = Some((idx, next_rung, srank));
                    }
                }
                match binding {
                    Some((idx, rung, _)) => {
                        let (name, cur, _) = (live[idx].0.clone(), live[idx].1, live[idx].2);
                        set_scenario_score(&mut probe, &name, rung as f64);
                        plan.push(GrindTarget {
                            scenario: name.clone(),
                            current_score: cur,
                            target_score: rung,
                            delta: rung - cur,
                        });
                        live[idx].1 = rung;
                    }
                    None => break, // everything maxed, still stuck: no honest plan
                }
            }
        }
    }
    if prev_rank >= next_index {
        consolidate_plan(plan)
    } else {
        Vec::new() // even maxing step-by-step didn't reach it: no honest plan
    }
}

/// Fold the plan's rung-by-rung steps so each scenario appears once: its
/// `current_score` is the score at its first step, `target_score` the score
/// after its last step. First-appearance order is preserved.
fn consolidate_plan(plan: Vec<GrindTarget>) -> Vec<GrindTarget> {
    let mut order: Vec<String> = Vec::new();
    let mut acc: std::collections::HashMap<String, (i64, i64)> = std::collections::HashMap::new();
    for step in plan {
        let e = acc.entry(step.scenario.clone()).or_insert_with(|| {
            order.push(step.scenario.clone());
            (step.current_score, step.current_score)
        });
        e.1 = step.target_score;
    }
    order
        .into_iter()
        .filter_map(|name| {
            acc.remove(&name).map(|(from, to)| GrindTarget {
                delta: to - from,
                current_score: from,
                target_score: to,
                scenario: name,
            })
        })
        .collect()
}

/// Compute grind targets for one benchmark difficulty from a stored progress
/// payload. Scenarios missing from it are treated as 0./// Compute grind targets for one benchmark difficulty from a stored progress
/// payload. Scenarios missing from it are treated as 0.
pub fn next_targets(
    base: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> GrindResult {
    let current = compute_rank(base, benchmark, difficulty);
    let ladder_len = difficulty.rank_colors.len() as u32;
    let next_index = if current.complete {
        ladder_len + 1
    } else {
        current.rank + 1
    };
    let next_name = if next_index > ladder_len {
        // Already at the top: no next tier — UI shows the panel as complete.
        current.name.clone()
    } else {
        difficulty
            .rank_colors
            .get((next_index - 1) as usize)
            .map(|t| t.name.clone())
            .unwrap_or_default()
    };

    // Per-scenario (name, current score, reachable ceiling). The scenario's own
    // ladder top is the highest score a real player can set; probing beyond it
    // produces mathematically-flipping but game-impossible targets, so it caps
    // the search. Scenarios without a ladder fall back to a fixed ceiling.
    const CEILING: i64 = 100_000;
    let mut scenarios: Vec<(String, i64, i64)> = Vec::new();
    for (_, cat) in &base.categories {
        for (name, entry) in &cat.scenarios {
            let score = entry.score as i64;
            let cap = entry
                .rank_maxes
                .last()
                .map(|m| *m as i64)
                .unwrap_or(CEILING)
                .max(CEILING.min(1))
                .max(score);
            match scenarios.iter_mut().find(|(n, _, _)| n == name) {
                Some((_, s, c)) => {
                    *s = (*s).max(score);
                    *c = (*c).max(cap);
                }
                None => scenarios.push((name.clone(), score, cap)),
            }
        }
    }

    let mut targets = Vec::new();
    if !current.complete {
        for (name, cur, cap) in &scenarios {
            if cur >= cap {
                continue; // already at the ladder top: no reachable headroom
            }
            if let Some(target) =
                minimal_score_for_rank(base, benchmark, difficulty, name, next_index, *cur, *cap)
            {
                if target > *cur {
                    targets.push(GrindTarget {
                        scenario: name.clone(),
                        current_score: *cur,
                        target_score: target,
                        delta: target - cur,
                    });
                }
            }
        }
    }
    targets.sort_by_key(|t| t.delta);

    // No single-scenario path: fall back to a step-by-step combined plan.
    let plan = if targets.is_empty() && !current.complete {
        combined_plan(base, benchmark, difficulty, &scenarios, next_index)
    } else {
        Vec::new()
    };

    GrindResult {
        current_rank: current.name,
        next_rank: next_name,
        next_rank_index: next_index,
        targets,
        plan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CategoryProgress, ScenarioEntry};

    fn progress_from(entries: &[(&str, f64, &[f64])]) -> BenchmarkProgress {
        let scenarios: Vec<(String, ScenarioEntry)> = entries
            .iter()
            .map(|(name, score, maxes)| {
                (
                    name.to_string(),
                    ScenarioEntry {
                        score: *score,
                        leaderboard_rank: 1,
                        scenario_rank: 1,
                        rank_maxes: maxes.to_vec(),
                        leaderboard_id: 0,
                    },
                )
            })
            .collect();
        BenchmarkProgress {
            benchmark_progress: 0.0,
            overall_rank: 0,
            categories: vec![(
                "Clicking".to_string(),
                CategoryProgress {
                    benchmark_progress: 0.0,
                    category_rank: 1,
                    rank_maxes: Vec::new(),
                    scenarios,
                },
            )],
        }
    }

    fn vt_s5_names() -> [&'static str; 18] {
        [
            "VT Pasu Advanced S5",
            "VT Popcorn Advanced S5",
            "VT 1w2ts Advanced S5",
            "VT ww5t Advanced S5",
            "VT Frogtagon Advanced S5",
            "VT Floating Heads Advanced S5",
            "VT PGT Advanced S5",
            "VT Snake Track Advanced S5",
            "VT Aether Advanced S5",
            "VT Ground Advanced S5",
            "VT Raw Control Advanced S5",
            "VT Controlsphere Advanced S5",
            "VT DotTS Advanced S5",
            "VT EddieTS Advanced S5",
            "VT DriftTS Advanced S5",
            "VT FlyTS Advanced S5",
            "VT ControlTS Advanced S5",
            "VT Penta Bounce Advanced S5",
        ]
    }

    fn vt_s5_maxes() -> Vec<&'static [f64]> {
        vec![
            &[910.0, 1020.0, 1110.0, 1240.0],
            &[680.0, 800.0, 910.0, 1020.0],
            &[1320.0, 1420.0, 1520.0, 1620.0],
            &[1510.0, 1610.0, 1720.0, 1860.0],
            &[1090.0, 1220.0, 1360.0, 1490.0],
            &[740.0, 830.0, 920.0, 1050.0],
            &[2750.0, 3175.0, 3625.0, 4050.0],
            &[3050.0, 3425.0, 3725.0, 4050.0],
            &[2750.0, 3175.0, 3525.0, 3825.0],
            &[2875.0, 3200.0, 3500.0, 3725.0],
            &[3150.0, 3550.0, 3875.0, 4250.0],
            &[3100.0, 3475.0, 3800.0, 4125.0],
            &[1280.0, 1360.0, 1420.0, 1500.0],
            &[1020.0, 1120.0, 1200.0, 1280.0],
            &[430.0, 470.0, 510.0, 540.0],
            &[540.0, 600.0, 660.0, 720.0],
            &[450.0, 490.0, 520.0, 550.0],
            &[530.0, 580.0, 630.0, 670.0],
        ]
    }

    /// Core contract: every returned target flips the rank and is minimal.
    #[test]
    fn targets_flip_rank_and_are_minimal() {
        let registry = crate::Registry;
        let (bench, difficulty) = registry.by_id(460).expect("VT S5");
        let maxes = vt_s5_maxes();
        let entries: Vec<(&str, f64, &[f64])> = vt_s5_names()
            .iter()
            .zip(maxes.iter())
            .map(|(n, m)| {
                let score = if *n == "VT DriftTS Advanced S5" {
                    470.0
                } else {
                    *m.last().unwrap()
                };
                (*n, score, *m)
            })
            .collect();
        let base = progress_from(&entries);
        let result = next_targets(&base, bench, &difficulty);
        for t in &result.targets {
            let mut probe = base.clone();
            set_scenario_score(&mut probe, &t.scenario, t.target_score as f64);
            let rank = compute_rank(&probe, bench, &difficulty);
            assert!(
                rank.rank >= result.next_rank_index,
                "target {} on {} did not flip (got {})",
                t.target_score,
                t.scenario,
                rank.rank
            );
            let mut probe = base.clone();
            set_scenario_score(&mut probe, &t.scenario, (t.target_score - 1) as f64);
            let rank = compute_rank(&probe, bench, &difficulty);
            assert!(
                rank.rank < result.next_rank_index,
                "target {} - 1 on {} already flips (not minimal)",
                t.target_score,
                t.scenario
            );
        }
    }

    /// Basic (floor) family: single-scenario gains can never lift the floor,
    /// so targets must be empty while current/next rank are still exposed.
    #[test]
    fn basic_family_has_no_single_scenario_targets() {
        let registry = crate::Registry;
        let (bench, difficulty) = registry.by_id(266).expect("VT S3 Advanced");
        let maxes: [&[f64]; 6] = [
            &[68.0, 76.0, 85.0, 95.0, 105.0, 110.2],
            &[78.0, 88.0, 98.0, 108.0, 115.0, 123.0],
            &[220.0, 260.0, 320.0, 390.0, 440.0, 450.0],
            &[130.0, 138.0, 148.0, 160.0, 170.0, 172.0],
            &[115.0, 120.0, 130.0, 142.0, 152.0, 156.0],
            &[152.0, 160.0, 175.0, 192.0, 210.0, 213.0],
        ];
        let names = [
            "Pasu Voltaic",
            "B180 Voltaic",
            "Popcorn Voltaic",
            "ww3t Voltaic",
            "1w4ts Voltaic",
            "6 Sphere Hipfire Voltaic",
        ];
        let entries: Vec<(&str, f64, &[f64])> = names
            .iter()
            .zip(maxes.iter())
            .map(|(n, m)| (*n, m[1], *m))
            .collect();
        let base = progress_from(&entries);
        let result = next_targets(&base, bench, &difficulty);
        assert!(
            result.targets.is_empty(),
            "floor family: {:?}",
            result.targets
        );
        assert!(!result.current_rank.is_empty());
        assert!(!result.next_rank.is_empty());
    }

    /// Real regression (Aimerz+ SpeedTS, id 584, aplus-alt family): before the
    /// reachability cap, one scenario probed to ~98k — far beyond its ladder.
    /// Every target must stay within its scenario's top rung.
    #[test]
    fn targets_never_exceed_scenario_ladder() {
        let registry = crate::Registry;
        let (bench, difficulty) = registry.by_id(584).expect("Aimerz+ SpeedTS");
        // Live snapshot shape: (name, score, rank_maxes) as stored by sync.
        let entries: Vec<(&str, f64, &[f64])> = vec![
            (
                "StaticSwitchingVox xxSmall",
                121.0,
                &[96.0, 101.0, 106.0, 112.0, 117.0, 123.0, 154.0],
            ),
            (
                "DotTS 30% Larger",
                2836.0,
                &[2200.0, 2350.0, 2450.0, 2600.0, 2700.0, 2850.0, 3600.0],
            ),
            (
                "voxTS Viscose Varied",
                124.0,
                &[86.0, 94.0, 100.0, 106.0, 112.0, 120.0, 154.0],
            ),
            (
                "patCircleSwitch NR",
                114.0,
                &[80.0, 84.0, 90.0, 96.0, 102.0, 108.0, 146.0],
            ),
            (
                "patTargetSwitch 90 LowTTK",
                3064.0,
                &[2350.0, 2500.0, 2650.0, 2800.0, 2950.0, 3100.0, 4240.0],
            ),
            (
                "beanTS",
                148.0,
                &[94.0, 104.0, 114.0, 122.0, 132.0, 140.0, 186.0],
            ),
            (
                "Target Switching 360",
                14733.0,
                &[
                    11400.0, 12100.0, 12800.0, 13600.0, 14200.0, 15000.0, 19600.0,
                ],
            ),
            (
                "voxTargetSwitch 2",
                130.0,
                &[83.0, 92.0, 102.0, 111.0, 117.0, 125.0, 161.0],
            ),
            (
                "Aimerz+ goaTS Med S1",
                113.0,
                &[81.0, 85.0, 90.0, 96.0, 102.0, 110.0, 148.0],
            ),
            (
                "Aimerz+ nuTS Med S1",
                110.0,
                &[80.0, 86.0, 92.0, 98.0, 106.0, 112.0, 156.0],
            ),
            (
                "Aimerz+ Week #5 - Static Switching",
                15441.0,
                &[
                    12500.0, 12900.0, 13400.0, 14200.0, 15000.0, 15800.0, 18600.0,
                ],
            ),
            (
                "360 Static TS",
                2900.0,
                &[2200.0, 2350.0, 2500.0, 2650.0, 2800.0, 2950.0, 3800.0],
            ),
        ];
        let base = progress_from(&entries);
        let result = next_targets(&base, bench, &difficulty);
        for t in &result.targets {
            let top = entries
                .iter()
                .find(|(n, _, _)| *n == t.scenario)
                .map(|(_, _, m)| m.last().unwrap())
                .expect("fixture scenario");
            assert!(
                (t.target_score as f64) <= *top,
                "target {} on {} exceeds its ladder top {}",
                t.target_score,
                t.scenario,
                top
            );
        }
    }

    /// Floor-family benchmark (VT S3, basic method): no single scenario can
    /// lift the floor, so the plan kicks in — every step stays within its
    /// scenario's ladder and the final probe reaches the next rank.
    #[test]
    fn combined_plan_reaches_next_rank_on_floor_family() {
        let registry = crate::Registry;
        let (bench, difficulty) = registry.by_id(266).expect("VT S3 Advanced");
        let maxes: [&[f64]; 6] = [
            &[68.0, 76.0, 85.0, 95.0, 105.0, 110.2],
            &[78.0, 88.0, 98.0, 108.0, 115.0, 123.0],
            &[220.0, 260.0, 320.0, 390.0, 440.0, 450.0],
            &[130.0, 138.0, 148.0, 160.0, 170.0, 172.0],
            &[115.0, 120.0, 130.0, 142.0, 152.0, 156.0],
            &[152.0, 160.0, 175.0, 192.0, 210.0, 213.0],
        ];
        let names = [
            "Pasu Voltaic",
            "B180 Voltaic",
            "Popcorn Voltaic",
            "ww3t Voltaic",
            "1w4ts Voltaic",
            "6 Sphere Hipfire Voltaic",
        ];
        let entries: Vec<(&str, f64, &[f64])> = names
            .iter()
            .zip(maxes.iter())
            .map(|(n, m)| (*n, m[1], *m))
            .collect();
        let base = progress_from(&entries);
        let result = next_targets(&base, bench, &difficulty);
        if result.targets.is_empty() {
            assert!(
                !result.plan.is_empty(),
                "floor family with headroom must produce a combined plan"
            );
            // Each scenario appears at most once in the plan.
            let mut names: Vec<&str> = result.plan.iter().map(|s| s.scenario.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(
                names.len(),
                result.plan.len(),
                "duplicate scenarios in plan"
            );
            // Replay the plan: every step within ladder, final state reaches
            // next_rank_index.
            let mut probe = base.clone();
            let mut seen: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
            for step in &result.plan {
                let top = entries
                    .iter()
                    .find(|(n, _, _)| *n == step.scenario)
                    .map(|(_, _, m)| *m.last().unwrap())
                    .expect("fixture scenario") as i64;
                assert!(
                    step.target_score <= top,
                    "plan step {} exceeds ladder top {}",
                    step.target_score,
                    top
                );
                let prev = seen.insert(step.scenario.clone(), step.target_score);
                if let Some(prev_score) = prev {
                    assert!(
                        step.target_score >= prev_score,
                        "plan moves {} backwards",
                        step.scenario
                    );
                }
                set_scenario_score(&mut probe, &step.scenario, step.target_score as f64);
            }
            let final_rank = compute_rank(&probe, bench, &difficulty);
            assert!(
                final_rank.rank >= result.next_rank_index,
                "plan replay reaches rank {} but next is {}",
                final_rank.rank,
                result.next_rank_index
            );
        } else {
            // If single targets exist for this state, plan must be empty.
            assert!(result.plan.is_empty());
        }
    }

    /// Complete benchmarks expose no targets and no plan.
    #[test]
    fn complete_benchmark_has_no_plan() {
        let registry = crate::Registry;
        let (bench, difficulty) = registry.by_id(266).expect("VT S3 Advanced");
        let maxes: [&[f64]; 6] = [
            &[68.0, 76.0, 85.0, 95.0, 105.0, 110.2],
            &[78.0, 88.0, 98.0, 108.0, 115.0, 123.0],
            &[220.0, 260.0, 320.0, 390.0, 440.0, 450.0],
            &[130.0, 138.0, 148.0, 160.0, 170.0, 172.0],
            &[115.0, 120.0, 130.0, 142.0, 152.0, 156.0],
            &[152.0, 160.0, 175.0, 192.0, 210.0, 213.0],
        ];
        let names = [
            "Pasu Voltaic",
            "B180 Voltaic",
            "Popcorn Voltaic",
            "ww3t Voltaic",
            "1w4ts Voltaic",
            "6 Sphere Hipfire Voltaic",
        ];
        let entries: Vec<(&str, f64, &[f64])> = names
            .iter()
            .zip(maxes.iter())
            .map(|(n, m)| (*n, m.last().unwrap() + 50.0, *m))
            .collect();
        let base = progress_from(&entries);
        let result = next_targets(&base, bench, &difficulty);
        assert!(result.targets.is_empty());
        assert!(result.plan.is_empty());
    }

    /// Complete benchmarks expose no targets.
    #[test]
    fn complete_benchmark_has_no_targets() {
        let registry = crate::Registry;
        let (bench, difficulty) = registry.by_id(266).expect("VT S3 Advanced");
        let maxes: [&[f64]; 6] = [
            &[68.0, 76.0, 85.0, 95.0, 105.0, 110.2],
            &[78.0, 88.0, 98.0, 108.0, 115.0, 123.0],
            &[220.0, 260.0, 320.0, 390.0, 440.0, 450.0],
            &[130.0, 138.0, 148.0, 160.0, 170.0, 172.0],
            &[115.0, 120.0, 130.0, 142.0, 152.0, 156.0],
            &[152.0, 160.0, 175.0, 192.0, 210.0, 213.0],
        ];
        let names = [
            "Pasu Voltaic",
            "B180 Voltaic",
            "Popcorn Voltaic",
            "ww3t Voltaic",
            "1w4ts Voltaic",
            "6 Sphere Hipfire Voltaic",
        ];
        let entries: Vec<(&str, f64, &[f64])> = names
            .iter()
            .zip(maxes.iter())
            .map(|(n, m)| (*n, m.last().unwrap() + 50.0, *m))
            .collect();
        let base = progress_from(&entries);
        let result = next_targets(&base, bench, &difficulty);
        assert!(result.targets.is_empty());
    }
}
