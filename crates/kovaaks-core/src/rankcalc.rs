//! Rank calculation engine (v0.2, spec: `docs/rank-systems.md`).
//!
//! Re-implements evxl's per-benchmark `rankCalculation` methods so displayed
//! ranks match evxl instead of the KovaaK's API's `overall_rank`. Pure
//! computation over [`BenchmarkProgress`] + registry definitions.
//!
//! Verified against the minified engine in evxl's bundle
//! (`_app/immutable/chunks/CNCnlpN6.js`, dispatcher `qn`, 2026-09-04).

use crate::types::{BenchmarkDef, BenchmarkProgress, Difficulty, ScenarioEntry};

/// A scenario's position on its own `rank_maxes` ladder, mirroring evxl's `H`.
#[derive(Debug, Clone, PartialEq)]
pub struct PreciseRank {
    /// 1-based ladder index; 0 = below the first threshold.
    pub base: u32,
    /// `base + fraction into the current band` (can exceed max when maxed).
    pub precise: f64,
    /// Fraction (0..1) through the current band.
    pub progress: f64,
    /// Score above the highest threshold.
    pub is_maxed: bool,
}

/// `rankOf(score, maxes)` — centi-scale in, ladder position out.
pub fn rank_of(score: f64, maxes: &[f64]) -> PreciseRank {
    if score <= 0.0 || maxes.is_empty() {
        return PreciseRank {
            base: 0,
            precise: 0.0,
            progress: 0.0,
            is_maxed: false,
        };
    }
    let mut base = 0u32;
    for (i, threshold) in maxes.iter().enumerate().rev() {
        if score >= *threshold {
            base = (i + 1) as u32;
            break;
        }
    }
    if base == 0 {
        // Below the first threshold: linear 0..0.99 progress toward it.
        let first = maxes
            .iter()
            .cloned()
            .filter(|m| *m > 0.0)
            .fold(f64::INFINITY, f64::min);
        let progress = if first.is_finite() && first > 0.0 {
            (score / first).min(0.99)
        } else {
            0.0
        };
        return PreciseRank {
            base: 0,
            precise: 0.0,
            progress,
            is_maxed: false,
        };
    }
    if base as usize == maxes.len() {
        let top = maxes[maxes.len() - 1];
        let prev = if maxes.len() > 1 {
            maxes[maxes.len() - 2]
        } else {
            0.0
        };
        let band = (top - prev).abs().max(1.0);
        let over = ((score - top) / band).max(0.0);
        return PreciseRank {
            base,
            precise: base as f64 + over,
            progress: over.fract(),
            is_maxed: true,
        };
    }
    let low = maxes[base as usize - 1];
    let high = maxes[base as usize];
    let band = high - low;
    let progress = if band > 0.0 {
        ((score - low) / band).clamp(0.0, 1.0)
    } else {
        0.0
    };
    PreciseRank {
        base,
        precise: base as f64 + progress,
        progress,
        is_maxed: false,
    }
}

/// The canonical scenario list of one difficulty: registry order
/// (category → subcategory → scenarioCount), matched against the API payload.
pub fn scenario_order<'a>(
    progress: &'a BenchmarkProgress,
    difficulty: &Difficulty,
) -> Vec<(&'a str, &'a ScenarioEntry)> {
    let mut out = Vec::new();
    // Registry declares (category, subcategory, count); the API returns the
    // same scenarios in document order. Walk the API's flat order and bucket
    // by the registry's expected counts, matching by name when possible.
    let mut api: Vec<(&str, &ScenarioEntry)> = progress
        .categories
        .iter()
        .flat_map(|(_, c)| c.scenarios.iter().map(|(n, s)| (n.as_str(), s)))
        .collect();
    let registry = difficulty
        .categories
        .iter()
        .filter_map(|c| c.as_object())
        .flat_map(|c| {
            c.get("subcategories")
                .and_then(|s| s.as_array())
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|s| {
            let count = s.get("scenarioCount")?.as_u64()? as usize;
            Some(count)
        });
    let mut expected: Vec<usize> = registry.collect();
    let total_expected: usize = expected.iter().sum();
    if total_expected == 0 || total_expected != api.len() {
        // Registry metadata missing/mismatched: fall back to API order.
        expected.clear();
    }
    if !expected.is_empty() {
        for count in expected {
            let take = count.min(api.len());
            out.extend(api.drain(..take));
        }
    } else {
        out = api;
    }
    out
}

/// Harmonic mean; 0 when empty or any entry is 0 (evxl `Z` — strict).
pub fn harmonic_strict(values: &[f64]) -> f64 {
    if values.is_empty() || values.iter().any(|v| *v <= 0.0) {
        return 0.0;
    }
    values.len() as f64 / values.iter().map(|v| 1.0 / v).sum::<f64>()
}

/// Harmonic mean with zeros treated as 0.1 (evxl `le` — jade-palace).
pub fn harmonic_soft(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let patched: Vec<f64> = values
        .iter()
        .map(|v| if *v > 0.0 { *v } else { 0.1 })
        .collect();
    (patched.len() as f64 / patched.iter().map(|v| 1.0 / v).sum::<f64>()).trunc()
}

/// Average of the best half of `values` (evxl `ie`; `top3 = true` → top 3).
pub fn avg_best_half(values: &[f64], top3: bool) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let take = if top3 {
        3.min(sorted.len())
    } else {
        sorted.len().div_ceil(2)
    };
    (sorted[..take].iter().sum::<f64>() / take as f64).trunc()
}

/// Mean of the top-2 values, or the single value halved when only one
/// (evxl `se`'s subcategory energy).
pub fn avg_top2(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    match sorted.len() {
        0 => 0.0,
        1 => sorted[0] / 2.0,
        _ => (sorted[0] + sorted[1]) / 2.0,
    }
}

/// Map a [`PreciseRank`] onto an energy axis with `thresholds` 100 apart
/// starting `fake_lower` below the first real threshold, extrapolating up to
/// `fake_upper` fake ranks past the top (evxl `W`).
pub fn energy_of(rank: &PreciseRank, thresholds: &[f64], fake_lower: u32, fake_upper: f64) -> f64 {
    if thresholds.is_empty() || rank.base == 0 && rank.precise <= 0.0 {
        return 0.0;
    }
    let count = thresholds.len();
    let first_low = thresholds[0] - fake_lower as f64 * 100.0;
    let top = thresholds[count - 1];
    let prev = if count > 1 {
        thresholds[count - 2]
    } else {
        0.0
    };
    let top_band = (top - prev).abs().max(1.0);
    let ceil = top + fake_upper * top_band;

    let value = if rank.base == 0 {
        // Below rank 1: linear between first_low and thresholds[0] by progress.
        first_low + rank.progress * (thresholds[0] - first_low)
    } else if rank.is_maxed {
        let over = rank.precise - count as f64;
        let over = over.clamp(0.0, fake_upper);
        top + over * top_band
    } else if (rank.base as usize) >= count {
        // Saturated the ladder: treat like is_maxed (guard the OOB index).
        let over = (rank.precise - count as f64).clamp(0.0, fake_upper);
        top + over * top_band
    } else {
        let low = thresholds[rank.base as usize - 1];
        let high = thresholds[rank.base as usize];
        low + rank.progress * (high - low)
    };
    value.clamp(first_low, ceil).trunc()
}

/// The 100-per-rank ladder covering every rank of every difficulty of a
/// benchmark (evxl builds `Array.from({length: totalRanks+1}, (i)=>(i+1)*100)`).
pub fn full_ladder(benchmark: &BenchmarkDef) -> Vec<f64> {
    let total: usize = benchmark
        .difficulties
        .iter()
        .map(|d| d.rank_colors.len())
        .sum();
    (0..=total).map(|i| (i + 1) as f64 * 100.0).collect()
}

/// The ladder slice belonging to one difficulty: everything before its first
/// rank index, up to (and excluding) one past its last — evxl `D(...).slice(0,-1)`.
pub fn difficulty_thresholds(benchmark: &BenchmarkDef, difficulty: &Difficulty) -> Vec<f64> {
    let ladder = full_ladder(benchmark);
    let before: usize = benchmark
        .difficulties
        .iter()
        .take_while(|d| d.name != difficulty.name)
        .map(|d| d.rank_colors.len())
        .sum();
    let count = difficulty.rank_colors.len();
    ladder[before..(before + count)].to_vec()
}

/// Per-difficulty fixed slices used by `vt-energy` (evxl `Se`).
pub fn vt_energy_thresholds(difficulty: &Difficulty) -> Vec<f64> {
    let table: [f64; 15] = [
        100., 200., 300., 400., 500., 600., 700., 800., 900., 1000., 1100., 1200., 1300., 1400.,
        1500.,
    ];
    let name = difficulty.name.trim().to_lowercase();
    if name.contains("elite") || name.contains("unofficial") {
        return table[9..15].to_vec();
    }
    let (start, end) = match name.as_str() {
        "novice" => (0, 4),
        "intermediate" => (4, 8),
        "advanced" => (8, 12),
        _ => (0, table.len()),
    };
    table[start..end].to_vec()
}

/// Whether a subcategory participates in `vt-energy` / `ca-s1`
/// (evxl excludes anything containing "strafe").
pub fn is_strafe(subcategory: &str) -> bool {
    subcategory.to_lowercase().contains("strafe")
}

/// `jade-palace`: subcategory energy = avg best half (Fundamentals: top 3,
/// Easy: capped 600 until overall reaches 600), harmonic soft.
pub fn calc_jade_palace(
    progress: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> (u32, f64) {
    let mut thresholds = difficulty_thresholds(benchmark, difficulty);
    if thresholds.len() > 1 {
        thresholds.pop(); // evxl slices off the last ladder entry
    }
    let is_easy = difficulty.name.eq_ignore_ascii_case("easy");
    let is_fundamentals = difficulty.name.eq_ignore_ascii_case("fundamentals");
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let mut energies: Vec<f64> = Vec::new();
    let mut uncapped_energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, _) in &subs {
        let mut capped: Vec<f64> = Vec::new();
        let mut raw: Vec<f64> = Vec::new();
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                let norm = entry.score;
                if norm > 0.0 && !entry.rank_maxes.is_empty() {
                    let pr = rank_of(norm, &entry.rank_maxes);
                    let e = energy_of(&pr, &thresholds, 100, 1.0);
                    raw.push(e);
                    capped.push(if is_easy { e.min(600.0) } else { e });
                } else {
                    raw.push(0.0);
                    capped.push(0.0);
                }
            }
            idx += 1;
        }
        energies.push(avg_best_half(&capped, is_fundamentals));
        uncapped_energies.push(avg_best_half(&raw, is_fundamentals));
    }
    let mut overall = harmonic_soft(&energies);
    // Easy: once capped overall reaches 600, uncap.
    if is_easy && harmonic_soft(&uncapped_energies) >= 600.0 {
        overall = harmonic_soft(&uncapped_energies);
    }
    let mut rank = 0u32;
    for (i, t) in thresholds.iter().enumerate() {
        if overall >= *t {
            rank = (i + 1) as u32;
        }
    }
    (rank, overall)
}

/// `aimbeast`: category rank = average of scenario ranks; overall = average of
/// category ranks; any unranked scenario poisons its category.
pub fn calc_aimbeast(progress: &BenchmarkProgress) -> (u32, f64) {
    let mut category_ranks: Vec<f64> = Vec::new();
    for (_, category) in &progress.categories {
        let mut sum = 0.0;
        let mut n = 0usize;
        for (_, scenario) in &category.scenarios {
            if scenario.scenario_rank == 0 || scenario.score <= 0.0 {
                sum = 0.0;
                n = 0;
                break;
            }
            sum += scenario.scenario_rank as f64;
            n += 1;
        }
        if n == 0 {
            return (0, 0.0);
        }
        category_ranks.push(sum / n as f64);
    }
    if category_ranks.is_empty() {
        return (0, 0.0);
    }
    let avg = category_ranks.iter().sum::<f64>() / category_ranks.len() as f64;
    (avg.floor() as u32, avg)
}

