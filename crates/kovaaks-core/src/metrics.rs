//! Improvement metrics over score series (plan Task 1.7).
//!
//! Pure computation, no I/O: one code path feeds both the dashboard stat
//! cards and the charts. [`compute`] scores the full-history view;
//! [`compute_window`] / [`compute_trailing_30d`] score the trailing-30-day
//! view.
//!
//! Semantics:
//! - Samples are bucketed by ISO week (`chrono` `%G-%V`, UTC) in
//!   chronological order.
//! - `avg_improvement_pct` compares the average of the *second half* of the
//!   weekly buckets (each bucket reduced to its own average first) against
//!   the average of the *first half*: `(second - first) / first * 100`.
//! - `high_improvement_pct` compares the current high score against the high
//!   of the chronologically first bucket.
//! - A single bucket with 2+ samples (e.g. a fresh player with one session)
//!   falls back to the same comparison at *sample* granularity: second-half
//!   sample mean vs first-half sample mean (avg), current high vs the first
//!   sample (high). One bucket and one sample — or a non-positive baseline —
//!   yields `None` — never `0.0` — so the UI can show "not enough data".

use chrono::{DateTime, Utc};

use crate::error::Result;
use crate::store::Store;

/// Trailing window (in days) used by [`compute_trailing_30d`].
pub const TRAILING_WINDOW_DAYS: i64 = 30;

/// Aggregated metrics for one score series.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Metrics {
    /// Mean of every score in the (windowed) series.
    pub avg_score: f64,
    /// Maximum score in the (windowed) series.
    pub high_score: f64,
    /// Second-half-of-buckets avg vs first-half-of-buckets avg, in percent.
    pub avg_improvement_pct: Option<f64>,
    /// Current high vs first-bucket high, in percent.
    pub high_improvement_pct: Option<f64>,
    /// Number of samples considered.
    pub samples: usize,
}

/// Full-history metrics over a score series.
pub fn compute(series: &[(DateTime<Utc>, f64)]) -> Metrics {
    compute_window(series, Utc::now(), chrono::Duration::weeks(5_200))
}

/// Metrics over the sub-series inside `[until - window, until]`.
pub fn compute_window(
    series: &[(DateTime<Utc>, f64)],
    until: DateTime<Utc>,
    window: chrono::Duration,
) -> Metrics {
    let since = until - window;
    let mut in_window: Vec<(DateTime<Utc>, f64)> = series
        .iter()
        .filter(|(ts, _)| *ts >= since && *ts <= until)
        .copied()
        .collect();
    if in_window.is_empty() {
        return Metrics::default();
    }
    in_window.sort_by_key(|(t, _)| *t);
    Metrics {
        avg_score: mean(in_window.iter().map(|(_, s)| *s)),
        high_score: in_window
            .iter()
            .map(|(_, s)| *s)
            .fold(f64::NEG_INFINITY, f64::max),
        avg_improvement_pct: avg_improvement(&in_window),
        high_improvement_pct: high_improvement(&in_window),
        samples: in_window.len(),
    }
}

/// Trailing-30-day window anchored at the latest sample of the series.
pub fn compute_trailing_30d(series: &[(DateTime<Utc>, f64)]) -> Metrics {
    match series.iter().map(|(ts, _)| *ts).max() {
        Some(latest) => {
            compute_window(series, latest, chrono::Duration::days(TRAILING_WINDOW_DAYS))
        }
        None => Metrics::default(),
    }
}

/// Metrics over one benchmark's snapshot history (benchmark_progress series).
pub fn metrics_for_benchmark(store: &Store, steam_id: &str, benchmark_id: i64) -> Result<Metrics> {
    let series: Vec<(DateTime<Utc>, f64)> = store
        .history(steam_id, benchmark_id)?
        .into_iter()
        .map(|snap| (snap.captured_at, snap.benchmark_progress as f64))
        .collect();
    Ok(compute(&series))
}

