//! Live regression harness: run the rank engine on real stored snapshots.
//!
//! Skipped unless both env vars are set (they point at YOUR local data, which
//! is not part of the repo):
//!   KAIROS_LIVE_DB        path to a Kairos store.db
//!   KAIROS_LIVE_STEAM_ID  the SteamID64 whose snapshots to load
//! Run: cargo test -p kovaaks-core --test live_rankcheck -- --ignored --nocapture
use kovaaks_core::{Registry, Store};

#[test]
#[ignore]
fn live_rankcheck_voltaic_s3_and_s5() {
    let Ok(db_path) = std::env::var("KAIROS_LIVE_DB") else {
        eprintln!("skipped: KAIROS_LIVE_DB not set");
        return;
    };
    let Ok(sid) = std::env::var("KAIROS_LIVE_STEAM_ID") else {
        eprintln!("skipped: KAIROS_LIVE_STEAM_ID not set");
        return;
    };
    let store = Store::open(&std::path::PathBuf::from(db_path)).unwrap();
    let registry = Registry;
    let mut report = String::new();

    for benchmark_id in [
        266, 460, 2070, 687, 2336, 458, 459, 2834, 2835, 2335, 688, 686, 2843, 2844, 2845, 2287,
        2305, 985, 1883, 1927, 2793, 711, 712, 582, 583, 584, 84, 636, 1822, 581, 738, 739, 740,
        535, 550, 319, 320, 826, 2005, 2093, 851, 852, 853, 2110, 2111, 2112, 880, 977, 2064, 2819,
        696, 697, 576, 577, 675, 802, 997, 2168, 877, 2223, 2377, 480, 2384, 2400, 2419, 959, 962,
        988, 644, 645, 646, 2109, 2488, 600, 601, 909, 536, 540, 821, 822, 823, 929, 732, 733,
        2724, 2725, 2727, 2510, 2846, 279, 286, 227, 379, 469, 638, 653, 656, 235, 237, 253, 603,
        604, 1908, 904, 906, 2656,
    ] {
        let Some((bench, difficulty)) = registry.by_id(benchmark_id as u64) else {
            continue;
        };
        let history = store.history(&sid, benchmark_id).unwrap();
        let Some(snap) = history.last() else {
            report.push_str(&format!("{}: no snapshot\n", bench.name));
            continue;
        };
        // Mirror the app's stored_to_progress conversion.
        use kovaaks_core::types::{CategoryProgress, ScenarioEntry};
        let mut categories: Vec<(String, CategoryProgress)> = Vec::new();
        for row in &snap.scenarios {
            let entry = ScenarioEntry {
                score: row.score as f64,
                leaderboard_rank: row.leaderboard_rank.max(0) as u64,
                scenario_rank: row.scenario_rank.max(0) as u32,
                rank_maxes: row.rank_maxes.clone(),
                leaderboard_id: 0,
            };
            match categories.last_mut() {
                Some((name, cat)) if *name == row.category => {
                    cat.scenarios.push((row.scenario.clone(), entry));
                }
                _ => categories.push((
                    row.category.clone(),
                    CategoryProgress {
                        benchmark_progress: 0.0,
                        category_rank: row.category_rank.max(0) as u32,
                        rank_maxes: Vec::new(),
                        scenarios: vec![(row.scenario.clone(), entry)],
                    },
                )),
            }
        }
        let progress = kovaaks_core::types::BenchmarkProgress {
            benchmark_progress: snap.benchmark_progress as f64,
            overall_rank: snap.overall_rank.max(0) as u32,
            categories,
        };
        let spans = kovaaks_core::rankcalc::subcategory_spans(&difficulty);
        report.push_str(&format!(
            "   spans: {:?}\n",
            spans
                .iter()
                .map(|(c, n, s)| format!("{c}/{s}x{n}"))
                .collect::<Vec<_>>()
        ));
        let result = kovaaks_core::rankcalc::compute_rank(&progress, bench, &difficulty);
        let order = kovaaks_core::rankcalc::scenario_order(&progress, &difficulty);
        let names: Vec<&str> = order.iter().take(4).map(|(n, _)| *n).collect();
        let method = match result.method {
            kovaaks_core::rankcalc::MethodSource::Engine => "Engine",
            _ => "ApiFallback",
        };
        report.push_str(&format!(
            "== {} / {}\n   api_rank={} scenarios={}\n   engine rank={} name={} method={}\n   order[0..4]={:?}\n",
            bench.name, difficulty.name, snap.overall_rank, snap.scenarios.len(),
            result.rank, result.name, method, names
        ));
        for (cat_name, cat) in &progress.categories {
            for (scen, entry) in &cat.scenarios {
                report.push_str(&format!(
                    "   row: {} | {} | score={} srank={}\n",
                    scen, cat_name, entry.score, entry.scenario_rank
                ));
            }
        }
    }
    println!("{report}");
}