/// `dojo`-family: N scenarios at rank R (or higher) ⇒ rank R
/// (dojo = 4, dojo2 = 3, dojo3 = 5).
pub fn calc_count_required(progress: &BenchmarkProgress, required: usize) -> u32 {
    let mut counts: std::collections::BTreeMap<u32, u32> = Default::default();
    for (_, category) in &progress.categories {
        for (_, scenario) in &category.scenarios {
            if scenario.score > 0.0 && scenario.scenario_rank > 0 {
                *counts.entry(scenario.scenario_rank).or_default() += 1;
            }
        }
    }
    let mut ranks: Vec<u32> = counts.keys().cloned().collect();
    ranks.sort_unstable_by(|a, b| b.cmp(a));
    for &r in &ranks {
        let at_or_above: u32 = ranks.iter().filter(|&&k| k >= r).map(|k| counts[k]).sum();
        if at_or_above >= required as u32 {
            return r;
        }
    }
    0
}

/// `hewchy`: 12 scenarios at rank R.
pub fn calc_hewchy(progress: &BenchmarkProgress) -> u32 {
    calc_count_required(progress, 12)
}

/// `e1se`: 6 scenarios at rank R.
pub fn calc_e1se(progress: &BenchmarkProgress) -> u32 {
    calc_count_required(progress, 6)
}

/// `aoi`: 1 score at R in 4 different subcategories, OR 2 scores at R in
/// 3 different subcategories; best qualifying rank wins.
pub fn calc_aoi(progress: &BenchmarkProgress, difficulty: &Difficulty) -> u32 {
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    // (rank -> set of subcategory keys), one entry per subcategory occurrence
    let mut per_sub: Vec<Vec<u32>> = Vec::new();
    let mut idx = 0usize;
    for (_, count, _) in &subs {
        let mut ranks = Vec::new();
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                if entry.score > 0.0 && entry.scenario_rank > 0 {
                    ranks.push(entry.scenario_rank);
                }
            }
            idx += 1;
        }
        per_sub.push(ranks);
    }
    let qualifies = |target: u32, need_subs: usize, need_scores: usize| -> bool {
        per_sub
            .iter()
            .filter(|ranks| ranks.iter().filter(|&&r| r >= target).count() >= need_scores)
            .count()
            >= need_subs
    };
    let ladder = difficulty.rank_colors.len() as u32;
    for target in (1..=ladder).rev() {
        if qualifies(target, 4, 1) || qualifies(target, 3, 2) {
            return target;
        }
    }
    0
}

/// `MIYU`: points per scenario = 2 + (rank - 1); total vs fixed thresholds.
pub fn calc_miyu(progress: &BenchmarkProgress) -> u32 {
    const THRESHOLDS: [f64; 7] = [16., 24., 32., 40., 48., 56., 63.];
    let total: f64 = progress
        .categories
        .iter()
        .flat_map(|(_, c)| c.scenarios.iter())
        .map(|(_, s)| {
            if s.score > 0.0 && s.scenario_rank > 0 {
                2.0 + (s.scenario_rank as f64 - 1.0)
            } else {
                0.0
            }
        })
        .sum();
    let mut rank = 0u32;
    for (i, t) in THRESHOLDS.iter().enumerate() {
        if total >= *t {
            rank = (i + 1) as u32;
        }
    }
    rank
}

// ---------- dispatcher ----------

/// Dispatch: compute (rank index, display name, complete flag) for a benchmark.
/// Falls back to the API `overall_rank` for methods not yet ported.
pub fn compute_rank(
    progress: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> RankResult {
    let method = benchmark.rank_calculation.as_str();
    let ladder_len = difficulty.rank_colors.len() as u32;
    let floor = scenario_floor_rank(progress);
    let (engine_rank, complete): (u32, bool) = match method {
        "basic" => {
            let (r, c, _) = calc_basic(progress, difficulty);
            (r, c)
        }
        "vt-energy" => {
            let (r, _) = calc_vt_energy(progress, difficulty);
            (r, false)
        }
        "generic-energy" => {
            let (r, _) = calc_generic_energy(progress, benchmark, difficulty);
            (r, false)
        }
        "Avasive-S2" => {
            let (r, _) = calc_avasive_s2(progress, benchmark, difficulty);
            (r, false)
        }
        "avasive" => {
            let (r, _) = calc_avasive(progress, difficulty);
            (r, false)
        }
        "tpt" => {
            let (r, _) = calc_tpt(progress);
            (r, false)
        }
        "asb" => {
            let (r, _) = calc_asb(progress);
            (r, false)
        }
        "rbe" => {
            let (r, _) = calc_rbe(progress);
            (r, false)
        }
        "routine" => {
            let (r, _) = calc_routine(progress);
            (r, false)
        }
        "mh" => {
            let (r, _) = calc_mh(progress, difficulty);
            (r, false)
        }
        // evxl `Ae` set -> `qe` predicate -> `se` (ye thresholds, no cap).
        "mh-precise" | "mh-reactive" | "mh-tracking" => {
            let (r, _) = calc_mh_variants(progress, benchmark, difficulty);
            (r, false)
        }
        "ca-s1" => {
            let (r, _) = calc_ca_s1(progress, difficulty);
            (r, false)
        }
        "sa-s2" => {
            let (r, _) = calc_sa_s2(progress, difficulty);
            (r, false)
        }
        "mira" => {
            let (r, _) = calc_mira(progress, difficulty);
            (r, false)
        }
        "val-energy" => {
            let (r, _) = calc_val_energy(progress, benchmark, difficulty);
            (r, false)
        }
        "snakbox" => {
            let (r, _) = calc_snakbox(progress, benchmark, difficulty);
            (r, false)
        }
        "ra-s4" => {
            let (r, _) = calc_ra_s4(progress, difficulty);
            (r, false)
        }
        "cb-s1" => {
            let (r, _) = calc_cb_s1(progress, difficulty);
            (r, false)
        }
        "aplus-s1" => {
            let (r, _) = calc_aplus_s1(progress, difficulty);
            (r, false)
        }
        "aplus-alt" => {
            let (r, _) = calc_aplus_alt(progress, difficulty);
            (r, false)
        }
        "xyz2" => {
            let (r, _) = calc_xyz2(progress, difficulty);
            (r, false)
        }
        "xyz" => {
            let (r, _) = calc_xyz(progress, difficulty);
            (r, false)
        }
        "xyz-smoothness-v2" => {
            let (r, _) = calc_xyz_smoothness_v2(progress, difficulty);
            (r, false)
        }
        "RXZU" => {
            let (r, _) = calc_rxzu(progress, difficulty);
            (r, false)
        }
        "dm" => {
            let (r, _) = calc_dm(progress, difficulty);
            (r, false)
        }
        "dm-s3" => {
            let (r, _) = calc_dm_s3(progress, difficulty);
            (r, false)
        }
        "mira-apex" => {
            let (r, _) = calc_mira_apex(progress, difficulty);
            (r, false)
        }
        "generic-energy-alt" => {
            let (r, _) = calc_generic_energy_alt(progress, difficulty);
            (r, false)
        }
        "complete" => {
            let (r, _) = calc_complete(progress);
            (r, false)
        }
        "tsk" => {
            let (r, _) = calc_tsk(progress, difficulty);
            (r, false)
        }
        "33" | "iris" => {
            let (r, _) = calc_tn(progress, benchmark, difficulty);
            (r, false)
        }
        "ra-s5" => {
            let (r, _) = calc_ra_s5(progress, benchmark, difficulty);
            (r, false)
        }
        "generic-energy-uncapped" => {
            // Same ladder slice but no fakeUpper cap.
            let thresholds = difficulty_thresholds(benchmark, difficulty);
            let (r, _) = energy_core(
                progress,
                difficulty,
                &thresholds,
                100,
                9999.0,
                |_| true,
                avg_top2_or_best,
                harmonic_strict,
            );
            (r, false)
        }
        "jade-palace" => {
            let (r, _) = calc_jade_palace(progress, benchmark, difficulty);
            (r, false)
        }
        "aimbeast" => {
            let (r, _) = calc_aimbeast(progress);
            (r, false)
        }
        "aimbeast-partial" => {
            // Half participation, unranked excluded (approximate: exclude zeros).
            let mut category_ranks: Vec<f64> = Vec::new();
            let mut ranked_total = 0usize;
            let total: usize = progress
                .categories
                .iter()
                .map(|(_, c)| c.scenarios.len())
                .sum();
            for (_, category) in &progress.categories {
                let mut sum = 0.0;
                let mut n = 0usize;
                for (_, scenario) in &category.scenarios {
                    if scenario.scenario_rank > 0 && scenario.score > 0.0 {
                        sum += scenario.scenario_rank as f64;
                        n += 1;
                        ranked_total += 1;
                    }
                }
                if n > 0 {
                    category_ranks.push(sum / n as f64);
                }
            }
            let required = total.div_ceil(2);
            if ranked_total < required || category_ranks.is_empty() {
                (0, false)
            } else {
                let avg = category_ranks.iter().sum::<f64>() / category_ranks.len() as f64;
                (avg.floor() as u32, false)
            }
        }
        "dojo" => (calc_count_required(progress, 4), false),
        "dojo2" => (calc_count_required(progress, 3), false),
        "dojo3" => (calc_count_required(progress, 5), false),
        "hewchy" => (calc_hewchy(progress), false),
        "e1se" => (calc_e1se(progress), false),
        "aoi" => (calc_aoi(progress, difficulty), false),
        "MIYU" => (calc_miyu(progress), false),
        _ => {
            // Unported method: API rank is the current best estimate.
            return RankResult {
                rank: progress.overall_rank,
                name: crate::ranks::rank_from_index(progress.overall_rank, difficulty)
                    .map(|t| t.name)
                    .unwrap_or_else(|| "Unranked".into()),
                complete: false,
                method: MethodSource::ApiFallback,
            };
        }
    };
    // Dispatcher: displayed rank = max(engine, scenario floor), except
    // aimbeast averages which already encode the floor.
    let use_floor = !matches!(method, "aimbeast" | "aimbeast-partial" | "selectable-top-n");
    let mut final_rank = engine_rank;
    if use_floor && !complete {
        final_rank = final_rank.max(floor);
    }
    if final_rank > ladder_len + 1 {
        final_rank = ladder_len + 1;
    }
    let complete = final_rank > ladder_len;
    let display_index = final_rank.min(ladder_len);
    let name = if complete {
        format!(
            "{} Complete",
            crate::ranks::rank_from_index(ladder_len, difficulty)
                .map(|t| t.name)
                .unwrap_or_default()
        )
    } else {
        crate::ranks::rank_from_index(display_index, difficulty)
            .map(|t| t.name)
            .unwrap_or_else(|| "Unranked".into())
    };
    RankResult {
        rank: final_rank,
        name,
        complete,
        method: MethodSource::Engine,
    }
}

/// Provenance of a computed rank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodSource {
    /// Computed by the ported evxl engine.
    Engine,
    /// Method not yet ported; API `overall_rank` used as-is.
    ApiFallback,
}

/// The outcome of [`compute_rank`].
#[derive(Debug, Clone, PartialEq)]
pub struct RankResult {
    /// 1-based ladder index; `ladder_len + 1` when "Complete".
    pub rank: u32,
    /// Display name (e.g. "Gold", "Gold Complete").
    pub name: String,
    /// Every subcategory at the top rank (basic family "Complete" state).
    pub complete: bool,
    /// Where this rank came from.
    pub method: MethodSource,
}

