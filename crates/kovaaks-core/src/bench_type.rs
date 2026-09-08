//! Benchmark-type tags: evxl's own filter taxonomy, adopted wholesale.
//!
//! evxl tags each benchmark with zero or more tags; everything without a
//! specialty tag shows under "Mixed". The tag assignments below were read off
//! evxl's live filter (tag counts: Mixed 85, Tracking 14, Static 12, Clicking 6,
//! Ground 5, Precise 3, Smooth 3, Micro 2, Dynamic 2, Reactive 2, Evasive 1,
//! Switching 1 — matches this table exactly for the embedded 125-benchmark
//! registry). Benchmarks added by future registry updates fall back to a
//! keyword classifier; benchmarks matching the curated table always use the
//! curated tags so the filter mirrors evxl 1:1.

use crate::registry::Registry;

/// The evxl filter tabs, in evxl's order.
pub const BENCHMARK_TYPES: &[&str] = &[
    "Mixed",
    "Tracking",
    "Static",
    "Clicking",
    "Ground",
    "Precise",
    "Smooth",
    "Micro",
    "Dynamic",
    "Reactive",
    "Evasive",
    "Switching",
];

/// evxl's curated tag assignments (exact benchmark names). One benchmark may
/// carry several tags (e.g. Aimerz+ Static Clicking = Static + Clicking).
const EVXL_TAGS: &[(&str, &[&str])] = &[
    (
        "Tracking",
        &[
            "m0narcS & hizku Precise Tracking",
            "m0narcS & hizku Reactive Tracking",
            "m0narcS & hizku Tracking",
            "Aimerz+ Precise Tracking",
            "Aimerz+ Reactive Tracking",
            "cA Ground Tracking S1",
            "Control Track Dojo 道場",
            "Ground Track DOJO 道場",
            "Pasu Track DOJO 道場",
            "thundah Precise Tracking",
            "Astro Tracking Benchmark",
            "Group MIYU Tracking",
            "Pojk's Evil Tracking Benchmark",
            "e1se Tracking Routine",
        ],
    ),
    (
        "Static",
        &[
            "Lemon Static Benchmark",
            "Aimerz+ Static Clicking",
            "cA Static S1",
            "Lemon Static S2",
            "Dantes Static",
            "Deadman's Poke Static Benchmarks",
            "Deadman's Static Benchmarks S1",
            "Deadman's Static Benchmarks S2",
            "Deadman's Static Benchmarks S3",
            "Serpent's Fast Static S1",
            "Setsunai's Static Benchmark",
            "Static Clicking Suite",
        ],
    ),
    (
        "Clicking",
        &[
            "Aimerz+ Dynamic Clicking",
            "Aimerz+ Static Clicking",
            "✯ Stellar - Speed Clicking Benchmarks",
            "Speedclick Archive Benchmarks",
            "Static Clicking Suite",
            "TZY SpeedClicking",
        ],
    ),
    (
        "Ground",
        &[
            "Jade Palace Ground",
            "Ground Track DOJO 道場",
            "Aimbeast Ground",
            "cA Ground Tracking S1",
            "Peter Ground Technology",
        ],
    ),
    (
        "Precise",
        &[
            "m0narcS & hizku Precise Tracking",
            "Aimerz+ Precise Tracking",
            "thundah Precise Tracking",
        ],
    ),
    (
        "Smooth",
        &[
            "xyz Smoothness",
            "xyz Smoothness V2",
            "e1se Smooth Benchmark",
        ],
    ),
    ("Micro", &["Anima Micro v1", "Anima Micro v2"]),
    (
        "Dynamic",
        &["Jade Palace Dynamic", "Aimerz+ Dynamic Clicking"],
    ),
    (
        "Reactive",
        &[
            "m0narcS & hizku Reactive Tracking",
            "Aimerz+ Reactive Tracking",
        ],
    ),
    ("Evasive", &["Aimerz+ Evasive Switching"]),
    ("Switching", &["Aimerz+ Evasive Switching"]),
];