/// Metrics over one scenario's CSV play history (score series).
pub fn metrics_for_scenario_plays(
    store: &Store,
    steam_id: &str,
    scenario: &str,
) -> Result<Metrics> {
    let series: Vec<(DateTime<Utc>, f64)> = store
        .plays_history(steam_id, scenario)?
        .into_iter()
        .map(|rec| (rec.played_at, rec.score))
        .collect();
    Ok(compute(&series))
}

/// Metrics over one scenario's combined score history: CSV plays AND
/// per-snapshot scenario scores merged into one series (snapshot rows fill
/// the gaps where no local play was recorded — the displayed value must be
/// absolute, from whichever source observed it).
///
/// Snapshots that don't set a new high are skipped (see [`improving_only`]):
/// a stale repeat usually means the scenario wasn't replayed, and when it
/// was, the local CSV plays already carry the average.
pub fn metrics_for_scenario_combined(
    store: &Store,
    steam_id: &str,
    benchmark_id: i64,
    scenario: &str,
) -> Result<Metrics> {
    let plays: Vec<(String, DateTime<Utc>, f64)> = store
        .plays_history(steam_id, scenario)?
        .into_iter()
        .map(|rec| (rec.scenario.clone(), rec.played_at, rec.score))
        .collect();
    let snapshots: Vec<(DateTime<Utc>, f64)> = store
        .history(steam_id, benchmark_id)?
        .iter()
        .filter_map(|snap| {
            snap.scenarios
                .iter()
                .find(|r| r.scenario == scenario)
                .map(|row| (snap.captured_at, row.score as f64))
        })
        .collect();
    // New-high snapshots only; snapshot points duplicating a local play (the
    // same run echoed by the sync) are dropped — see merge_plays_snapshots_dedup.
    let series = merge_plays_snapshots_dedup(&plays, &improving_only(&snapshots));
    Ok(compute(&series))
}

/// Keep only the snapshots that set a new high for a scenario, in order.
///
/// A sync snapshot that doesn't improve the score almost always means the
/// scenario wasn't replayed since the previous sync — recording it would
/// drag averages toward stale data and spray flat dots across the chart.
/// When the scenario WAS replayed without a new high, the local CSV plays
/// already describe the average, so the snapshot adds nothing.
///
/// The running max seeds at `0.0`: an all-zero (never-played) series
/// contributes nothing. Input must be chronological (as `Store::history`
/// returns); output preserves order.
pub fn improving_only(series: &[(DateTime<Utc>, f64)]) -> Vec<(DateTime<Utc>, f64)> {
    let mut best = 0.0f64;
    let mut kept = Vec::new();
    for &(at, score) in series {
        if score > best && score.is_finite() {
            best = score;
            kept.push((at, score));
        }
    }
    kept
}

/// Merge local plays with (already new-high-filtered) snapshot points into one
/// chronological series, dropping snapshot points that duplicate a local play.
///
/// A snapshot whose score equals a local play's score (to 2dp) is the SAME
/// scenario run observed twice — KovaaK's scenario scores are precise floats,
/// so an exact match is the same run no matter how much later the sync ran.
/// Keeping both would add a phantom sample at the sync timestamp and truncate
/// the real average improvement, so the snapshot point is skipped and the play
/// (exact time) is kept. Plays are always kept verbatim.
pub fn merge_plays_snapshots_dedup(
    plays: &[(String, DateTime<Utc>, f64)],
    snapshots: &[(DateTime<Utc>, f64)],
) -> Vec<(DateTime<Utc>, f64)> {
    let mut merged: Vec<(DateTime<Utc>, f64)> =
        plays.iter().map(|(_, at, score)| (*at, *score)).collect();
    for &(at, score) in snapshots {
        let dup = plays
            .iter()
            .any(|(_, _, pscore)| (*pscore - score).abs() < 0.01);
        if !dup {
            merged.push((at, score));
        }
    }
    merged.sort_by_key(|(t, _)| *t);
    merged
}

// ---------- scenario history (charts) ----------

