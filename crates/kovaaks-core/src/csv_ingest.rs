//! Forward-only CSV ingest of KovaaK's local stats files (plan Task 1.9).
//!
//! KovaaK's writes one CSV per play into its `stats/` directory, named
//! `<scenario> - Challenge - <YYYY.MM.DD-HH.MM.SS> Stats.csv`, with kill rows
//! up top and a metadata footer (`Score:,959.120239`, `Hit Count:,101`,
//! `Avg FPS:,420.766052`). This module parses those files and ingests them
//! into the [`Store`] **forward-only**: a meta-keyed cutoff
//! (`first_run_csv_cutoff`, set to "now" on first app run) guarantees the
//! pre-existing backlog (1,031+ files on this machine) is never backfilled.

use std::path::Path;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};

use crate::error::{Error, Result};
use crate::store::Store;
use crate::types::PlayRecord;

/// Meta key holding the forward-only cutoff (RFC3339 UTC).
pub const FIRST_RUN_CUTOFF_KEY: &str = "first_run_csv_cutoff";

/// Suffix every KovaaK's stats file ends with.
const STATS_SUFFIX: &str = " Stats.csv";

/// Separator between the scenario name and the timestamp in stats filenames.
/// Split from the RIGHT so scenario names containing ` - ` survive.
const CHALLENGE_SEP: &str = " - Challenge - ";

/// Filename timestamp format (`2026.07.27-15.52.38`).
const FILENAME_TS_FORMAT: &str = "%Y.%m.%d-%H.%M.%S";

/// Zero-sized ingest facade (`CsvIngest::new().scan(...)`).
#[derive(Debug, Clone, Copy, Default)]
pub struct CsvIngest;

impl CsvIngest {
    pub fn new() -> Self {
        CsvIngest
    }

    /// Parse one stats CSV (see [`parse_stats_csv`]).
    pub fn parse(&self, path: &Path) -> Result<ParsedPlay> {
        parse_stats_csv(path)
    }

    /// Scan a stats directory forward-only (see [`scan_dir`]).
    pub fn scan(
        &self,
        dir: &Path,
        cutoff: DateTime<Utc>,
        store: &Store,
        steam_id: &str,
    ) -> IngestReport {
        scan_dir(dir, cutoff, store, steam_id)
    }

    /// First-run cutoff guard (see [`ensure_cutoff`]).
    pub fn ensure_cutoff(&self, store: &Store) -> Result<DateTime<Utc>> {
        ensure_cutoff(store)
    }
}

/// One parsed stats CSV (everything `record_play` needs).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPlay {
    /// Scenario name from the filename (may itself contain ` - `).
    pub scenario: String,
    /// Play timestamp from the filename, converted local → UTC.
    pub played_at: DateTime<Utc>,
    /// Footer `Score:` value.
    pub score: f64,
    /// Footer `Hit Count:` value.
    pub hit_count: i64,
    /// Footer `Avg FPS:` value.
    pub avg_fps: f64,
}

/// Summary of one [`scan_dir`] pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IngestReport {
    /// `.csv` files found in the directory.
    pub seen: usize,
    /// New rows inserted (idempotent re-scans insert 0).
    pub inserted: usize,
    /// Files whose parsed timestamp is older than the cutoff (forward-only
    /// skips).
    pub skipped_old: usize,
    /// Per-file failure descriptions (never fatal for the scan).
    pub errors: Vec<String>,
}

