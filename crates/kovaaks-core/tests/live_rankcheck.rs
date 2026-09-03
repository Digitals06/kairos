//! Live repro: run the rank engine on real stored snapshots.
//! Run: cargo test -p kovaaks-core --test live_rankcheck -- --ignored --nocapture
use kovaaks_core::{Registry, Store};

#[test]
#[ignore]
fn live_rankcheck_voltaic_s3_and_s5() {
    let store = Store::open(&std::path::PathBuf::from(
        "std::env::var("KAIROS_LIVE_DB").unwrap_or_default()",
    ))
    .unwrap();
    let registry = Registry;
    let sid = "76561190000000001";
    let mut report = String::new();

    for benchmark_id in [266i64, 460] {
        let Some((bench, difficulty)) = registry.by_id(benchmark_id as u64) else {
            continue;
        };
        let history = store.history(sid, benchmark_id).unwrap();
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
