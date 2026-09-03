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
//! - Fewer than 2 buckets (empty series, single sample, one week of data)
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
pub fn metrics_for_scenario_combined(
    store: &Store,
    steam_id: &str,
    benchmark_id: i64,
    scenario: &str,
) -> Result<Metrics> {
    let mut series: Vec<(DateTime<Utc>, f64)> = store
        .plays_history(steam_id, scenario)?
        .into_iter()
        .map(|rec| (rec.played_at, rec.score))
        .collect();
    for snap in store.history(steam_id, benchmark_id)? {
        if let Some(row) = snap.scenarios.iter().find(|r| r.scenario == scenario) {
            if row.score > 0 {
                series.push((snap.captured_at, row.score as f64));
            }
        }
    }
    series.sort_by_key(|(t, _)| *t);
    Ok(compute(&series))
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
/// / first-half avg * 100`, or `None` with fewer than 2 buckets or a
/// non-positive baseline.
fn avg_improvement(sorted: &[(DateTime<Utc>, f64)]) -> Option<f64> {
    let keys = iso_week_keys(&sorted.iter().map(|(t, _)| *t).collect::<Vec<_>>());
    let buckets = group_runs(&keys);
    if buckets.len() < 2 {
        return None;
    }
    let bucket_avgs: Vec<f64> = buckets
        .iter()
        .map(|(_, idxs)| {
            let sum: f64 = idxs.iter().map(|&i| sorted[i].1).sum();
            sum / idxs.len() as f64
        })
        .collect();
    let mid = bucket_avgs.len() / 2;
    let first: f64 = bucket_avgs[..mid].iter().sum::<f64>() / mid as f64;
    let second: f64 = bucket_avgs[mid..].iter().sum::<f64>() / (bucket_avgs.len() - mid) as f64;
    if first <= 0.0 || !first.is_finite() {
        return None;
    }
    Some((second - first) / first * 100.0)
}

/// `(current high - high of first bucket) / first-bucket high * 100`, or
/// `None` with fewer than 2 buckets or a non-positive baseline high.
fn high_improvement(sorted: &[(DateTime<Utc>, f64)]) -> Option<f64> {
    let keys = iso_week_keys(&sorted.iter().map(|(t, _)| *t).collect::<Vec<_>>());
    let buckets = group_runs(&keys);
    if buckets.len() < 2 {
        return None;
    }
    let first_bucket = &buckets[0].1;
    let first_high = first_bucket
        .iter()
        .map(|&i| sorted[i].1)
        .fold(f64::NEG_INFINITY, f64::max);
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

    /// avg [100, 200, 300] = 200, high = 300; one ISO week → improvements None.
    #[test]
    fn basic_stats_and_single_week_have_no_improvement() {
        let series = vec![
            (ts(2026, 9, 1, 10), 100.0),
            (ts(2026, 9, 1, 11), 200.0),
            (ts(2026, 9, 1, 12), 300.0),
        ];
        let m = compute(&series);
        approx(m.avg_score, 200.0);
        approx(m.high_score, 300.0);
        assert_eq!(m.samples, 3);
        assert_eq!(m.avg_improvement_pct, None);
        assert_eq!(m.high_improvement_pct, None);

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
}