/// Parse a KovaaK's stats CSV: scenario + timestamp from the filename,
/// score/hit-count/fps from the metadata footer.
pub fn parse_stats_csv(path: &Path) -> Result<ParsedPlay> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Csv(format!("non-utf8 filename: {}", path.display())))?;
    let parsed_name = parse_filename(file_name)
        .ok_or_else(|| Error::Csv(format!("not a KovaaK's stats filename: {file_name}")))?;

    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Csv(format!("cannot read {}: {e}", path.display())))?;

    let mut score: Option<f64> = None;
    let mut hit_count: Option<i64> = None;
    let mut avg_fps: Option<f64> = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim();
        if let Some(rest) = line.strip_prefix("Score:") {
            if score.is_none() {
                score = Some(parse_footer_value::<f64>(rest)?);
            }
        } else if let Some(rest) = line.strip_prefix("Hit Count:") {
            if hit_count.is_none() {
                hit_count = Some(parse_footer_value::<i64>(rest)?);
            }
        } else if let Some(rest) = line.strip_prefix("Avg FPS:") {
            if avg_fps.is_none() {
                avg_fps = Some(parse_footer_value::<f64>(rest)?);
            }
        }
    }

    let score = score.ok_or_else(|| Error::Csv(format!("no 'Score:' footer in {file_name}")))?;
    let hit_count =
        hit_count.ok_or_else(|| Error::Csv(format!("no 'Hit Count:' footer in {file_name}")))?;
    let avg_fps =
        avg_fps.ok_or_else(|| Error::Csv(format!("no 'Avg FPS:' footer in {file_name}")))?;

    Ok(ParsedPlay {
        scenario: parsed_name.scenario,
        played_at: parsed_name.played_at,
        score,
        hit_count,
        avg_fps,
    })
}

/// Footer lines look like `Score:,959.120239` — the value sits after the
/// first comma. `Score:,3064.0` and `Hit Count:,101` both parse; a
/// comma-less `Score: 3064.0` variant is tolerated too.
fn parse_footer_value<T: std::str::FromStr>(rest: &str) -> Result<T> {
    let trimmed = rest.trim();
    let value = match trimmed.split_once(',') {
        Some((_, after)) => after.trim(),
        None => trimmed,
    };
    value.parse::<T>().map_err(|_| {
        Error::Csv(format!(
            "unparseable footer value (expected number): {value:?}"
        ))
    })
}

/// Scan `dir` for `*.csv` stats files and ingest each newer than `cutoff`
/// into the store (idempotent per file path). Broken files are collected in
/// the report's `errors` and never abort the scan.
pub fn scan_dir(dir: &Path, cutoff: DateTime<Utc>, store: &Store, steam_id: &str) -> IngestReport {
    let mut report = IngestReport::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            report
                .errors
                .push(format!("cannot read dir {}: {e}", dir.display()));
            return report;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                report
                    .errors
                    .push(format!("dir entry error in {}: {e}", dir.display()));
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("csv") {
            continue;
        }
        report.seen += 1;
        let parsed = match parse_stats_csv(&path) {
            Ok(p) => p,
            Err(e) => {
                report.errors.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        if parsed.played_at < cutoff {
            report.skipped_old += 1;
            continue;
        }
        let csv_path = path.to_string_lossy().into_owned();
        match store.record_play(
            steam_id,
            &PlayRecord {
                scenario: parsed.scenario,
                played_at: parsed.played_at,
                score: parsed.score,
                hit_count: parsed.hit_count.max(0) as u64,
                avg_fps: parsed.avg_fps,
                source: "csv".to_string(),
            },
            &csv_path,
        ) {
            Ok(true) => report.inserted += 1,
            Ok(false) => {}
            Err(e) => report.errors.push(format!("{}: {e}", path.display())),
        }
    }
    report
}

/// Forward-only cutoff guard: on first call (key absent) store `Utc::now()`
/// under [`FIRST_RUN_CUTOFF_KEY`] and return it; every later call returns the
/// stored value — so files older than the first run are never backfilled.
pub fn ensure_cutoff(store: &Store) -> Result<DateTime<Utc>> {
    if let Some(stored) = store.get_meta(FIRST_RUN_CUTOFF_KEY)? {
        return DateTime::parse_from_rfc3339(&stored)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| Error::Csv(format!("corrupt {FIRST_RUN_CUTOFF_KEY}: {e}")));
    }
    let now = Utc::now();
    store.set_meta(FIRST_RUN_CUTOFF_KEY, &now.to_rfc3339())?;
    Ok(now)
}