/// Where a scenario's chart series came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioSeriesSource {
    /// Synced snapshot entries (score > 0, new-highs only).
    Snapshot,
    /// Local CSV plays — used when the scenario has no synced score.
    Local,
}

impl ScenarioSeriesSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ScenarioSeriesSource::Snapshot => "snapshot",
            ScenarioSeriesSource::Local => "local",
        }
    }
}

/// A scenario's chart series: which scenario, its category label, where the
/// points came from, and the (filtered) points themselves.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioSeries {
    pub scenario: String,
    pub category: String,
    pub source: ScenarioSeriesSource,
    pub points: Vec<(DateTime<Utc>, i64)>,
}

/// Build per-scenario chart series for one benchmark.
///
/// Scenario order follows the latest snapshot (API document order); scenarios
/// that appear only in local plays are appended after, alphabetically.
///
/// A scenario's points come from its synced snapshot scores (score > 0,
/// new-highs only — see [`improving_only`]). When a scenario has NO synced
/// score (score 0 in the latest snapshot, or absent from every snapshot) but
/// has local CSV plays, its series is built from those plays instead — the
/// chart still renders even though KovaaK's never reported a score for it.
pub fn build_scenario_history(
    history: &[crate::store::StoredSnapshot],
    local_plays: &[(String, DateTime<Utc>, f64)],
) -> Vec<ScenarioSeries> {
    let Some(latest) = history.last() else {
        return Vec::new();
    };

    // Scenarios seen anywhere in this benchmark's snapshot history but missing
    // from the latest snapshot (API sometimes omits scenarios it has no data
    // for). Category from the most recent snapshot that carried the scenario.
    let mut extras: Vec<(String, String)> = Vec::new(); // (scenario, category)
    for snap in history.iter().rev() {
        for row in &snap.scenarios {
            if latest.scenarios.iter().any(|r| r.scenario == row.scenario) {
                continue;
            }
            if extras.iter().any(|(s, _)| s == &row.scenario) {
                continue;
            }
            extras.push((row.scenario.clone(), row.category.clone()));
        }
    }

    let mut out = Vec::new();
    for row in &latest.scenarios {
        let pts = snapshot_points(history, &row.scenario);
        let (source, points) = if pts.is_empty() {
            local_points(local_plays, &row.scenario)
        } else {
            (ScenarioSeriesSource::Snapshot, pts)
        };
        if points.is_empty() {
            continue; // nothing synced AND nothing local: no chart
        }
        out.push(ScenarioSeries {
            scenario: row.scenario.clone(),
            category: row.category.clone(),
            source,
            points,
        });
    }
    for (scenario, category) in extras {
        let pts = snapshot_points(history, &scenario);
        let (source, points) = if pts.is_empty() {
            local_points(local_plays, &scenario)
        } else {
            (ScenarioSeriesSource::Snapshot, pts)
        };
        if points.is_empty() {
            continue;
        }
        out.push(ScenarioSeries {
            scenario,
            category,
            source,
            points,
        });
    }
    // Local-only scenarios (never in any snapshot, unattributable via rows):
    // include when the player has plays for them — appended alphabetically.
    let mut locals: Vec<&(String, DateTime<Utc>, f64)> = local_plays
        .iter()
        .filter(|(s, _, _)| !out.iter().any(|series| &series.scenario == s))
        .collect();
    locals.sort_by(|a, b| a.0.cmp(&b.0));
    let mut seen: Vec<&str> = Vec::new();
    for (scenario, _at, _score) in locals {
        if seen.contains(&scenario.as_str()) {
            continue;
        }
        seen.push(scenario);
        let (_, points) = local_points(local_plays, scenario);
        if points.is_empty() {
            continue;
        }
        out.push(ScenarioSeries {
            scenario: scenario.clone(),
            category: "Local".to_string(),
            source: ScenarioSeriesSource::Local,
            points,
        });
    }
    out
}