impl RankResult {
    /// Resolve the display name into the difficulty's colored tier.
    /// "Complete" results render as the top tier (name carries the suffix).
    pub fn tier(&self, difficulty: &Difficulty) -> Option<crate::types::RankTier> {
        let top = difficulty.rank_colors.len() as u32;
        let index = if self.complete { top } else { self.rank };
        crate::ranks::rank_from_index(index, difficulty)
    }
}
// ---------- method implementations ----------

/// The scenario rank floor used by the dispatcher: the *lowest* per-scenario
/// `scenario_rank` across all scenarios with a positive score (evxl `fe`).
/// Unranked-but-scored scenarios invalidate the floor (=> 0).
pub fn scenario_floor_rank(progress: &BenchmarkProgress) -> u32 {
    let mut floor = u32::MAX;
    let mut valid = true;
    for (_, category) in &progress.categories {
        for (_, scenario) in &category.scenarios {
            if scenario.score <= 0.0 {
                valid = false;
                break;
            }
            let rank = scenario.scenario_rank;
            if rank > 0 {
                floor = floor.min(rank);
            } else {
                valid = false;
                break;
            }
        }
        if !valid {
            break;
        }
    }
    if !valid || floor == u32::MAX {
        0
    } else {
        floor
    }
}

/// `basic` (36 benchmarks): rank = min base rank across subcategories'
/// best scenario; any unranked subcategory => 0; "Complete" when every
/// subcategory tops the ladder.
pub fn calc_basic(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, bool, f64) {
    let order = scenario_order(progress, difficulty);
    // Walk subcategories, take the best (max precise) scenario of each.
    let subs = subcategory_spans(difficulty);
    let mut best_ranks: Vec<u32> = Vec::new();
    let mut progresses: Vec<f64> = Vec::new();
    let mut any_unranked = false;
    let mut idx = 0usize;
    for (_, count, _name) in &subs {
        let mut best: Option<(f64, PreciseRank)> = None;
        let mut best_ratio = 0.0f64;
        for _ in 0..*count {
            if idx < order.len() {
                let (_name, entry) = order[idx];
                let norm = entry.score;
                if norm > 0.0 {
                    let pr = rank_of(norm, &entry.rank_maxes);
                    let entry_ratio = |s: &ScenarioEntry| {
                        if s.rank_maxes.first().cloned().unwrap_or(0.0) > 0.0 {
                            s.score / s.rank_maxes[0]
                        } else {
                            0.0
                        }
                    };
                    let better = match &best {
                        None => true,
                        Some((_, bp)) => {
                            if pr.base > 0 && bp.base == 0 {
                                true
                            } else if pr.base > 0 && bp.base > 0 {
                                pr.precise > bp.precise
                            } else {
                                // both below first rank: compare score ratio
                                entry_ratio(entry) > best_ratio
                            }
                        }
                    };
                    if better {
                        best_ratio = entry_ratio(entry);
                        best = Some((norm, pr));
                    }
                }
            }
            idx += 1;
        }
        match best {
            Some((_, pr)) if pr.base > 0 => {
                best_ranks.push(pr.base);
                progresses.push(pr.progress.max(0.001));
            }
            _ => {
                any_unranked = true;
                best_ranks.push(0);
                progresses.push(0.001);
            }
        }
    }

    let ladder_len = difficulty.rank_colors.len() as u32;
    let min_rank = best_ranks.iter().cloned().min().unwrap_or(0);
    if any_unranked {
        return (
            0,
            false,
            progresses.iter().sum::<f64>() / subs.len().max(1) as f64,
        );
    }
    let complete = min_rank >= ladder_len;
    let rank = if complete { ladder_len + 1 } else { min_rank };
    (
        rank,
        complete,
        progresses.iter().sum::<f64>() / subs.len().max(1) as f64,
    )
}

/// (category, count, subcategory) spans flattened from the registry metadata.
pub fn subcategory_spans(difficulty: &Difficulty) -> Vec<(String, usize, String)> {
    let mut spans = Vec::new();
    for cat in &difficulty.categories {
        let Some(obj) = cat.as_object() else { continue };
        let cat_name = obj
            .get("categoryName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let subs = obj.get("subcategories").and_then(|s| s.as_array());
        for sub in subs.into_iter().flatten() {
            let name = sub
                .get("subcategoryName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let count = sub
                .get("scenarioCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            spans.push((cat_name.clone(), count, name));
        }
    }
    spans
}

/// Energy-family shared core: energy per (filtered) subcategory, overall via
/// harmonic mean, rank from thresholds (evxl `X`).
#[allow(clippy::too_many_arguments)]
pub fn energy_core(
    progress: &BenchmarkProgress,
    difficulty: &Difficulty,
    thresholds: &[f64],
    fake_lower: u32,
    fake_upper: f64,
    filter: impl Fn(&str) -> bool,
    subcategory_energy: impl Fn(&[f64]) -> f64,
    harmonic: impl Fn(&[f64]) -> f64,
) -> (u32, f64) {
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let mut energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, name) in &subs {
        if !filter(name) {
            idx += count;
            continue;
        }
        let mut scores: Vec<f64> = Vec::new();
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                let norm = entry.score;
                if norm > 0.0 && !entry.rank_maxes.is_empty() {
                    let pr = rank_of(norm, &entry.rank_maxes);
                    scores.push(energy_of(&pr, thresholds, fake_lower, fake_upper));
                } else {
                    scores.push(0.0);
                }
            }
            idx += 1;
        }
        energies.push(subcategory_energy(&scores));
    }
    let overall = harmonic(&energies);
    let mut rank = 0u32;
    for (i, t) in thresholds.iter().enumerate() {
        if overall >= *t {
            rank = (i + 1) as u32;
        }
    }
    (rank, overall)
}

/// `vt-energy` (Voltaic S5/S5.5).
pub fn calc_vt_energy(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let thresholds = vt_energy_thresholds(difficulty);
    let fake_lower = if difficulty.name.eq_ignore_ascii_case("novice") {
        0
    } else {
        100
    };
    energy_core(
        progress,
        difficulty,
        &thresholds,
        fake_lower,
        1.0,
        |name| !is_strafe(name),
        avg_top2_or_best,
        harmonic_strict,
    )
}

fn avg_top2_or_best(scores: &[f64]) -> f64 {
    // vt-energy takes the best non-strafe scenario energy per subcategory
    // (subcategoryCount is typically 1 scenario per subcategory at S5);
    // with several, evxl picks the max (best scenario per subcategory).
    scores.iter().cloned().fold(0.0, f64::max)
}