/// Filename → (scenario, UTC timestamp). The scenario is everything left of
/// the rightmost ` - Challenge - `; the timestamp is parsed as local time.
fn parse_filename(file_name: &str) -> Option<ParsedFilename> {
    let stem = file_name.strip_suffix(STATS_SUFFIX)?;
    let sep_pos = stem.rfind(CHALLENGE_SEP)?;
    let scenario = &stem[..sep_pos];
    if scenario.is_empty() {
        return None;
    }
    let ts_str = &stem[sep_pos + CHALLENGE_SEP.len()..];
    let naive = NaiveDateTime::parse_from_str(ts_str, FILENAME_TS_FORMAT).ok()?;
    let local = Local.from_local_datetime(&naive).earliest()?; // DST gap → None
    Some(ParsedFilename {
        scenario: scenario.to_string(),
        played_at: local.with_timezone(&Utc),
    })
}

struct ParsedFilename {
    scenario: String,
    played_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::path::PathBuf;

    // ---------- temp helpers (std only) ----------

    fn temp_tag(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .subsec_nanos();
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "kovaaks-ingest-{tag}-{}-{nanos}-{seq}",
            std::process::id()
        ))
    }

    fn cleanup(path: &Path) {
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Replica of a real KovaaK's stats CSV (CRLF endings, header rows, kill
    /// rows, metadata footer) with the plan's verified footer values.
    fn fixture_csv() -> String {
        [
            "Kill #,Timestamp,Bot,Weapon,TTK,Shots,Hits,Accuracy,Damage Done,Damage Possible,Efficiency,Cheated,OverShots",
            "1,6.802,Certificate v2 60qi,vulcan,-,2,2,100.0,200.0,100.0,1.0,false,0",
            "2,7.401,Certificate v2 60qi,vulcan,-,2,2,100.0,200.0,100.0,1.0,false,0",
            "",
            "Weapon,Shots,Hits,Damage Done,Damage Possible,,Sens Scale,Horiz Sens,Vert Sens,FOV,Hide Gun,Crosshair,Crosshair Scale,Crosshair Color,ADS Sens,ADS Zoom Scale,Avg Target Scale,Avg Time Dilation",
            "LG,101,55,55.0,101.0,,1.0,0.2768,0.2768,103.0,false,dot.png,0.8,00FFFFFF,1.0,1.0,1.0,1.0",
            "",
            "Kills:,2",
            "Deaths:,0",
            "Fight Time:,30.0",
            "Horiz Sens:,27.700001",
            "Vert Sens:,27.700001",
            "DPI:,6400",
            "FOV:,103.0",
            "Resolution:,2560x1440",
            "Score:,959.120239",
            "Hit Count:,101",
            "Avg FPS:,420.766052",
            "Resolution Scale:,100.0",
        ]
        .join("\r\n")
    }

    /// Write the fixture under `dir` with a filename timestamped
    /// `now - days_back` days (local time, real KovaaK's naming).
    fn write_fixture(dir: &Path, days_back: i64, scenario: &str) -> PathBuf {
        std::fs::create_dir_all(dir).expect("create stats dir");
        let ts: DateTime<Local> = Local::now() - chrono::Duration::days(days_back);
        let name = format!(
            "{scenario} - Challenge - {} Stats.csv",
            ts.format(FILENAME_TS_FORMAT)
        );
        let path = dir.join(name);
        std::fs::write(&path, fixture_csv()).expect("write fixture");
        path
    }

    fn local_utc(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        let naive = NaiveDateTime::parse_from_str(
            &format!("{y:04}.{m:02}.{d:02}-{h:02}.{mi:02}.{s:02}"),
            FILENAME_TS_FORMAT,
        )
        .expect("valid fixture naive ts");
        Local
            .from_local_datetime(&naive)
            .earliest()
            .expect("resolvable local time")
            .with_timezone(&Utc)
    }

    // ---------- parse_stats_csv ----------

    #[test]
    fn parses_real_fixture_format_with_embedded_separator_scenario() {
        let dir = temp_tag("parse");
        let scenario = "Aim - God - Hard";
        let path = write_fixture(&dir, 1, scenario);
        let parsed = parse_stats_csv(&path).expect("fixture must parse");
        assert_eq!(parsed.scenario, scenario);
        assert_eq!(parsed.score, 959.120239);
        assert_eq!(parsed.hit_count, 101);
        assert_eq!(parsed.avg_fps, 420.766052);
        // Timestamp comes from the filename as local → UTC: verify against the
        // same conversion performed via the public chrono API.
        let expected = {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let stem = name.strip_suffix(STATS_SUFFIX).unwrap();
            let ts_str = &stem[stem.rfind(CHALLENGE_SEP).unwrap() + CHALLENGE_SEP.len()..];
            let naive = NaiveDateTime::parse_from_str(ts_str, FILENAME_TS_FORMAT).unwrap();
            Local
                .from_local_datetime(&naive)
                .earliest()
                .unwrap()
                .with_timezone(&Utc)
        };
        assert_eq!(parsed.played_at, expected);
        cleanup(&dir);
    }

    #[test]
    fn parses_the_verified_ground_truth_filename() {
        // Real file observed on this machine (plan ground truth).
        let dir = temp_tag("groundtruth");
        let path = dir.join("VT Pasu Advanced S5 - Challenge - 2026.07.27-15.52.38 Stats.csv");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, fixture_csv()).unwrap();
        let parsed = parse_stats_csv(&path).expect("real-world name must parse");
        assert_eq!(parsed.scenario, "VT Pasu Advanced S5");
        assert_eq!(parsed.played_at, local_utc(2026, 7, 27, 15, 52, 38));
        cleanup(&dir);
    }

    #[test]
    fn rejects_non_stats_filenames() {
        let dir = temp_tag("badname");
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "random.csv",
            "Gridshot - Challenge - not-a-date Stats.csv",
            "Gridshot Stats.csv",
        ] {
            let path = dir.join(name);
            std::fs::write(&path, fixture_csv()).unwrap();
            assert!(parse_stats_csv(&path).is_err(), "{name} must not parse");
        }
        cleanup(&dir);
    }

    #[test]
    fn rejects_missing_or_garbled_footer() {
        let dir = temp_tag("badfooter");
        std::fs::create_dir_all(&dir).unwrap();
        // No Score: line.
        let no_score = dir.join("Gridshot - Challenge - 2026.08.01-12.00.00 Stats.csv");
        std::fs::write(&no_score, "Kill #,Timestamp\n1,1.0\nAvg FPS:,300.0\n").unwrap();
        assert!(parse_stats_csv(&no_score).is_err());
        // Non-numeric score value.
        let bad_score = dir.join("Gridshot - Challenge - 2026.08.01-12.01.00 Stats.csv");
        std::fs::write(&bad_score, "Score:,N/A\nHit Count:,101\nAvg FPS:,300.0\n").unwrap();
        assert!(parse_stats_csv(&bad_score).is_err());
        cleanup(&dir);
    }

    // ---------- scan_dir ----------

    const SID: &str = "76561190000000001";

    fn fresh_store(dir: &Path) -> Store {
        std::fs::create_dir_all(dir).expect("create temp dir");
        Store::open(&dir.join("test.db")).expect("open temp store")
    }

    #[test]
    fn scan_inserts_then_rescan_is_idempotent() {
        let dir = temp_tag("scan-insert");
        let path = write_fixture(&dir, 1, "Fixture Scenario");
        let store = fresh_store(&dir);
        // File is 1 day old → cutoff 2 days ago lets it through.
        let cutoff = Utc::now() - chrono::Duration::days(2);

        let report = scan_dir(&dir, cutoff, &store, SID);
        assert_eq!(report.seen, 1, "one csv file");
        assert_eq!(report.inserted, 1, "first scan inserts");
        assert_eq!(report.skipped_old, 0);
        assert!(report.errors.is_empty(), "{:?}", report.errors);

        let plays = store.plays_history(SID, "Fixture Scenario").expect("plays");
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].score, 959.120239);
        assert_eq!(plays[0].hit_count, 101);
        assert_eq!(plays[0].avg_fps, 420.766052);
        assert_eq!(
            plays[0].played_at,
            parse_stats_csv(&path).unwrap().played_at
        );

        // Re-scan: same file, zero new rows (idempotent by csv_path).
        let again = scan_dir(&dir, cutoff, &store, SID);
        assert_eq!(again.seen, 1);
        assert_eq!(again.inserted, 0, "re-scan must insert nothing");
        assert_eq!(
            store.plays_history(SID, "Fixture Scenario").unwrap().len(),
            1
        );
        cleanup(&dir);
    }

    #[test]
    fn scan_skips_files_older_than_cutoff() {
        let dir = temp_tag("scan-skip");
        write_fixture(&dir, 1, "Old Fixture Scenario");
        let store = fresh_store(&dir);
        // cutoff = now: a 1-day-old file is in the past → forward-only skip.
        let report = scan_dir(&dir, Utc::now(), &store, SID);
        assert_eq!(report.seen, 1);
        assert_eq!(report.inserted, 0, "old files must not backfill");
        assert_eq!(report.skipped_old, 1);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(store
            .plays_history(SID, "Old Fixture Scenario")
            .unwrap()
            .is_empty());
        cleanup(&dir);
    }

    #[test]
    fn scan_reports_errors_without_aborting() {
        let dir = temp_tag("scan-errors");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("garbage.csv"), "not a stats file\n").unwrap();
        write_fixture(&dir, 1, "Good Scenario");
        // Non-csv files are ignored entirely.
        std::fs::write(dir.join("notes.txt"), "skip me").unwrap();
        let store = fresh_store(&dir);
        let report = scan_dir(&dir, Utc::now() - chrono::Duration::days(2), &store, SID);
        assert_eq!(report.seen, 2, "only .csv files count as seen");
        assert_eq!(report.inserted, 1);
        assert_eq!(report.errors.len(), 1, "garbage.csv must be one error");
        assert!(report.errors[0].contains("garbage.csv"));
        cleanup(&dir);
    }

    #[test]
    fn scan_missing_dir_is_a_single_error_not_a_panic() {
        let dir = temp_tag("scan-missing");
        let store = fresh_store(&dir);
        let report = scan_dir(&dir.join("nope"), Utc::now(), &store, SID);
        assert_eq!(report.seen, 0);
        assert_eq!(report.inserted, 0);
        assert_eq!(report.errors.len(), 1);
        cleanup(&dir);
    }

    // ---------- ensure_cutoff (forward-only guarantee) ----------

    #[test]
    fn ensure_cutoff_sets_once_then_returns_stored_value() {
        let dir = temp_tag("cutoff-fresh");
        let store = fresh_store(&dir);
        assert_eq!(store.get_meta(FIRST_RUN_CUTOFF_KEY).unwrap(), None);

        let first = ensure_cutoff(&store).expect("first call sets cutoff");
        assert!(first <= Utc::now(), "cutoff is set to 'now'");
        assert!(first > Utc::now() - chrono::Duration::minutes(1));
        assert!(store.get_meta(FIRST_RUN_CUTOFF_KEY).unwrap().is_some());

        std::thread::sleep(std::time::Duration::from_millis(30));
        let second = ensure_cutoff(&store).expect("second call reads cutoff");
        assert_eq!(second, first, "cutoff must never move once set");

        // Deleting the meta key re-arms the guard (documented behaviour).
        store.set_meta(FIRST_RUN_CUTOFF_KEY, "").unwrap();
        assert!(
            ensure_cutoff(&store).is_err(),
            "corrupt value must surface, not silently reset"
        );
        cleanup(&dir);
    }

    #[test]
    fn ensure_cutoff_returns_preexisting_value() {
        let dir = temp_tag("cutoff-preexisting");
        let store = fresh_store(&dir);
        store
            .set_meta(FIRST_RUN_CUTOFF_KEY, "2020-01-01T00:00:00+00:00")
            .unwrap();
        let cutoff = ensure_cutoff(&store).unwrap();
        assert_eq!(cutoff, Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap());
        cleanup(&dir);
    }

    #[test]
    fn csv_ingest_facade_delegates() {
        let dir = temp_tag("facade");
        let path = write_fixture(&dir, 1, "Facade Scenario");
        let store = fresh_store(&dir);
        let ingest = CsvIngest::new();
        let cutoff = ingest.ensure_cutoff(&store).unwrap();
        // The fixture was written "now - 1 day" but ensure_cutoff just set the
        // cutoff to now → forward-only skips it. Parse directly instead.
        let parsed = ingest.parse(&path).unwrap();
        assert_eq!(parsed.scenario, "Facade Scenario");
        let report = ingest.scan(&dir, cutoff - chrono::Duration::days(2), &store, SID);
        assert_eq!((report.seen, report.inserted), (1, 1));
        cleanup(&dir);
    }

    // ---------- live (read-only, #[ignore]) ----------

    /// Real KovaaK's stats dir on this machine (read-only w.r.t. the game
    /// dir; the DB is a throwaway temp file). Asserts the 1,000+ file corpus
    /// is scannable with a low error rate and that filenames + footers yield
    /// real scores. `cutoff = now` ⇒ nothing inserts (forward-only).
    #[test]
    #[ignore]
    fn live_scan_real_stats_dir_read_only_with_temp_db() {
        const STATS_DIR: &str =
            r"C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats";
        let dir = Path::new(STATS_DIR);
        if !dir.is_dir() {
            eprintln!("live test skipped: {STATS_DIR} not present on this machine");
            return;
        }

        // Direct-parse sweep: counts files, parse errors, positive scores.
        let mut seen = 0usize;
        let mut parse_errors = 0usize;
        let mut any_positive_score = false;
        for entry in std::fs::read_dir(dir).expect("stats dir readable") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("csv") {
                continue;
            }
            seen += 1;
            match parse_stats_csv(&path) {
                Ok(parsed) => {
                    if parsed.score > 0.0 {
                        any_positive_score = true;
                    }
                }
                Err(e) => {
                    parse_errors += 1;
                    if parse_errors <= 5 {
                        eprintln!("parse error: {path:?}: {e}");
                    }
                }
            }
        }
        eprintln!("live sweep: {seen} csv files, {parse_errors} parse errors");
        assert!(seen >= 1000, "expected >= 1000 stats files, saw {seen}");
        assert!(
            parse_errors * 20 < seen,
            "parse error rate must be < 5% ({parse_errors}/{seen})"
        );
        assert!(
            any_positive_score,
            "at least one file must parse to score > 0"
        );

        // scan_dir with cutoff=now must insert nothing (forward-only), using
        // a temp DB so the real store is never touched.
        let tmp = temp_tag("live-db");
        let store = fresh_store(&tmp);
        let report = scan_dir(dir, Utc::now(), &store, SID);
        eprintln!(
            "scan_dir: seen={} inserted={} skipped_old={} errors={}",
            report.seen,
            report.inserted,
            report.skipped_old,
            report.errors.len()
        );
        assert_eq!(report.seen, seen, "scan_dir must see the same corpus");
        assert_eq!(report.inserted, 0, "cutoff=now ⇒ nothing inserts");
        assert!(
            report.errors.len() * 20 < report.seen,
            "scan error rate must be < 5% ({}/{})",
            report.errors.len(),
            report.seen
        );
        cleanup(&tmp);
    }
}
