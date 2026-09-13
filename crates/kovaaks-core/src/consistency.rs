//! Scenario consistency & plateau detection (`what to grind next` companion).
//!
//! Consistency = coefficient of variation (σ/μ) over the most recent plays of a
//! scenario's merged series — low CV on a played scenario means a stable PB the
//! player can replicate reliably. Plateau = days since the last personal best;
//! a scenario with samples but no new high for PLATEAU_DAYS days is flagged as
//! plateaued — time to grind elsewhere.

use chrono::{DateTime, Utc};

/// Days without a new personal best before a scenario counts as plateaued.
pub const PLATEAU_DAYS: i64 = 3;
/// Recent window for consistency (plays considered).
pub const RECENT_WINDOW: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioConsistency {
    /// Number of series samples considered.
    pub samples: usize,
    /// Last (best-assuming series is chronological) score.
    pub last: f64,
    /// Personal best across the series.
    pub best: f64,
    /// best − last (0 when the last score is the best).
    pub gap_to_best: f64,
    /// Coefficient of variation (σ/μ) over the recent window; 0.0 when fewer
    /// than 2 non-zero samples or mean 0.
    pub cv: f64,
    /// Days since the last new personal best (999.0 when a single sample).
    pub plateau_days: f64,
    /// `plateau_days >= PLATEAU_DAYS` with enough history (≥ 4 samples).
    pub plateaued: bool,
}

/// Compute consistency from a chronological (oldest → newest) merged series.
pub fn scenario_consistency(series: &[(DateTime<Utc>, f64)]) -> ScenarioConsistency {
    let samples = series.len();
    let last = series.last().map(|(_, s)| *s).unwrap_or(0.0);
    let best = series.iter().map(|(_, s)| *s).fold(0.0_f64, f64::max);

    // Running-PB timeline: track when each new high was set.
    let mut pb_at: Option<DateTime<Utc>> = None;
    let mut running = 0.0_f64;
    for (ts, score) in series {
        if *score > running {
            running = *score;
            pb_at = Some(*ts);
        }
    }
    let plateau_days = match pb_at {
        Some(ts) => {
            let last_ts = series.last().map(|(ts, _)| *ts);
            match last_ts {
                Some(last_ts) => (last_ts - ts).num_hours() as f64 / 24.0,
                None => 0.0,
            }
        }
        None => 999.0,
    };

    // CV over the recent non-zero scores.
    let recent: Vec<f64> = series
        .iter()
        .rev()
        .take(RECENT_WINDOW)
        .map(|(_, s)| *s)
        .filter(|s| *s > 0.0)
        .collect();
    let (cv, plateaued) = if recent.len() < 2 {
        (0.0, false)
    } else {
        let mean = recent.iter().sum::<f64>() / recent.len() as f64;
        let cv = if mean > 0.0 {
            let var =
                recent.iter().map(|s| (s - mean) * (s - mean)).sum::<f64>() / recent.len() as f64;
            (var.sqrt() / mean).min(1.0)
        } else {
            0.0
        };
        (cv, plateau_days >= PLATEAU_DAYS as f64 && recent.len() >= 4)
    };

    ScenarioConsistency {
        samples,
        last,
        best,
        gap_to_best: (best - last).max(0.0),
        cv,
        plateau_days,
        plateaued,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn series(scores: &[f64], day_step: i64) -> Vec<(DateTime<Utc>, f64)> {
        scores
            .iter()
            .enumerate()
            .map(|(i, s)| {
                (
                    chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
                        + chrono::Duration::days((i as i64) * day_step),
                    *s,
                )
            })
            .collect()
    }

    #[test]
    fn cv_is_stddev_over_mean() {
        // [10, 10] → σ 0 → CV 0.
        let c = scenario_consistency(&series(&[10.0, 10.0], 0));
        assert_eq!(c.cv, 0.0);
        // [10, 20] → mean 15 σ 5 → CV 1/3.
        let c = scenario_consistency(&series(&[10.0, 20.0], 0));
        assert!((c.cv - 1.0 / 3.0).abs() < 1e-9, "cv = {}", c.cv);
    }

    #[test]
    fn plateau_requires_gap_since_pb() {
        // Fresh PB → 0 days → not plateaued.
        let c = scenario_consistency(&series(&[10.0, 20.0, 30.0], 0));
        assert!(!c.plateaued);
        assert!(c.plateau_days < 1.0);
        // Best on day 0, series of 5 spread 5 days apart → 20 days since PB.
        let c = scenario_consistency(&series(&[50.0, 10.0, 11.0, 12.0, 13.0], 5));
        assert!(c.plateaued, "plateau_days {}", c.plateau_days);
        assert!(c.plateau_days >= 15.0);
    }

    #[test]
    fn single_sample_is_never_plateaued() {
        let c = scenario_consistency(&series(&[42.0], 0));
        assert_eq!(c.samples, 1);
        assert_eq!(c.cv, 0.0);
        assert!(!c.plateaued);
        assert_eq!(c.gap_to_best, 0.0);
    }

    #[test]
    fn gap_to_best_is_best_minus_last() {
        let c = scenario_consistency(&series(&[30.0, 25.0], 0));
        assert_eq!(c.gap_to_best, 5.0);
        let c = scenario_consistency(&series(&[25.0, 30.0], 0));
        assert_eq!(c.gap_to_best, 0.0);
    }
}