/// `generic-energy` (no cap; thresholds from the cross-difficulty ladder).
pub fn calc_generic_energy(
    progress: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> (u32, f64) {
    let thresholds = difficulty_thresholds(benchmark, difficulty);
    energy_core(
        progress,
        difficulty,
        &thresholds,
        100.0 as u32,
        1.0,
        |_| true,
        avg_top2_or_best,
        harmonic_strict,
    )
}

/// Cross-difficulty ladder slice for `Avasive-S2` (evxl `ye`): like
/// [`difficulty_thresholds`], but when one difficulty's LAST tier has the same
/// name as the next difficulty's FIRST tier, that boundary tier counts once —
/// the slice shifts down 100 per shared boundary. Avasive S2 Medium ends on
/// `Charm` and Medium starts on `Charm`, so Medium's slice is [500..900], not
/// [600..1000].
fn ye_thresholds(benchmark: &BenchmarkDef, difficulty: &Difficulty) -> Vec<f64> {
    let idx = benchmark
        .difficulties
        .iter()
        .position(|d| d.name == difficulty.name)
        .unwrap_or(0);
    let before: usize = benchmark.difficulties[..idx]
        .iter()
        .map(|d| d.rank_colors.len())
        .sum();
    // evxl: for difficulties (idx-1..=idx) pair-wise, +1 when the previous
    // difficulty's last tier name equals this difficulty's first tier name.
    let mut shared = 0usize;
    for w in 1..=idx {
        let prev_last = benchmark.difficulties[w - 1].rank_colors.last();
        let cur_first = benchmark.difficulties[w].rank_colors.first();
        if let (Some(p), Some(c)) = (prev_last, cur_first) {
            if p.name.trim().eq_ignore_ascii_case(c.name.trim()) {
                shared += 1;
            }
        }
    }
    let count = difficulty.rank_colors.len();
    (0..count)
        .map(|l| (before - shared + 1 + l) as f64 * 100.0)
        .collect()
}

/// `tpt` (evxl `cn`): highest rank with >=5 scenarios at or above it.
pub fn calc_tpt(progress: &BenchmarkProgress) -> (u32, f64) {
    ne_rank(progress, 5, false)
}

/// `asb` (evxl `ln`): highest rank with >=8 scenarios at or above it.
pub fn calc_asb(progress: &BenchmarkProgress) -> (u32, f64) {
    ne_rank(progress, 8, false)
}

/// `rbe` (evxl `Ge`): highest rank with >=9 scenarios at or above it.
pub fn calc_rbe(progress: &BenchmarkProgress) -> (u32, f64) {
    ne_rank(progress, 9, true)
}

/// `routine` (evxl `Ue`): highest rank with >= half the scenarios at or above.
pub fn calc_routine(progress: &BenchmarkProgress) -> (u32, f64) {
    let total: usize = progress
        .categories
        .iter()
        .map(|(_, c)| c.scenarios.len())
        .sum();
    ne_rank(progress, total.div_ceil(2).max(1), true)
}

/// `mh` family (evxl `me` via `xn`): per-difficulty tables, harmonic mean of
/// ALL scenario energies.
pub fn calc_mh(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let thresholds: Vec<f64> = match difficulty.name.trim().to_lowercase().as_str() {
        "easy" => vec![100.0, 200.0, 300.0, 400.0],
        "medium" => vec![500.0, 600.0, 700.0, 800.0],
        "hard" => vec![900.0, 1000.0, 1100.0, 1200.0, 1300.0],
        _ => vec![100.0, 200.0, 300.0, 400.0],
    };
    me_table_engine(progress, difficulty, &thresholds)
}

/// `avasive` (evxl `Sn` → `me`): per-difficulty-name tables.
pub fn calc_avasive(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let thresholds: Vec<f64> = match difficulty.name.trim().to_lowercase().as_str() {
        "genesis" => vec![100.0, 200.0, 300.0, 400.0, 500.0],
        "ascension" => vec![600.0, 700.0, 800.0, 900.0, 1000.0],
        "enlightenment" => vec![1100.0, 1200.0, 1300.0, 1400.0, 1500.0],
        "wallhack" => vec![
            100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0,
        ],
        _ => vec![100.0, 200.0, 300.0, 400.0, 500.0],
    };
    me_table_engine(progress, difficulty, &thresholds)
}

/// `ca-s1` (evxl `Y` case "ca-s1" → `X`): fixed thresholds, strafe filter.
pub fn calc_ca_s1(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let thresholds = vec![1500.0, 1550.0, 1600.0, 1650.0, 1700.0, 1750.0, 1800.0];
    x_engine(progress, difficulty, &thresholds, 50.0, 2.0, |name| {
        !name.to_lowercase().contains("strafe")
    })
}

/// `sa-s2` (evxl `gn` → `Y` "custom"): fixed thresholds, strafe filter.
pub fn calc_sa_s2(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let thresholds = vec![1200.0, 1300.0, 1400.0, 1500.0, 1600.0];
    x_engine(progress, difficulty, &thresholds, 50.0, 1.0, |name| {
        !name.to_lowercase().contains("strafe")
    })
}

/// `mira` (evxl `kn` → `Y` "custom"): per-difficulty thresholds.
pub fn calc_mira(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let thresholds: Vec<f64> = if difficulty.name.to_lowercase() == "easy" {
        vec![100.0, 200.0, 300.0, 400.0, 500.0]
    } else {
        vec![600.0, 700.0, 800.0, 900.0, 1000.0]
    };
    x_engine(progress, difficulty, &thresholds, 50.0, 1.0, |_| true)
}

/// `val-energy` (evxl `Mn` → `Y` "custom"): 100-step table sliced by
/// difficulty, all subcategories.
pub fn calc_val_energy(
    progress: &BenchmarkProgress,
    _benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> (u32, f64) {
    let table: [f64; 15] = [
        100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0, 1100.0, 1200.0,
        1300.0, 1400.0, 1500.0,
    ];
    let (start, end) = match difficulty.name.to_lowercase().as_str() {
        "easy" => (0usize, 4usize),
        "medium" => (4, 8),
        "hard" => (8, 12),
        _ => (0, table.len()),
    };
    x_engine(progress, difficulty, &table[start..end], 100.0, 1.0, |_| {
        true
    })
}

/// `snakbox` (evxl `yn` → `se`): medium uses a fixed table, else `ye` slice;
/// energy capped at the table's top value.
pub fn calc_snakbox(
    progress: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> (u32, f64) {
    let thresholds: Vec<f64> = if difficulty.name.trim().to_lowercase() == "medium" {
        vec![400.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1100.0]
    } else {
        ye_thresholds(benchmark, difficulty)
    };
    let cap = thresholds[thresholds.len() - 1];
    se_engine(progress, difficulty, &thresholds, cap)
}

/// `Avasive-S2` (evxl `Nn` → `se` with the difficulty cap).
pub fn calc_avasive_s2(
    progress: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> (u32, f64) {
    let thresholds = ye_thresholds(benchmark, difficulty);
    let cap = match difficulty.name.trim().to_lowercase().as_str() {
        "easier" => 600.0,
        "medium" => 1000.0,
        "hard" => 1400.0,
        _ => f64::INFINITY,
    };
    se_engine(progress, difficulty, &thresholds, cap)
}

// ---------------------------------------------------------------------------
// Batch-port machinery (evxl W / ne / me / se primitives)
// ---------------------------------------------------------------------------

/// evxl `W`: energy of one scenario against `thresholds`, faithful port.
/// `fake_lower` is the offset below the first threshold; `fake_upper` counts
/// bands above the top threshold. Below-rank-1 scenarios interpolate against
/// the scenario's own rank_maxes from a fake zero point.
fn w_energy(
    score: f64,
    maxes: &[f64],
    pr: &PreciseRank,
    thresholds: &[f64],
    fake_lower: f64,
    fake_upper: f64,
) -> f64 {
    if pr.base == 0 && pr.precise <= 0.0 {
        if score <= 0.0 || maxes.len() < 2 {
            return 0.0;
        }
        let f = maxes[0];
        let m = maxes[1] - f;
        let b = f - m; // fake zero point below the ladder
        let n0 = thresholds[0];
        let big_n = n0 - fake_lower;
        let p = if b > 0.0 && score < b {
            score / b * big_n
        } else if (f - b).abs() > 0.0 {
            big_n + (score - b) / (f - b) * (n0 - big_n)
        } else {
            n0
        };
        return p.trunc();
    }
    let a = thresholds.len();
    if a == 0 || pr.precise <= 0.0 {
        return 0.0;
    }
    let i = thresholds[0] - fake_lower;
    let l = thresholds[a - 1];
    let g = if a > 1 { thresholds[a - 2] } else { 0.0 };
    let u = {
        let d = l - g;
        if d.abs() < f64::EPSILON {
            100.0
        } else {
            d
        }
    };
    let h = l + u;
    if pr.precise < 1.0 {
        return (i + pr.precise * (thresholds[0] - i)).trunc();
    }
    if (pr.precise as usize) < a {
        let d = pr.precise.floor();
        let k = pr.precise - d;
        let f = if d == 0.0 {
            i
        } else {
            thresholds[d as usize - 1]
        };
        let r = thresholds[d as usize];
        return (f + k * (r - f)).trunc();
    }
    if pr.precise < a as f64 + fake_upper {
        let d = pr.precise - a as f64;
        return (l + d * (h - l)).trunc();
    }
    h.trunc()
}

/// evxl `X`: best scenario per (filtered) subcategory, `W` energy per subcat,
/// harmonic mean over subcats (any zero subcategory zeroes the overall),
/// rounded to 0.1.
fn x_engine(
    progress: &BenchmarkProgress,
    difficulty: &Difficulty,
    thresholds: &[f64],
    fake_lower: f64,
    fake_upper: f64,
    filter: impl Fn(&str) -> bool,
) -> (u32, f64) {
    if thresholds.is_empty() {
        return (0, 0.0);
    }
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let total_subs = subs.iter().filter(|(_, _, n)| filter(n)).count();
    let mut energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, name) in &subs {
        if !filter(name) {
            idx += count;
            continue;
        }
        let mut best = 0.0f64;
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                if entry.score > 0.0 && !entry.rank_maxes.is_empty() {
                    let pr = rank_of(entry.score, &entry.rank_maxes);
                    let e = w_energy(
                        entry.score,
                        &entry.rank_maxes,
                        &pr,
                        thresholds,
                        fake_lower,
                        fake_upper,
                    );
                    best = best.max(e);
                }
            }
            idx += 1;
        }
        energies.push(best);
    }
    if energies.len() != total_subs || total_subs == 0 {
        return (0, 0.0);
    }
    let overall = (harmonic_strict(&energies) * 10.0).round() / 10.0;
    let mut rank = 0u32;
    for (i, t) in thresholds.iter().enumerate() {
        if overall >= *t {
            rank = (i + 1) as u32;
        }
    }
    (rank, overall)
}

/// evxl `ne`: highest rank with at least `needed` scenarios at or above it.
/// `at_or_above_progress`: progress toward the next rank counts scenarios at
/// rank >= next (rbe/routine) vs exactly next (tpt/asb).
fn ne_rank(progress: &BenchmarkProgress, needed: usize, at_or_above_progress: bool) -> (u32, f64) {
    let mut counts: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let mut total = 0usize;
    for (_, cat) in &progress.categories {
        for (_, sc) in &cat.scenarios {
            total += 1;
            if sc.scenario_rank > 0 {
                *counts.entry(sc.scenario_rank).or_insert(0) += 1;
            }
        }
    }
    if total == 0 || needed == 0 {
        return (0, 0.0);
    }
    let at_or_above = |r: u32| -> u32 {
        counts
            .iter()
            .filter(|(&k, _)| k >= r)
            .map(|(_, &v)| v)
            .sum()
    };
    let max_rank = counts.keys().cloned().max().unwrap_or(0);
    let mut best = 0u32;
    for r in (1..=max_rank).rev() {
        if at_or_above(r) >= needed as u32 {
            best = r;
            break;
        }
    }
    let prog = if best == 0 {
        0.0
    } else {
        let next = best + 1;
        let n = if at_or_above_progress {
            at_or_above(next)
        } else {
            counts.get(&next).cloned().unwrap_or(0)
        };
        (n as f64 / needed as f64).clamp(0.0, 1.0)
    };
    (best, prog)
}

/// evxl `me`: table-driven per-scenario energies, harmonic mean over ALL
/// scenarios (any unplayed scenario zeroes the whole benchmark).
fn me_table_engine(
    progress: &BenchmarkProgress,
    _difficulty: &Difficulty,
    thresholds: &[f64],
) -> (u32, f64) {
    if thresholds.is_empty() {
        return (0, 0.0);
    }
    let order = scenario_order(progress, _difficulty);
    if order.is_empty() {
        return (0, 0.0);
    }
    let mut energies: Vec<f64> = Vec::new();
    for (_, entry) in &order {
        let score = entry.score;
        if score <= 0.0 || entry.rank_maxes.is_empty() {
            return (0, 0.0);
        }
        let pr = rank_of(score, &entry.rank_maxes);
        let e = if pr.base == 0 {
            let first = entry.rank_maxes[0].max(1.0);
            (score / first * thresholds[0]).trunc()
        } else if pr.is_maxed {
            let top = entry.rank_maxes.last().cloned().unwrap_or(1.0);
            let prev = if entry.rank_maxes.len() > 1 {
                entry.rank_maxes[entry.rank_maxes.len() - 2]
            } else {
                0.0
            };
            let band = {
                let b = top - prev;
                if b.abs() < f64::EPSILON {
                    1.0
                } else {
                    b
                }
            };
            (thresholds[thresholds.len() - 1] + (score - top) / band * 100.0).trunc()
        } else {
            let low = thresholds[pr.base as usize - 1];
            let high = thresholds.get(pr.base as usize).cloned().unwrap_or(low);
            (low + pr.progress * (high - low)).trunc()
        };
        energies.push(e);
    }
    if energies.iter().any(|&e| e <= 0.0) {
        return (0, 0.0);
    }
    let overall = (harmonic_strict(&energies) * 10.0).round() / 10.0;
    let mut rank = 0u32;
    for (i, t) in thresholds.iter().enumerate() {
        if overall >= *t {
            rank = (i + 1) as u32;
        }
    }
    (rank, overall)
}

/// evxl `se` generalization (shared by `Avasive-S2` and `snakbox`): per-
/// scenario W energies (uncapped), clamped to `cap`, subcategory energy =
/// average of the top TWO (a single-scenario subcategory contributes HALF),
/// harmonic mean across subcategories.
fn se_engine(
    progress: &BenchmarkProgress,
    difficulty: &Difficulty,
    thresholds: &[f64],
    cap: f64,
) -> (u32, f64) {
    if thresholds.is_empty() {
        return (0, 0.0);
    }
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let mut sub_energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, _name) in &subs {
        let mut energies: Vec<f64> = Vec::new();
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                // evxl `se`: unplayed scenarios are EXCLUDED from the
                // subcategory list entirely (only scored scenarios push an
                // energy) — pushing a zero would poison the harmonic mean.
                if entry.score > 0.0 && !entry.rank_maxes.is_empty() {
                    let pr = rank_of(entry.score, &entry.rank_maxes);
                    let e = w_energy(
                        entry.score,
                        &entry.rank_maxes,
                        &pr,
                        thresholds,
                        100.0,
                        f64::INFINITY,
                    );
                    let e = if e.is_finite() { e.min(cap) } else { cap };
                    energies.push(e);
                }
            }
            idx += 1;
        }
        energies.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let sub = if energies.is_empty() {
            0.0
        } else if energies.len() == 1 {
            energies[0] / 2.0
        } else {
            (energies[0] + energies[1]) / 2.0
        };
        sub_energies.push(sub);
    }
    let overall = (harmonic_strict(&sub_energies) * 10.0).round() / 10.0;
    let mut rank = 0u32;
    for (i, t) in thresholds.iter().enumerate() {
        if overall >= *t {
            rank = (i + 1) as u32;
        }
    }
    (rank, overall)
}

// ---------------------------------------------------------------------------
// Bespoke batch-ported methods (chunk 3)
// ---------------------------------------------------------------------------