/// New-high snapshot points with a positive score for one scenario.
fn snapshot_points(
    history: &[crate::store::StoredSnapshot],
    scenario: &str,
) -> Vec<(DateTime<Utc>, i64)> {
    let raw: Vec<(DateTime<Utc>, f64)> = history
        .iter()
        .filter_map(|s| {
            s.scenarios
                .iter()
                .find(|r| r.scenario == scenario)
                .filter(|r| r.score > 0)
                .map(|r| (s.captured_at, r.score as f64))
        })
        .collect();
    improving_only(&raw)
        .into_iter()
        .map(|(t, v)| (t, v as i64))
        .collect()
}

/// New-high local play points for one scenario.
fn local_points(
    local_plays: &[(String, DateTime<Utc>, f64)],
    scenario: &str,
) -> (ScenarioSeriesSource, Vec<(DateTime<Utc>, i64)>) {
    let raw: Vec<(DateTime<Utc>, f64)> = local_plays
        .iter()
        .filter(|(s, _, _)| s == scenario)
        .map(|(_, at, score)| (*at, *score))
        .collect();
    (
        ScenarioSeriesSource::Local,
        improving_only(&raw)
            .into_iter()
            .map(|(t, v)| (t, v as i64))
            .collect(),
    )
}

// ---------- internals ----------

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    let mut n = 0usize;
    for v in values {
        sum += v;
        n += 1;
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

/// ISO-week key (`%G-%V`) in chronological order, one entry per sample.
fn iso_week_keys(times: &[DateTime<Utc>]) -> Vec<String> {
    times
        .iter()
        .map(|t| t.format("%G-%V").to_string())
        .collect()
}

/// Group sample *indices* into consecutive equal-key runs (input must be
/// chronologically sorted) so bucket order is preserved without a HashMap.
fn group_runs(keys: &[String]) -> Vec<(String, Vec<usize>)> {
    let mut buckets: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        match buckets.last_mut() {
            Some((k, idxs)) if *k == *key => idxs.push(i),
            _ => buckets.push((key.clone(), vec![i])),
        }
    }
    buckets
}

/// `(avg of second-half bucket-averages - avg of first-half bucket-averages)
/// / first-half avg * 100`, or `None` with fewer than 2 samples or a
/// non-positive baseline. A single bucket falls back to the same split at
/// sample granularity so fresh (single-session) series still score.
fn avg_improvement(sorted: &[(DateTime<Utc>, f64)]) -> Option<f64> {
    let keys = iso_week_keys(&sorted.iter().map(|(t, _)| *t).collect::<Vec<_>>());
    let buckets = group_runs(&keys);
    let avgs: Vec<f64> = if buckets.len() >= 2 {
        buckets
            .iter()
            .map(|(_, idxs)| {
                let sum: f64 = idxs.iter().map(|&i| sorted[i].1).sum();
                sum / idxs.len() as f64
            })
            .collect()
    } else {
        // Single-bucket fallback: one average per sample, same halves split.
        if sorted.len() < 2 {
            return None;
        }
        sorted.iter().map(|(_, s)| *s).collect()
    };
    halves_pct(&avgs)
}

/// Split `values` chronologically in halves (mirroring the bucket split:
/// `mid = len / 2`) and return `(second - first) / first * 100`, or `None`
/// on a non-positive/non-finite baseline.
fn halves_pct(values: &[f64]) -> Option<f64> {
    let mid = values.len() / 2;
    if mid == 0 {
        return None;
    }
    let first: f64 = values[..mid].iter().sum::<f64>() / mid as f64;
    let second: f64 = values[mid..].iter().sum::<f64>() / (values.len() - mid) as f64;
    if first <= 0.0 || !first.is_finite() || !second.is_finite() {
        return None;
    }
    Some((second - first) / first * 100.0)
}