fn re_contains(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// Keyword fallback for benchmarks absent from the curated table (future
/// registry additions). Conservative: anything unresolved lands in Mixed,
/// which is where evxl puts everything without a specialty anyway.
fn fallback_tags(bench: &crate::types::BenchmarkDef) -> &'static [&'static str] {
    let name = bench.name.to_lowercase();
    if re_contains(&name, &["static"]) {
        return &["Static"];
    }
    if re_contains(&name, &["track"]) {
        return &["Tracking"];
    }
    if re_contains(&name, &["smooth"]) {
        return &["Smooth"];
    }
    if re_contains(&name, &["ground"]) {
        return &["Ground"];
    }
    if re_contains(&name, &["speed click", "speedclick"]) {
        return &["Clicking"];
    }
    &[]
}

/// The tags for one benchmark: curated evxl tags when the name matches, else
/// the keyword fallback, else Mixed (evxl's catch-all for unspecialized
/// benchmarks).
pub fn benchmark_types_for_def(bench: &crate::types::BenchmarkDef) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = EVXL_TAGS
        .iter()
        .filter(|(_, names)| names.contains(&bench.name.as_str()))
        .map(|(tag, _)| *tag)
        .collect();
    if out.is_empty() {
        out.extend_from_slice(fallback_tags(bench));
    }
    if out.is_empty() {
        out.push("Mixed");
    }
    // Order by the canonical tab order.
    let mut ordered: Vec<&'static str> = BENCHMARK_TYPES
        .iter()
        .copied()
        .filter(|ty| out.contains(ty))
        .collect();
    for ty in out {
        if !ordered.contains(&ty) {
            ordered.push(ty);
        }
    }
    ordered
}

/// Convenience wrapper taking a benchmark id.
pub fn benchmark_types(registry: &Registry, benchmark_id: i64) -> Vec<&'static str> {
    let Some((bench, _)) = registry.by_id(benchmark_id as u64) else {
        return vec![];
    };
    benchmark_types_for_def(bench)
}

/// A benchmark is "pure" for tag T when T is its ONLY tag (Mixed is never
/// pure). Style tabs that want evxl's specialty view filter on this.
pub fn pure_style(bench: &crate::types::BenchmarkDef) -> Option<&'static str> {
    let tags = benchmark_types_for_def(bench);
    if tags.len() == 1 && tags[0] != "Mixed" {
        Some(tags[0])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Registry;

    #[test]
    fn every_registry_benchmark_gets_at_least_one_tag() {
        let registry = Registry;
        for bench in registry.all() {
            let tags = benchmark_types_for_def(bench);
            assert!(!tags.is_empty(), "{} has no tags", bench.name);
            for ty in &tags {
                assert!(
                    BENCHMARK_TYPES.contains(ty),
                    "{} produced unknown tag {ty}",
                    bench.name
                );
            }
        }
    }

    #[test]
    fn curated_tags_match_evxl_counts() {
        let registry = Registry;
        let mut counts = std::collections::BTreeMap::new();
        for bench in registry.all() {
            for ty in benchmark_types_for_def(bench) {
                *counts.entry(ty).or_insert(0) += 1;
            }
        }
        let expected: &[(&str, usize)] = &[
            ("Mixed", 85),
            ("Tracking", 14),
            ("Static", 12),
            ("Clicking", 6),
            ("Ground", 5),
            ("Precise", 3),
            ("Smooth", 3),
            ("Micro", 2),
            ("Dynamic", 2),
            ("Reactive", 2),
            ("Evasive", 1),
            ("Switching", 1),
        ];
        for (ty, n) in expected {
            assert_eq!(
                counts.get(*ty).copied().unwrap_or(0),
                *n,
                "tag {ty} count mismatch"
            );
        }
    }

    #[test]
    fn known_specialties_resolve() {
        let registry = Registry;
        let deadman = registry
            .all()
            .iter()
            .find(|b| b.name.contains("Deadman's Static Benchmarks S1"))
            .expect("Deadman's");
        assert!(benchmark_types_for_def(deadman).contains(&"Static"));
        let grid = registry
            .all()
            .iter()
            .find(|b| b.name.contains("Mastering Gridshot"))
            .expect("Mastering Gridshot");
        // Not in evxl's Static list -> Mixed (specialty-free benchmark).
        assert_eq!(benchmark_types_for_def(grid), vec!["Mixed"]);
        let vt5 = registry.by_id(460).expect("VT S5").0;
        assert_eq!(benchmark_types_for_def(vt5), vec!["Mixed"]);
    }
}