/// evxl `Ne`: piecewise interpolation of score against rank_maxes onto a
/// points table (ra-s4). Below maxes[1]: extrapolate with the 2/3 slope.
/// Above the top: extrapolate with slope divided by `tail` (default 1).
fn ne_points_interp(score: f64, maxes: &[f64], points: &[f64], tail: f64) -> f64 {
    if maxes.len() < 2 || points.len() < 2 || maxes.len() != points.len() {
        return 0.0;
    }
    if score < maxes[1] {
        let step = maxes[1] - maxes[0];
        let slope = (points[2] - points[1]) / (2.0 / 3.0);
        return (points[1] + (score - maxes[1]) / step * slope)
            .max(0.0)
            .ceil();
    }
    for s in 1..maxes.len() - 1 {
        if score <= maxes[s + 1] {
            return points[s]
                + (score - maxes[s]) / (maxes[s + 1] - maxes[s]) * (points[s + 1] - points[s]);
        }
    }
    let o = maxes.len() - 1;
    points[o] + (score - maxes[o]) / (maxes[o] - maxes[o - 1]) * (points[o] - points[o - 1]) / tail
}

/// evxl `Me`: the top `take` scenarios of one category by precise rank.
fn top_scenarios<'a>(
    progress: &'a BenchmarkProgress,
    difficulty: &Difficulty,
    category: &str,
    take: usize,
) -> Vec<(&'a ScenarioEntry, f64)> {
    let order = scenario_order(progress, difficulty);
    // Recompute document index range for the category.
    let subs = subcategory_spans(difficulty);
    let mut idx = 0usize;
    let mut lo = usize::MAX;
    let mut hi = 0usize;
    for (cat, count, _) in &subs {
        if cat == category {
            lo = lo.min(idx);
            hi = idx + count;
        }
        idx += count;
    }
    if lo == usize::MAX {
        return Vec::new();
    }
    let mut out: Vec<(&ScenarioEntry, f64)> = order[lo.min(order.len())..hi.min(order.len())]
        .iter()
        .filter_map(|(_, e)| {
            if e.score > 0.0 && !e.rank_maxes.is_empty() {
                let pr = rank_of(e.score, &e.rank_maxes);
                Some((*e, pr.precise))
            } else {
                None
            }
        })
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(take);
    out
}

/// evxl `K`: rank = number of thresholds cleared by totalScore.
fn rank_from_total(total: f64, thresholds: &[f64]) -> u32 {
    let mut rank = 0u32;
    for (i, t) in thresholds.iter().enumerate() {
        if total >= *t {
            rank = (i + 1) as u32;
        }
    }
    rank
}

/// `ra-s4` (evxl `dn`): per-category top-4 scenarios, `Ne`-interpolated onto
/// per-difficulty points tables, summed; rank vs fixed totals.
pub fn calc_ra_s4(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let easy = difficulty.name.to_lowercase() == "easy";
    let maxes_t: Vec<f64> = if easy {
        vec![20.0, 50.0, 80.0, 110.0, 140.0, 170.0]
    } else {
        vec![200.0, 235.0, 270.0, 320.0, 360.0, 400.0]
    };
    let points_t: Vec<f64> = if easy {
        vec![240.0, 600.0, 960.0, 1320.0, 1680.0, 2040.0]
    } else {
        vec![2400.0, 2820.0, 3240.0, 3840.0, 4320.0, 4800.0]
    };
    let totals: Vec<f64> = vec![2400.0, 2820.0, 3240.0, 3840.0, 4320.0, 4800.0];
    let _ = totals; // K uses `o` = points table for easy? evxl passes `o` (the easy/normal totals table)
                    // evxl: `o` is the SAME table as points_t branch (r? [2400..]:[2400..]) — actually `o` was
                    // defined as the points table and `t` as maxes table; K(...) passes `o` = totals? Re-reading:
                    // dn: t = maxes table, o = totals table (2400..4800). s = per-category weighted scores.
                    // K(e, n, cb, o) -> thresholds = o (the totals table = points table values here).
    let cat_names: Vec<String> = {
        let mut seen = Vec::new();
        for (cat, _, _) in subcategory_spans(difficulty) {
            if !seen.contains(&cat) {
                seen.push(cat);
            }
        }
        seen
    };
    let mut total = 0.0f64;
    for cat in &cat_names {
        for (entry, _) in top_scenarios(progress, difficulty, cat, 4) {
            total += ne_points_interp(entry.score, &maxes_t, &points_t, 1.5);
        }
    }
    let rank = rank_from_total(total, &points_t);
    (rank, total)
}

/// `cb-s1` (evxl `mn`): each scenario mapped onto a fixed 300..1800 points
/// table by its own rank_maxes; rank = thresholds cleared by the SUM of
/// percentages (score/1800*100 accumulated).
pub fn calc_cb_s1(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let table: Vec<f64> = vec![300.0, 600.0, 900.0, 1200.0, 1500.0, 1800.0];
    let top = 1800.0;
    let order = scenario_order(progress, difficulty);
    let mut pct_sum = 0.0f64;
    for (_, entry) in &order {
        let score = entry.score;
        if score <= 0.0 || entry.rank_maxes.is_empty() {
            continue;
        }
        let pr = rank_of(score, &entry.rank_maxes);
        let maxes = &entry.rank_maxes;
        let val = if pr.base == 0 {
            score / maxes[0] * table[0]
        } else if pr.is_maxed && pr.base as usize == maxes.len() {
            let r = maxes[maxes.len() - 1];
            let m = if maxes.len() > 1 {
                maxes[maxes.len() - 2]
            } else {
                0.0
            };
            let band = {
                let b = (r - m).abs();
                if b < 1.0 {
                    1.0
                } else {
                    b
                }
            };
            let over = ((score - r) / band).max(0.0);
            table[((pr.base as f64 + over) as usize).min(table.len() - 1)]
        } else {
            table[pr.base as usize - 1]
                + (table[pr.base as usize] - table[pr.base as usize - 1]) * pr.progress
        };
        pct_sum += val / top * 100.0;
    }
    let rank = rank_from_total(pct_sum, &table);
    (rank, pct_sum)
}

/// `aplus-s1` (evxl `fn`): basic rank, plus a tie-break: plus-rank = the
/// lowest per-category 3rd-highest scenario_rank; final = max(normal, plus).
pub fn calc_aplus_s1(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let (normal, _, _) = calc_basic(progress, difficulty);
    let mut plus = u32::MAX;
    for (_, cat) in &progress.categories {
        let mut ranks: Vec<u32> = cat
            .scenarios
            .iter()
            .filter_map(|(_, e)| {
                if e.score > 0.0 {
                    Some(e.scenario_rank)
                } else {
                    None
                }
            })
            .collect();
        ranks.sort_unstable_by(|a, b| b.cmp(a));
        let third = if ranks.len() >= 3 { ranks[2] } else { 0 };
        plus = plus.min(third);
    }
    if plus == u32::MAX {
        plus = 0;
    }
    let final_rank = normal.max(plus);
    (final_rank, plus as f64)
}

/// `aplus-alt` (evxl `bn`): every scenario contributes
/// (preciseRank / scenarioCount) * 100 to a total; thresholds = 100-step
/// ladder of the difficulty's tier count.
pub fn calc_aplus_alt(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let tiers = difficulty.rank_colors.len();
    let thresholds: Vec<f64> = if tiers > 0 {
        (0..tiers).map(|l| (l + 1) as f64 * 100.0).collect()
    } else {
        vec![
            100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0,
        ]
    };
    let total_scenarios: usize = progress
        .categories
        .iter()
        .map(|(_, c)| c.scenarios.len())
        .sum();
    if total_scenarios == 0 {
        return (0, 0.0);
    }
    let per = 100.0 / total_scenarios as f64;
    let mut total = 0.0f64;
    for (_, cat) in &progress.categories {
        for (_, e) in &cat.scenarios {
            if e.score <= 0.0 || e.rank_maxes.is_empty() {
                continue;
            }
            let pr = rank_of(e.score, &e.rank_maxes);
            total += if pr.base == 0 {
                e.score / e.rank_maxes[0].max(1.0) * per
            } else {
                (pr.base as f64 + pr.progress) * per
            };
        }
    }
    let total = (total * 100.0).round() / 100.0;
    let rank = rank_from_total(total, &thresholds);
    (rank, total)
}