/// `(current high - high of first bucket) / first-bucket high * 100`, or
/// `None` with fewer than 2 samples or a non-positive baseline high. A
/// single bucket compares against the first *sample* instead.
fn high_improvement(sorted: &[(DateTime<Utc>, f64)]) -> Option<f64> {
    if sorted.len() < 2 {
        return None;
    }
    let keys = iso_week_keys(&sorted.iter().map(|(t, _)| *t).collect::<Vec<_>>());
    let buckets = group_runs(&keys);
    let first_high = if buckets.len() >= 2 {
        buckets[0]
            .1
            .iter()
            .map(|&i| sorted[i].1)
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        sorted[0].1
    };
    let current_high = sorted
        .iter()
        .map(|(_, s)| *s)
        .fold(f64::NEG_INFINITY, f64::max);
    if first_high <= 0.0 || !first_high.is_finite() {
        return None;
    }
    Some((current_high - first_high) / first_high * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    fn ts(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0)
            .single()
            .expect("valid timestamp")
    }

    /// 8 consecutive ISO weeks (Mondays 2026-01-05 … 2026-02-23): buckets 1-4
    /// score 100, buckets 5-7 score 200, bucket 8 has two samples (200, 300).
    fn eight_week_series() -> Vec<(DateTime<Utc>, f64)> {
        let base = ts(2026, 1, 5, 12);
        let mut series: Vec<(DateTime<Utc>, f64)> = (0..7)
            .map(|k| {
                (
                    base + chrono::Duration::days(7 * k),
                    if k < 4 { 100.0 } else { 200.0 },
                )
            })
            .collect();
        series.push((base + chrono::Duration::days(49), 200.0));
        series.push((
            base + chrono::Duration::days(49) + chrono::Duration::hours(1),
            300.0,
        ));
        series
    }

    /// avg [100, 200, 300] = 200, high = 300; one ISO week → the
    /// single-bucket fallback scores at sample granularity.
    #[test]
    fn single_week_falls_back_to_sample_granularity() {
        let series = vec![
            (ts(2026, 9, 1, 10), 100.0),
            (ts(2026, 9, 1, 11), 200.0),
            (ts(2026, 9, 1, 12), 300.0),
        ];
        let m = compute(&series);
        approx(m.avg_score, 200.0);
        approx(m.high_score, 300.0);
        assert_eq!(m.samples, 3);
        // Halves split [100] vs [200, 300] → (250-100)/100 = +150 %.
        approx(m.avg_improvement_pct.expect("fallback"), 150.0);
        // Current high 300 vs first sample 100 → +200 %.
        approx(m.high_improvement_pct.expect("fallback"), 200.0);

        // Input order must not matter (internal chronological sort).
        let shuffled = vec![
            (ts(2026, 9, 1, 12), 300.0),
            (ts(2026, 9, 1, 10), 100.0),
            (ts(2026, 9, 1, 11), 200.0),
        ];
        assert_eq!(compute(&shuffled), m);
    }

    #[test]
    fn eight_week_series_improvement_math() {
        let m = compute(&eight_week_series());
        assert_eq!(m.samples, 9);
        approx(m.avg_score, 1500.0 / 9.0);
        approx(m.high_score, 300.0);
        // First half (4 buckets) avg 100; second half bucket avgs
        // [200, 200, 200, 250] → avg 212.5 → +112.5 %.
        approx(m.avg_improvement_pct.expect("8 buckets"), 112.5);
        // Current high 300 vs first-bucket high 100 → +200 %.
        approx(m.high_improvement_pct.expect("8 buckets"), 200.0);
    }

    #[test]
    fn trailing_30d_window_only_counts_recent_samples() {
        // Anchor = 2026-02-23 13:00, window 30 d → since 2026-01-24 13:00 →
        // samples: Jan 26, Feb 2, 9, 16, 23 (×2) = 6 samples, avg 200.
        let m = compute_trailing_30d(&eight_week_series());
        assert_eq!(m.samples, 6);
        approx(m.avg_score, 200.0);
        approx(m.high_score, 300.0);
        assert!(m.avg_improvement_pct.is_some());
    }

    #[test]
    fn explicit_window_excludes_old_samples() {
        // 10 weekly points 100..=1000 starting Monday 2026-03-02; the 30-day
        // window anchored at the last sample covers exactly the last 5
        // (offsets 0/7/14/21/28 days) → avg 800.
        let base = ts(2026, 3, 2, 12);
        let series: Vec<(DateTime<Utc>, f64)> = (0..10)
            .map(|k| {
                (
                    base + chrono::Duration::days(7 * k),
                    100.0 * (k as f64 + 1.0),
                )
            })
            .collect();
        let m = compute_window(
            &series,
            base + chrono::Duration::days(63),
            chrono::Duration::days(30),
        );
        assert_eq!(m.samples, 5);
        approx(m.avg_score, 800.0);
    }

    #[test]
    fn empty_series_yields_defaults() {
        let m = compute(&[]);
        assert_eq!(m, Metrics::default());
        assert_eq!(m.samples, 0);
        assert_eq!(m.avg_score, 0.0);
        assert_eq!(m.high_score, 0.0);
        assert_eq!(m.avg_improvement_pct, None);
        assert_eq!(m.high_improvement_pct, None);
    }

    #[test]
    fn single_sample_has_no_improvement() {
        let m = compute(&[(ts(2026, 3, 2, 12), 500.0)]);
        assert_eq!(m.samples, 1);
        approx(m.avg_score, 500.0);
        approx(m.high_score, 500.0);
        assert_eq!(m.avg_improvement_pct, None);
        assert_eq!(m.high_improvement_pct, None);
        // The windowed path matches for a lone sample too.
        let w = compute_trailing_30d(&[(ts(2026, 3, 2, 12), 500.0)]);
        assert_eq!(w.avg_improvement_pct, None);
        assert_eq!(w.samples, 1);
    }

    #[test]
    fn zero_first_half_yields_none_never_zero_or_nan() {
        let base = ts(2026, 1, 5, 12);
        let series = vec![
            (base, 0.0),
            (base + chrono::Duration::days(7), 0.0),
            (base + chrono::Duration::days(14), 100.0),
            (base + chrono::Duration::days(21), 100.0),
        ];
        let m = compute(&series);
        assert_eq!(
            m.avg_improvement_pct, None,
            "divide-by-zero baseline must be None, never 0/NaN"
        );
        assert_eq!(
            m.high_improvement_pct, None,
            "first-bucket high of 0 must be None, never 0/NaN"
        );
    }

    /// REGRESSION (stat cards showed 0 while the chart had snapshot scores):
    /// per-scenario metrics must merge CSV plays AND snapshot scenario rows —
    /// the absolute value comes from whichever source observed it.
    #[test]
    fn combined_metrics_merge_plays_and_snapshots() {
        let path = std::env::temp_dir().join(format!(
            "kovaaks-comb-metrics-{}-{}.db",
            std::process::id(),
            ts(2026, 1, 1, 0).timestamp_micros()
        ));
        let store = Store::open(&path).expect("temp store");

        // One CSV play (score 100) and one snapshot observation (score 300).
        let play = crate::types::PlayRecord {
            scenario: "VT 1w4ts Novice S5".to_string(),
            played_at: ts(2026, 1, 5, 12),
            score: 100.0,
            hit_count: 50,
            avg_fps: 240.0,
            source: "csv".to_string(),
        };
        store
            .record_play("76561190000000001", &play, "C:/fake/play-1.csv")
            .expect("record play");

        let mut categories = vec![(
            "Clicking".to_string(),
            crate::types::CategoryProgress {
                benchmark_progress: 30000.0,
                category_rank: 3,
                rank_maxes: vec![10000.0, 20000.0, 30000.0, 40000.0],
                scenarios: vec![(
                    "VT 1w4ts Novice S5".to_string(),
                    crate::types::ScenarioEntry {
                        score: 300.0,
                        leaderboard_rank: 42,
                        scenario_rank: 3,
                        rank_maxes: vec![250.0, 275.0, 290.0, 310.0],
                        leaderboard_id: 98059,
                    },
                )],
            },
        )];
        let progress = crate::types::BenchmarkProgress {
            benchmark_progress: 30000.0,
            overall_rank: 3,
            categories,
        };
        store
            .record_snapshot("76561190000000001", 459, &progress, ts(2026, 2, 5, 12))
            .expect("record snapshot");

        let m =
            metrics_for_scenario_combined(&store, "76561190000000001", 459, "VT 1w4ts Novice S5")
                .expect("combined metrics");

        assert_eq!(m.samples, 2, "play + snapshot must both count");
        approx(m.avg_score, 200.0);
        approx(m.high_score, 300.0);
        assert!(m.avg_improvement_pct.is_some(), "2 buckets -> Some");

        // Sanity: plays-only metrics ignore the snapshot (the old behavior).
        let plays_only =
            metrics_for_scenario_plays(&store, "76561190000000001", "VT 1w4ts Novice S5").unwrap();
        assert_eq!(plays_only.samples, 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn improving_only_keeps_strict_highs_in_order() {
        let day = |d: u32| ts(2026, 1, d, 12);
        let series = vec![
            (day(1), 0.0),   // unplayed: never a data point
            (day(2), 500.0), // baseline high: kept
            (day(3), 500.0), // stale repeat: dropped
            (day(4), 400.0), // below high: dropped
            (day(5), 450.0), // partial recovery, still stale: dropped
            (day(6), 600.0), // new high: kept
            (day(7), 600.0), // stale repeat: dropped
        ];
        let kept = improving_only(&series);
        assert_eq!(
            kept,
            vec![(day(2), 500.0), (day(6), 600.0)],
            "only new highs survive, in order"
        );
        assert!(improving_only(&[]).is_empty());
        assert_eq!(
            improving_only(&[(day(1), 300.0)]),
            vec![(day(1), 300.0)],
            "a lone baseline is informative"
        );
        assert!(
            improving_only(&[(day(1), 0.0), (day(2), 0.0)]).is_empty(),
            "all-zero series contributes nothing"
        );
    }

    /// REGRESSION (flat cyan dots + stale averages): a snapshot that repeats
    /// the previous high must not count as a new sample in combined metrics.
    #[test]
    fn combined_metrics_skip_stale_snapshot_repeats() {
        let path = std::env::temp_dir().join(format!(
            "kovaaks-comb-stale-{}-{}.db",
            std::process::id(),
            ts(2026, 3, 1, 0).timestamp_micros()
        ));
        let store = Store::open(&path).expect("temp store");

        let scenario = "VT 1w4ts Novice S5".to_string();
        let entry = |score: f64| crate::types::ScenarioEntry {
            score,
            leaderboard_rank: 42,
            scenario_rank: 3,
            rank_maxes: vec![250.0, 275.0, 290.0, 310.0],
            leaderboard_id: 98059,
        };
        let snapshot_at = |score: f64, day: u32| {
            let progress = crate::types::BenchmarkProgress {
                benchmark_progress: 30000.0,
                overall_rank: 3,
                categories: vec![(
                    "Clicking".to_string(),
                    crate::types::CategoryProgress {
                        benchmark_progress: 30000.0,
                        category_rank: 3,
                        rank_maxes: vec![10000.0, 20000.0, 30000.0, 40000.0],
                        scenarios: vec![(scenario.clone(), entry(score))],
                    },
                )],
            };
            store
                .record_snapshot("76561190000000001", 459, &progress, ts(2026, 3, day, 12))
                .expect("record snapshot")
        };
        // Baseline high, stale repeat, dip, new high.
        snapshot_at(300.0, 5);
        snapshot_at(300.0, 6);
        snapshot_at(250.0, 7);
        snapshot_at(350.0, 8);

        let m =
            metrics_for_scenario_combined(&store, "76561190000000001", 459, "VT 1w4ts Novice S5")
                .expect("combined metrics");
        assert_eq!(m.samples, 2, "only the 300 baseline + 350 high count");
        approx(m.avg_score, 325.0);
        approx(m.high_score, 350.0);

        let _ = std::fs::remove_file(&path);
    }
}
