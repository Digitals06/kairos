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

/// `Avasive-S2` (evxl `Nn` → `se` with an energy cap): per-difficulty
/// thresholds (`ye`), per-scenario energy clamped to a difficulty cap
/// (easier 600 / medium 1000 / hard 1400), subcategory energy = average of
/// the top TWO scenario energies (a single-scenario subcategory contributes
/// HALF its energy), harmonic mean across subcategories.
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
    let order = scenario_order(progress, difficulty);
    let subs = subcategory_spans(difficulty);
    let mut sub_energies: Vec<f64> = Vec::new();
    let mut idx = 0usize;
    for (_, count, _name) in &subs {
        let mut energies: Vec<f64> = Vec::new();
        for _ in 0..*count {
            if idx < order.len() {
                let (_, entry) = order[idx];
                let score = entry.score;
                if score > 0.0 && !entry.rank_maxes.is_empty() {
                    let pr = rank_of(score, &entry.rank_maxes);
                    // Uncapped energy interpolation over the threshold table
                    // (evxl W with fakeUpper = +Infinity), then the cap.
                    let mut e = energy_of(&pr, &thresholds, 100, f64::INFINITY);
                    if e.is_finite() {
                        e = e.min(cap);
                    } else {
                        e = cap;
                    }
                    energies.push(e);
                } else {
                    energies.push(0.0);
                }
            }
            idx += 1;
        }
        energies.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let sub = if energies.len() == 1 {
            energies[0] / 2.0
        } else if energies.len() >= 2 {
            (energies[0] + energies[1]) / 2.0
        } else {
            0.0
        };
        sub_energies.push(sub);
    }
    let overall = harmonic_strict(&sub_energies);
    let mut rank = 0u32;
    for (i, t) in thresholds.iter().enumerate() {
        if overall >= *t {
            rank = (i + 1) as u32;
        }
    }
    (rank, overall)
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
        rank_colors: std::iter::repeat(())
            .take(n)
            .enumerate()
            .map(|(i, _)| crate::types::RankTier {
                name: if i == 0 { first.to_string() } else if i == n - 1 { last.to_string() } else { format!("{name}T{i}") },
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
        difficulties: vec![mk("Easier", "Flutter", "Charm", 5), mk("Medium", "Charm", "Tranquility", 5)],
        hidden: false,
    };
    let easier = &bench.difficulties[0];
    let medium = &bench.difficulties[1];
    // Easier: 0 tiers before, no shared boundary -> [100, 200, 300, 400, 500].
    assert_eq!(ye_thresholds(&bench, easier), vec![100.0, 200.0, 300.0, 400.0, 500.0]);
    // Medium: 5 tiers before, 1 shared 'Charm' boundary -> [500..900], NOT [600..1000].
    assert_eq!(ye_thresholds(&bench, medium), vec![500.0, 600.0, 700.0, 800.0, 900.0]);
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