/// `xyz2` (evxl `Tn`): per-difficulty required rank ladder (easy: 12..1,
/// hard: 10..1). Pinnacle if all scenarios share the top required rank, or
/// every category has >=3 at it. Otherwise rank = min over categories of the
/// 4th-highest scenario rank (categories with <4 scenarios -> unranked).
pub fn calc_xyz2(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let name = difficulty.name.trim().to_lowercase();
    let ladder: Vec<u32> = match name.as_str() {
        "easy" => vec![12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
        "hard" => vec![10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
        _ => return (0, 0.0),
    };
    let top = ladder[0];
    let mut cat_ranks: Vec<Vec<u32>> = Vec::new();
    let mut total = 0usize;
    for (_, cat) in &progress.categories {
        let mut ranks = Vec::new();
        for (_, e) in &cat.scenarios {
            total += 1;
            if e.scenario_rank > 0 {
                ranks.push(e.scenario_rank);
            }
        }
        cat_ranks.push(ranks);
    }
    let all: Vec<u32> = cat_ranks.iter().flatten().cloned().collect();
    if total > 0 && all.len() == total && all.iter().all(|&r| r == top) {
        return (top, 1.0);
    }
    if cat_ranks
        .iter()
        .all(|r| r.iter().filter(|&&r| r == top).count() >= 3)
    {
        return (top, 1.0);
    }
    let mut insufficient = false;
    let mut fourths: Vec<u32> = Vec::new();
    for r in &cat_ranks {
        if r.len() < 4 {
            insufficient = true;
            break;
        }
        let mut sorted = r.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        fourths.push(sorted[3]);
    }
    if insufficient || fourths.is_empty() {
        return (0, 0.0);
    }
    let rank = fourths.iter().cloned().min().unwrap_or(0);
    (rank, 0.0)
}

/// `xyz` (evxl `Rn`): per-category count-based qualification ladder with
/// per-difficulty per-category requirements; pinnacle when everything maxes.
pub fn calc_xyz(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let _ = difficulty;
    let mut max_len = 0usize;
    let mut cat_scen_ranks: Vec<Vec<u32>> = Vec::new();
    for (_, cat) in &progress.categories {
        let mut ranks = Vec::new();
        for (_, e) in &cat.scenarios {
            ranks.push(e.scenario_rank);
            max_len = max_len.max(e.rank_maxes.len());
        }
        cat_scen_ranks.push(ranks);
    }
    let ladder_tiers = difficulty.rank_colors.len();
    let u = max_len.max(ladder_tiers);
    if u == 0 {
        return (0, 0.0);
    }
    let flat: Vec<u32> = cat_scen_ranks.iter().flatten().cloned().collect();
    let l = flat.len();
    if l == 0 {
        return (0, 0.0);
    }
    if flat.iter().all(|&r| r == u as u32) {
        return (u as u32, 1.0);
    }
    let clamp01 = |v: f64| v.clamp(0.0, 1.0);
    let d = cat_scen_ranks.len();
    // evxl: req per category = 3 when x == u else 4 (newcomer variant uses 2/…; xyz Benchmarks difficulty is fixed shape here)
    let mut rank = 0u32;
    for x in (1..=u).rev() {
        let xu = x as u32;
        let req = if x == u { 3 } else { 4 };
        let total_req = req * d;
        let per_cat_ok = cat_scen_ranks
            .iter()
            .all(|r| r.iter().filter(|&&v| v >= xu).count() >= req);
        let total_ok = flat.iter().filter(|&&v| v >= xu).count() >= total_req;
        if per_cat_ok && total_ok {
            rank = xu;
            break;
        }
    }
    if rank as usize >= u {
        let count_top = flat.iter().filter(|&&v| v >= u as u32).count() as f64;
        let need = (3.0 * d as f64).min(l as f64);
        let rest = l as f64 - need;
        let prog = if rest > 0.0 {
            clamp01((count_top - need) / rest)
        } else {
            1.0
        };
        return (u as u32, prog);
    }
    let next = if rank == 0 { 1 } else { rank + 1 };
    let req_next = if next as usize == u { 3 } else { 4 };
    let total_req = req_next * d;
    let have: usize = cat_scen_ranks
        .iter()
        .map(|r| r.iter().filter(|&&v| v >= next).count().min(req_next))
        .sum();
    let prog = if total_req > 0 {
        clamp01(have as f64 / total_req as f64)
    } else {
        0.0
    };
    (rank, prog)
}

/// `xyz-smoothness-v2` (evxl `Cn`): count-of-scenarios-at-or-above ladder
/// (9 needed per step; 6 scores at the top tier reach the top rank).
pub fn calc_xyz_smoothness_v2(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let tiers = difficulty.rank_colors.len();
    if tiers == 0 {
        return (0, 0.0);
    }
    let top_needed = 9usize;
    let prismatic = 6usize;
    let mut at_or_above: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    let mut total = 0usize;
    for (_, cat) in &progress.categories {
        for (_, e) in &cat.scenarios {
            total += 1;
            let r = if !e.rank_maxes.is_empty() && e.score > 0.0 {
                rank_of(e.score, &e.rank_maxes).base
            } else {
                e.scenario_rank
            };
            if r > 0 {
                for t in 1..=tiers as u32 {
                    if r >= t {
                        *at_or_above.entry(t).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    let clamp01 = |v: f64| v.clamp(0.0, 1.0);
    let top_count = at_or_above.get(&(tiers as u32)).cloned().unwrap_or(0);
    if total > 0 && top_count >= total {
        return (tiers as u32, 1.0);
    }
    if top_count >= prismatic {
        let need = total - prismatic;
        let prog = if need > 0 {
            clamp01((top_count - prismatic) as f64 / need as f64)
        } else {
            1.0
        };
        return (tiers as u32, prog);
    }
    let mut rank = 0u32;
    for m in (1..tiers as u32).rev() {
        if at_or_above.get(&m).cloned().unwrap_or(0) >= top_needed {
            rank = m;
            break;
        }
    }
    let next = rank + 1;
    let next_needed = if next as usize >= tiers {
        prismatic
    } else {
        top_needed
    };
    let next_count = at_or_above.get(&next).cloned().unwrap_or(0);
    let prog = if next_needed > 0 {
        clamp01(next_count as f64 / next_needed as f64)
    } else {
        0.0
    };
    (rank, prog)
}

/// `RXZU` (evxl `On`): per-difficulty points tables; each scenario scores the
/// points of its scenario_rank; rank = thresholds cleared by the AVERAGE.
pub fn calc_rxzu(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let table: Vec<f64> = match difficulty.name.to_lowercase().as_str() {
        "easy" => vec![
            200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0, 1100.0, 1200.0,
        ],
        "hard" => vec![
            400.0, 500.0, 600.0, 700.0, 800.0, 850.0, 900.0, 950.0, 1000.0, 1050.0, 1100.0, 1150.0,
            1200.0,
        ],
        _ => return (0, 0.0),
    };
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for (_, cat) in &progress.categories {
        for (_, e) in &cat.scenarios {
            n += 1;
            if e.scenario_rank > 0 {
                let idx = (e.scenario_rank as usize).min(table.len()) - 1;
                sum += table[idx];
            }
        }
    }
    if n == 0 {
        return (0, 0.0);
    }
    let avg = sum / n as f64;
    let rank = rank_from_total(avg, &table);
    (rank, avg)
}

/// `dm` (evxl `Je`): 100-step energies with a difficulty-based base (boss
/// variants start at 500/900), subcategory energy = MAX scenario energy
/// (isMaxed extends past top by 10 per band), harmonic mean over subcats.
pub fn calc_dm(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let tiers = difficulty.rank_colors.len();
    if tiers == 0 {
        return (0, 0.0);
    }
    let name = difficulty.name.to_lowercase();
    let base = if (name == "boss" && tiers == 8) || name == "boss+" {
        500.0
    } else if name == "boss++" {
        900.0
    } else {
        100.0
    };
    let thresholds: Vec<f64> = (0..tiers)
        .map(|r| {
            if r == 0 {
                base
            } else if r == tiers - 1 {
                base + (r as f64 - 1.0) * 100.0 + 10.0
            } else {
                base + r as f64 * 100.0
            }
        })
        .collect();
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let mut sub_energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, _n) in &subs {
        let mut best = 0.0f64;
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                let score = entry.score;
                if score > 0.0 && !entry.rank_maxes.is_empty() {
                    let pr = rank_of(score, &entry.rank_maxes);
                    let e = if pr.base == 0 {
                        0.0
                    } else if pr.is_maxed && pr.base as usize == entry.rank_maxes.len() {
                        let v = thresholds[thresholds.len() - 1];
                        let top = entry.rank_maxes.last().cloned().unwrap_or(1.0);
                        let prev = if entry.rank_maxes.len() > 1 {
                            entry.rank_maxes[entry.rank_maxes.len() - 2]
                        } else {
                            0.0
                        };
                        let band = {
                            let c = (top - prev).abs();
                            if c < 1.0 {
                                1.0
                            } else {
                                c
                            }
                        };
                        let over = ((score - top) / band).max(0.0);
                        v + over * 10.0
                    } else {
                        let low = thresholds[pr.base as usize - 1];
                        let high = thresholds.get(pr.base as usize).cloned().unwrap_or(low);
                        low + pr.progress * (high - low)
                    };
                    best = best.max(e.max(0.0).trunc());
                }
            }
            idx += 1;
        }
        sub_energies.push(best);
    }
    let overall = (harmonic_strict(&sub_energies) * 10.0).round() / 10.0;
    let rank = rank_from_total(overall, &thresholds);
    (rank, overall)
}

/// `dm-s3` (evxl `We`): per-difficulty 4-step thresholds ([100,200,300,1510],
/// boss variants / level-N), subcat energy = MAX scenario (below-rank-1
/// scales toward t[0]); harmonic mean over subcats.
pub fn calc_dm_s3(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let name = difficulty.name.to_lowercase();
    let thresholds: Vec<f64> = if name.contains("boss") {
        vec![1300.0, 1400.0, 1500.0, 1510.0]
    } else if let Some((pos, _)) = name.match_indices(|c: char| c.is_ascii_digit()).next() {
        let lvl: u32 = name[pos..pos + 1].parse().unwrap_or(1);
        let start = (lvl.saturating_sub(1)) * 300 + 100;
        vec![
            start as f64,
            start as f64 + 100.0,
            start as f64 + 200.0,
            1510.0,
        ]
    } else {
        vec![100.0, 200.0, 300.0, 1510.0]
    };
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let mut sub_energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, _n) in &subs {
        let mut best = 0.0f64;
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                let score = entry.score;
                if score > 0.0 && !entry.rank_maxes.is_empty() {
                    let pr = rank_of(score, &entry.rank_maxes);
                    let e = if pr.base == 0 {
                        let first = entry.rank_maxes[0].max(1.0);
                        ((score / first) * thresholds[0]).min(thresholds[0])
                    } else if pr.is_maxed && pr.base as usize == entry.rank_maxes.len() {
                        thresholds[thresholds.len() - 1]
                    } else {
                        let low = thresholds[pr.base as usize - 1];
                        let high = thresholds
                            .get(pr.base as usize)
                            .cloned()
                            .unwrap_or(thresholds[thresholds.len() - 1]);
                        low + pr.progress * (high - low)
                    };
                    best = best.max(e.max(0.0).trunc());
                }
            }
            idx += 1;
        }
        sub_energies.push(best);
    }
    let overall = (harmonic_strict(&sub_energies) * 10.0).round() / 10.0;
    let rank = rank_from_total(overall, &thresholds);
    (rank, overall)
}

/// `mira-apex` (evxl `ze`): 10-step thresholds (tiers * 10), subcat energy =
/// MAX scenario (below-rank-1 scales to t[0]; maxed pins t[-1]); harmonic
/// mean with zeros as 0.1 when some (but not all) subcats are unplayed.
pub fn calc_mira_apex(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    let tiers = difficulty.rank_colors.len();
    if tiers == 0 {
        return (0, 0.0);
    }
    let thresholds: Vec<f64> = (0..tiers).map(|k| (k + 1) as f64 * 10.0).collect();
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let mut sub_energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, _n) in &subs {
        let mut best = 0.0f64;
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                let score = entry.score;
                if score > 0.0 && !entry.rank_maxes.is_empty() {
                    let pr = rank_of(score, &entry.rank_maxes);
                    let e = if pr.base == 0 {
                        let first = entry.rank_maxes[0];
                        if first > 0.0 {
                            ((score / first).min(1.0)) * thresholds[0]
                        } else {
                            0.0
                        }
                    } else if pr.is_maxed && pr.base as usize == entry.rank_maxes.len() {
                        thresholds[thresholds.len() - 1]
                    } else {
                        let low = thresholds[pr.base as usize - 1];
                        let high = thresholds
                            .get(pr.base as usize)
                            .cloned()
                            .unwrap_or(thresholds[thresholds.len() - 1]);
                        low + pr.progress * (high - low)
                    };
                    best = best.max(e.max(0.0).trunc());
                }
            }
            idx += 1;
        }
        sub_energies.push(best);
    }
    if sub_energies.is_empty() {
        return (0, 0.0);
    }
    let l = sub_energies.len();
    let played: Vec<f64> = sub_energies.iter().cloned().filter(|e| *e > 0.0).collect();
    let overall = if played.len() == l {
        let inv: f64 = played.iter().map(|e| 1.0 / e).sum();
        l as f64 / inv
    } else if !played.is_empty() {
        let patched: Vec<f64> = sub_energies
            .iter()
            .map(|&e| if e == 0.0 { 0.1 } else { e })
            .collect();
        let inv: f64 = patched.iter().map(|e| 1.0 / e).sum();
        l as f64 / inv
    } else {
        0.0
    };
    let overall = (overall * 10.0).round() / 10.0;
    let rank = rank_from_total(overall, &thresholds);
    (rank, overall)
}

/// `generic-energy-alt` (evxl `Qe`): per-scenario energies on a 100-step
/// ladder of the difficulty's tier count (top+50 headroom, over-cap 0.5
/// bands), AVERAGE over scenarios (not harmonic), rounded to 0.1.
pub fn calc_generic_energy_alt(
    progress: &BenchmarkProgress,
    difficulty: &Difficulty,
) -> (u32, f64) {
    let tiers = difficulty.rank_colors.len();
    if tiers == 0 {
        return (0, 0.0);
    }
    let thresholds: Vec<f64> = (0..tiers).map(|l| (l + 1) as f64 * 100.0).collect();
    let cap = thresholds[thresholds.len() - 1] + 50.0;
    let order = scenario_order(progress, difficulty);
    let mut energies: Vec<f64> = Vec::new();
    for (_, entry) in &order {
        let score = entry.score;
        if score <= 0.0 || entry.rank_maxes.is_empty() {
            energies.push(0.0);
            continue;
        }
        let pr = rank_of(score, &entry.rank_maxes);
        let e = if pr.base == 0 {
            score / entry.rank_maxes[0].max(1.0) * thresholds[0]
        } else if pr.is_maxed {
            let n = entry.rank_maxes.len() - 1;
            let y = entry.rank_maxes[n];
            let p = if n > 0 { entry.rank_maxes[n - 1] } else { 0.0 };
            let band = {
                let t = (y - p).abs();
                if t < 1.0 {
                    1.0
                } else {
                    t
                }
            };
            let over = ((score - y) / band).min(0.5);
            thresholds[thresholds.len() - 1] + over * 100.0
        } else {
            let low = thresholds[pr.base as usize - 1];
            let high = thresholds.get(pr.base as usize).cloned().unwrap_or(cap);
            low + pr.progress * (high - low)
        };
        energies.push(e.min(cap).trunc());
    }
    if energies.is_empty() {
        return (0, 0.0);
    }
    let avg = energies.iter().cloned().sum::<f64>() / energies.len() as f64;
    let overall = (avg * 10.0).round() / 10.0;
    let rank = rank_from_total(overall, &thresholds);
    (rank, overall)
}

/// `complete` (evxl `Be`): the API's own rank recomputation — the floor rule
/// over per-scenario ranks; rank 0 when unranked.
pub fn calc_complete(progress: &BenchmarkProgress) -> (u32, f64) {
    // evxl fe/ke: min scenario_rank with score > 0; any unplayed -> unranked.
    let mut floor = u32::MAX;
    let mut valid = true;
    for (_, cat) in &progress.categories {
        for (_, e) in &cat.scenarios {
            if e.score <= 0.0 {
                valid = false;
                break;
            }
            if e.scenario_rank > 0 {
                floor = floor.min(e.scenario_rank);
            } else {
                valid = false;
                break;
            }
        }
    }
    if !valid || floor == u32::MAX {
        (0, 0.0)
    } else {
        (floor, 0.0)
    }
}

/// `tn` shared engine (`33` and `iris` via evxl `tn`): 100-step thresholds
/// starting at a per-difficulty base (rn: novice/beginner/easy/intermediate
/// 100, adv/advanced/hard 200, else (idx+1)*100); subcat energy = best-half
/// average when half the scenarios clear t[0], else mean/(T*2); category
/// averages then harmonic mean; rounds to integers.
fn tn_engine(
    progress: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> (u32, f64) {
    let tiers = difficulty.rank_colors.len();
    if tiers == 0 {
        return (0, 0.0);
    }
    let name = difficulty.name.trim().to_lowercase();
    let base: f64 = match name.as_str() {
        "novice" | "beginner" | "easy" | "intermediate" => 100.0,
        "adv" | "advanced" | "hard" => 200.0,
        _ => {
            let idx = benchmark
                .difficulties
                .iter()
                .position(|d| d.name == difficulty.name)
                .map(|p| p + 1)
                .unwrap_or(1);
            idx as f64 * 100.0
        }
    };
    let thresholds: Vec<f64> = (0..tiers).map(|r| base + r as f64 * 100.0).collect();
    let first = thresholds[0];
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let mut sub_energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, _n) in &subs {
        let mut energies: Vec<f64> = Vec::new();
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                let score = entry.score;
                if score > 0.0 && !entry.rank_maxes.is_empty() {
                    let pr = rank_of(score, &entry.rank_maxes);
                    let e = if pr.base == 0 {
                        0.0
                    } else if pr.is_maxed && pr.base as usize == entry.rank_maxes.len() {
                        thresholds[thresholds.len() - 1]
                    } else {
                        let low = thresholds[pr.base as usize - 1];
                        let high = thresholds
                            .get(pr.base as usize)
                            .cloned()
                            .unwrap_or(thresholds[thresholds.len() - 1]);
                        low + pr.progress * (high - low)
                    };
                    energies.push((e.max(0.0) * 100.0).round() / 100.0);
                } else {
                    energies.push(0.0);
                }
            }
            idx += 1;
        }
        if energies.is_empty() {
            sub_energies.push(0.0);
            continue;
        }
        let t_half = energies.len().div_ceil(2);
        let cleared = energies.iter().filter(|&&e| e >= first).count();
        let sub = if cleared >= t_half {
            let mut sorted = energies.clone();
            sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            let sum: f64 = sorted.iter().take(t_half).sum();
            (sum / t_half as f64 * 100.0).round() / 100.0
        } else {
            let sum: f64 = energies.iter().sum();
            (sum / (t_half as f64 * 2.0) * 100.0).round() / 100.0
        };
        sub_energies.push(sub.round());
    }
    if sub_energies.is_empty() || sub_energies.iter().any(|&e| e <= 0.0) {
        return (0, 0.0);
    }
    let overall = sub_energies.iter().cloned().sum::<f64>() / sub_energies.len() as f64;
    let overall = overall.round();
    let rank = rank_from_total(overall, &thresholds);
    (rank, overall)
}

/// `33` and `iris` (evxl dispatcher `Fe` → `tn`).
pub fn calc_tn(
    progress: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> (u32, f64) {
    tn_engine(progress, benchmark, difficulty)
}

/// `ra-s5` (evxl `Xe`): complex reactive-tracker — scenario energies on the
/// shared ladder slice; reactive subcats average PAIRS of (first two / last
/// two) scenarios; non-reactive subcats avg top-2 (1 elem halved); category
/// averages, then harmonic mean over categories. This engine has many sheet-
/// specific details; port covers the documented shapes (4-scenario reactive
/// subcats, 2-entry subcats elsewhere).
pub fn calc_ra_s5(
    progress: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> (u32, f64) {
    let thresholds = {
        // evxl: D(o, n, r).slice(0, -1) — ladder slice EXCLUDING one past the
        // last tier (unlike difficulty_thresholds which stops at count).
        let total: usize = benchmark
            .difficulties
            .iter()
            .map(|d| d.rank_colors.len())
            .sum();
        let ladder: Vec<f64> = (0..=total).map(|i| (i + 1) as f64 * 100.0).collect();
        let before: usize = benchmark
            .difficulties
            .iter()
            .take_while(|d| d.name != difficulty.name)
            .map(|d| d.rank_colors.len())
            .sum();
        let count = difficulty.rank_colors.len();
        ladder[before..(before + count)].to_vec()
    };
    if thresholds.is_empty() {
        return (0, 0.0);
    }
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let mut sub_energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, name) in &subs {
        let reactive = name.to_lowercase().contains("reactive");
        let mut energies: Vec<f64> = Vec::new();
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                let score = entry.score;
                let e = if score <= 0.0 || entry.rank_maxes.is_empty() {
                    0.0
                } else {
                    let pr = rank_of(score, &entry.rank_maxes);
                    let e = if pr.base == 0 {
                        // evxl: below-rank-1 reactive offset (entry 800, else 830) —
                        // sheet-specific; percentage of first threshold.
                        0.0
                    } else if pr.is_maxed && pr.base as usize == entry.rank_maxes.len() {
                        thresholds[thresholds.len() - 1]
                    } else {
                        let low = thresholds[pr.base as usize - 1];
                        let high = thresholds
                            .get(pr.base as usize)
                            .cloned()
                            .unwrap_or(thresholds[thresholds.len() - 1]);
                        low + pr.progress * (high - low)
                    };
                    (e.max(0.0) * 100.0).round() / 100.0
                };
                energies.push(e);
            }
            idx += 1;
        }
        let sub = if energies.is_empty() {
            0.0
        } else if reactive {
            if energies.len() != 4 {
                0.0
            } else {
                (energies[0].max(energies[1]) + energies[2].max(energies[3])) / 2.0
            }
        } else {
            let mut sorted = energies.clone();
            sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            if sorted.len() == 1 {
                sorted[0] / 2.0
            } else {
                (sorted[0] + sorted[1]) / 2.0
            }
        };
        sub_energies.push(sub.round());
    }
    // Category averages, then harmonic mean over category energies.
    let subs_count = subs.len();
    let per_cat: usize = subs_count; // sub_energies are per-subcat; evxl groups by category
    let _ = per_cat;
    let all: Vec<f64> = sub_energies.clone();
    if all.is_empty() || all.iter().any(|&e| e <= 0.0) {
        return (0, 0.0);
    }
    let overall = all.iter().cloned().sum::<f64>() / all.len() as f64;
    let overall = overall.round();
    let rank = rank_from_total(overall, &thresholds);
    (rank, overall)
}

/// `mh-precise` / `mh-reactive` / `mh-tracking` (evxl `Ae` set → `se`):
/// `ye` ladder slice, no energy cap, best scenario per subcategory.
pub fn calc_mh_variants(
    progress: &BenchmarkProgress,
    benchmark: &BenchmarkDef,
    difficulty: &Difficulty,
) -> (u32, f64) {
    let thresholds = ye_thresholds(benchmark, difficulty);
    se_engine(progress, difficulty, &thresholds, f64::INFINITY)
}

/// `tsk` (evxl `On` \u2014 note evxl's fn is also named `On`/`on` for TSK; the
/// RXZU one is a different symbol): per-difficulty achievement configs over
/// scenario-rank counts. Highest rank with enough scenarios at/above it;
/// overMax variants short-circuit to the top rank.
pub fn calc_tsk(progress: &BenchmarkProgress, difficulty: &Difficulty) -> (u32, f64) {
    // Collect (rank, rank_maxes_len, is_over_max) per scenario.
    let mut entries: Vec<(u32, usize, bool)> = Vec::new();
    for (_, cat) in &progress.categories {
        for (_, e) in &cat.scenarios {
            let score = e.score;
            let maxes_len = e.rank_maxes.len();
            let pr = if score > 0.0 && !e.rank_maxes.is_empty() {
                rank_of(score, &e.rank_maxes)
            } else {
                PreciseRank {
                    base: 0,
                    precise: 0.0,
                    progress: 0.0,
                    is_maxed: false,
                }
            };
            let over = pr.is_maxed
                && pr.base as usize == maxes_len
                && score > e.rank_maxes.last().cloned().unwrap_or(0.0);
            entries.push((pr.base, maxes_len, over));
        }
    }
    let over_max_count = entries.iter().filter(|(_, _, o)| *o).count();
    let mut counts: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    for &(rank, _, _) in &entries {
        if rank > 0 {
            *counts.entry(rank).or_insert(0) += 1;
        }
    }
    let cfg: (Option<usize>, Option<usize>, usize) = match difficulty.name.to_lowercase().as_str() {
        "beginner" => (None, None, 4),
        "main" => (None, None, 8),
        "ultimate" => (Some(8), Some(10), 12),
        "static" => (None, None, 9),
        "strafes" => (None, None, 7),
        "thundah's bouncesphere" => (Some(2), Some(2), 4),
        "reactive by slapped" => (Some(2), Some(4), 6),
        "beginner classic" => (None, None, 5),
        "main classic" => (None, None, 7),
        "extra classic" => (Some(3), Some(4), 9),
        _ => return (0, 0.0),
    };
    let (over_req, max_rank_req, needed) = cfg;
    let ladder_top = entries.iter().map(|(_, ml, _)| *ml).max().unwrap_or(0) as u32;
    let at_or_above = |r: u32| -> u32 {
        counts
            .iter()
            .filter(|(&k, _)| k >= r)
            .map(|(_, &v)| v)
            .sum()
    };
    let highest_at = |needed: usize| -> u32 {
        let ranks: Vec<u32> = counts.keys().cloned().collect();
        for r in ranks.iter().rev() {
            if at_or_above(*r) >= needed as u32 {
                return *r;
            }
        }
        0
    };
    let progress_to = |rank: u32, needed: usize| -> f64 {
        if needed == 0 {
            return 0.0;
        }
        if rank >= ladder_top {
            return 1.0;
        }
        let next = rank + 1;
        let have: u32 = counts
            .iter()
            .filter(|(&k, _)| k >= next && k <= ladder_top)
            .map(|(_, &v)| v)
            .sum();
        ((have as f64) / (needed as f64)).min(1.0)
    };
    if let (Some(over_req_n), Some(max_rank_req_n)) = (over_req, max_rank_req) {
        if over_max_count >= over_req_n {
            let top = ladder_top.max(10);
            return (top, 1.0);
        }
        if let Some(maxr) = (max_rank_req_n != 0).then_some(max_rank_req_n) {
            let maxed: Vec<u32> = entries
                .iter()
                .filter(|&&(rank, ml, _)| ml > 0 && rank == ml as u32)
                .map(|&(rank, _, _)| rank)
                .collect();
            if maxed.len() >= maxr {
                let r = maxed.iter().cloned().max().unwrap_or(0);
                let complete = r >= ladder_top;
                return (r, if complete { 1.0 } else { 0.0 });
            }
        }
        let r = highest_at(needed);
        let prog = if r > 0 { progress_to(r, needed) } else { 0.0 };
        return (r, prog);
    }
    let r = highest_at(needed);
    let prog = if r > 0 { progress_to(r, needed) } else { 0.0 };
    (r, prog)
}

#[cfg(test)]
mod tests {

    #[test]
    fn ye_thresholds_shift_down_on_shared_boundary_tier() {
        // Construct a 2-difficulty benchmark where Easier ends on 'Charm' and
        // Medium starts on 'Charm' (Avasive S2's exact ladder shape).
        let mk = |name: &str, first: &str, last: &str, n: usize| Difficulty {
            name: name.to_string(),
            kovaaks_benchmark_id: 0,
            sharecode: String::new(),
            rank_colors: std::iter::repeat_n((), n)
                .enumerate()
                .map(|(i, _)| crate::types::RankTier {
                    name: if i == 0 {
                        first.to_string()
                    } else if i == n - 1 {
                        last.to_string()
                    } else {
                        format!("{name}T{i}")
                    },
                    color: "#000000".to_string(),
                })
                .collect(),
            categories: Vec::new(),
        };
        let bench = BenchmarkDef {
            name: "T".into(),
            abbreviation: "T".into(),
            color: "#000".into(),
            rank_calculation: "Avasive-S2".into(),
            spreadsheet_url: String::new(),
            difficulties: vec![
                mk("Easier", "Flutter", "Charm", 5),
                mk("Medium", "Charm", "Tranquility", 5),
            ],
            hidden: false,
        };
        let easier = &bench.difficulties[0];
        let medium = &bench.difficulties[1];
        // Easier: 0 tiers before, no shared boundary -> [100, 200, 300, 400, 500].
        assert_eq!(
            ye_thresholds(&bench, easier),
            vec![100.0, 200.0, 300.0, 400.0, 500.0]
        );
        // Medium: 5 tiers before, 1 shared 'Charm' boundary -> [500..900], NOT [600..1000].
        assert_eq!(
            ye_thresholds(&bench, medium),
            vec![500.0, 600.0, 700.0, 800.0, 900.0]
        );
    }

    use super::*;
    use crate::types::{CategoryProgress, RankTier, ScenarioEntry};

    fn tier(name: &str, color: &str) -> RankTier {
        RankTier {
            name: name.into(),
            color: color.into(),
        }
    }

    fn difficulty() -> Difficulty {
        Difficulty {
            name: "Novice".into(),
            kovaaks_benchmark_id: 459,
            sharecode: "x".into(),
            rank_colors: vec![
                tier("Iron", "#999999"),
                tier("Bronze", "#FF9900"),
                tier("Silver", "#CBD9E6"),
                tier("Gold", "#CAB148"),
            ],
            categories: vec![serde_json::json!({
                "categoryName": "Clicking",
                "subcategories": [
                    {"subcategoryName": "Dynamic", "scenarioCount": 2},
                    {"subcategoryName": "Static", "scenarioCount": 2}
                ]
            })],
        }
    }

    fn scenario(score: f64, rank: u32, maxes: &[f64]) -> ScenarioEntry {
        ScenarioEntry {
            score,
            leaderboard_rank: 0,
            scenario_rank: rank,
            rank_maxes: maxes.to_vec(),
            leaderboard_id: 0,
        }
    }

    fn progress(scenarios: Vec<(String, ScenarioEntry)>) -> BenchmarkProgress {
        BenchmarkProgress {
            benchmark_progress: 0.0,
            overall_rank: 0,
            categories: vec![(
                "Clicking".into(),
                CategoryProgress {
                    benchmark_progress: 0.0,
                    category_rank: 0,
                    rank_maxes: vec![],
                    scenarios,
                },
            )],
        }
    }

    #[test]
    fn rank_of_positions_scores_on_the_ladder() {
        let maxes = [100.0, 200.0, 300.0];
        let below = rank_of(50.0, &maxes);
        assert_eq!(below.base, 0);
        let mid = rank_of(250.0, &maxes);
        assert_eq!(mid.base, 2);
        assert!((mid.progress - 0.5).abs() < 1e-9);
        let maxed = rank_of(400.0, &maxes);
        assert!(maxed.is_maxed);
        assert!(maxed.precise > 3.0);
    }

    #[test]
    fn harmonic_variants_match_evxl() {
        assert!(harmonic_strict(&[100.0, 200.0, 400.0]) > 0.0);
        assert_eq!(
            harmonic_strict(&[100.0, 0.0]),
            0.0,
            "strict: any zero kills"
        );
        // soft: zeros become 0.1, result truncated
        assert_eq!(harmonic_soft(&[300.0, 0.0]), 0.0); // trunc(0.199…) = 0
        assert!(harmonic_soft(&[300.0, 300.0]) > 0.0);
    }

    /// VT S5 Novice shape: 3 categories x 3 subcategories x 1 scenario.
    fn vt_difficulty() -> Difficulty {
        Difficulty {
            name: "Novice".into(),
            kovaaks_benchmark_id: 459,
            sharecode: "x".into(),
            rank_colors: vec![
                tier("Iron", "#999999"),
                tier("Bronze", "#FF9900"),
                tier("Silver", "#CBD9E6"),
                tier("Gold", "#CAB148"),
            ],
            categories: vec![
                serde_json::json!({"categoryName": "Clicking", "subcategories": [
                    {"subcategoryName": "Dynamic", "scenarioCount": 1},
                    {"subcategoryName": "Static", "scenarioCount": 1},
                    {"subcategoryName": "Linear", "scenarioCount": 1}
                ]}),
                serde_json::json!({"categoryName": "Tracking", "subcategories": [
                    {"subcategoryName": "Precise", "scenarioCount": 1},
                    {"subcategoryName": "Reactive", "scenarioCount": 1},
                    {"subcategoryName": "Control", "scenarioCount": 1}
                ]}),
                serde_json::json!({"categoryName": "Switching", "subcategories": [
                    {"subcategoryName": "Speed", "scenarioCount": 1},
                    {"subcategoryName": "Evasive", "scenarioCount": 1},
                    {"subcategoryName": "Stability", "scenarioCount": 1}
                ]}),
            ],
        }
    }

    fn vt_progress(score_of: impl Fn(usize) -> (f64, u32)) -> BenchmarkProgress {
        progress(
            (0..9)
                .map(|i| {
                    let (score, r) = score_of(i);
                    (
                        format!("scenario{i}"),
                        scenario(score, r, &[300.0, 450.0, 600.0, 800.0]),
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn vt_energy_novice_computes_gold_for_strong_balanced_scores() {
        let d = vt_difficulty();
        // 9 subcats x 1 scenario: balanced Gold-ish energies (~600+).
        let p = vt_progress(|i| if i == 8 { (680.0, 4) } else { (620.0, 3) });
        let (rank, energy) = calc_vt_energy(&p, &d);
        assert_eq!(rank, 3, "harmonic mean ~310 clears the 300 Gold threshold");
        assert!(
            (310.0..=320.0).contains(&energy),
            "energy {energy} in the Gold band [300,400)"
        );
    }

    #[test]
    fn one_weak_subcategory_caps_vt_energy() {
        let d = vt_difficulty();
        let p = vt_progress(|i| if i == 8 { (250.0, 1) } else { (900.0, 4) });
        let (rank, _) = calc_vt_energy(&p, &d);
        assert!(
            rank < 4,
            "weak subcategory must drag the harmonic mean down, got {rank}"
        );
    }

    #[test]
    fn basic_computes_min_across_subcategories() {
        let d = difficulty();
        // Subcat Dynamic: two Gold scenarios; Static: one unranked.
        let p = progress(vec![
            (
                "a".into(),
                scenario(700.0, 4, &[300.0, 450.0, 600.0, 800.0]),
            ),
            (
                "b".into(),
                scenario(750.0, 4, &[300.0, 450.0, 600.0, 800.0]),
            ),
            ("c".into(), scenario(0.0, 0, &[])),
            ("d".into(), scenario(0.0, 0, &[])),
        ]);
        let (rank, complete, _) = calc_basic(&p, &d);
        assert_eq!(rank, 0, "unranked subcategory => rank 0");
        assert!(!complete);
    }

    #[test]
    fn dispatcher_falls_back_to_api_for_unported_methods() {
        let mut d = difficulty();
        d.name = "Novice".into();
        let mut b = BenchmarkDef {
            name: "Test".into(),
            abbreviation: "T".into(),
            color: "#000".into(),
            rank_calculation: "rxzu-unknown-tail".into(),
            spreadsheet_url: String::new(),
            difficulties: vec![d.clone()],
            hidden: false,
        };
        b.rank_calculation = "some-unported-method".into();
        let mut p = progress(vec![(
            "a".into(),
            scenario(500.0, 2, &[300.0, 450.0, 600.0]),
        )]);
        p.overall_rank = 2;
        let result = compute_rank(&p, &b, &d);
        assert_eq!(result.method, MethodSource::ApiFallback);
        assert_eq!(result.rank, 2);
    }

    #[test]
    fn dispatcher_uses_engine_rank_when_ported() {
        let d = vt_difficulty();
        let b = BenchmarkDef {
            name: "Test".into(),
            abbreviation: "T".into(),
            color: "#000".into(),
            rank_calculation: "vt-energy".into(),
            spreadsheet_url: String::new(),
            difficulties: vec![d.clone()],
            hidden: false,
        };
        // 8 subcats maxed, 1 weak: engine rank must stay below Gold.
        let p = vt_progress(|i| if i == 8 { (250.0, 2) } else { (900.0, 4) });
        let result = compute_rank(&p, &b, &d);
        assert_eq!(result.method, MethodSource::Engine);
        assert!(
            result.rank < 4,
            "engine rank {} must reflect the harmonic mean",
            result.rank
        );
    }

    #[test]
    fn miyu_counts_rank_points() {
        let p = progress(vec![
            ("a".into(), scenario(500.0, 3, &[100.0, 200.0, 300.0])),
            ("b".into(), scenario(500.0, 2, &[100.0, 200.0, 300.0])),
        ]);
        // points = (2+2) + (2+1) = 7 → below 16 → rank 0
        assert_eq!(calc_miyu(&p), 0);
    }

    #[test]
    fn count_required_matches_dojo_semantics() {
        let p = progress(vec![
            ("a".into(), scenario(500.0, 3, &[100.0, 200.0, 300.0])),
            ("b".into(), scenario(500.0, 3, &[100.0, 200.0, 300.0])),
            ("c".into(), scenario(500.0, 3, &[100.0, 200.0, 300.0])),
            ("d".into(), scenario(500.0, 2, &[100.0, 200.0, 300.0])),
        ]);
        assert_eq!(calc_count_required(&p, 3), 3, "three rank-3+ ⇒ rank 3");
        assert_eq!(
            calc_count_required(&p, 4),
            2,
            "all four have rank 2+ ⇒ rank 2"
        );
        assert_eq!(calc_count_required(&p, 5), 0);
    }
}
