//! Embedded evxl.app benchmark registry (plan Task 1.2).
//!
//! The registry JSON is included at compile time via [`include_str!`], so all
//! registry lookups are fully offline. Provenance is recorded in
//! `REGISTRY_META.json` next to the data file.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::types::{BenchmarkDef, Difficulty};

/// Raw registry JSON (125 benchmarks, extracted from the evxl.app SPA on
/// 2026-09-02 — see `assets/REGISTRY_META.json`).
static REGISTRY_JSON: &str = include_str!("../assets/evxl_benchmark_registry.json");

/// Raw registry provenance metadata.
static REGISTRY_META_JSON: &str = include_str!("../assets/REGISTRY_META.json");

/// Provenance of the embedded registry snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RegistryMeta {
    /// Date the registry was extracted from evxl.app (YYYY-MM-DD).
    pub fetched: String,
    /// Source site the registry was extracted from.
    pub source: String,
}

/// The 15 default "major" benchmark families probed during discovery
/// (plan decision 5); deeper scans are opt-in.
static MAJOR_FAMILIES: &[&str] = &[
    "Voltaic",
    "Viscose",
    "Revosect",
    "cAt",
    "MIRA",
    "XYZ",
    "Aimerz+",
    "TSK",
    "Snakbox",
    "Stellar",
    "Tosoku",
    "RXZU",
    "Jade Palace",
    "Point Zero",
    "Continium",
];

/// Read-only access to the embedded registry.
#[derive(Debug, Clone, Copy, Default)]
pub struct Registry;

impl Registry {
    /// The 15 major benchmark family name prefixes, in plan order.
    pub fn major_families() -> &'static [&'static str] {
        MAJOR_FAMILIES
    }

    /// Provenance of the embedded snapshot (fetched date + source).
    pub fn meta(&self) -> RegistryMeta {
        serde_json::from_str(REGISTRY_META_JSON).expect("REGISTRY_META.json must be valid")
    }

    /// Every benchmark in the registry (registry order).
    pub fn all(&self) -> &[BenchmarkDef] {
        static REGISTRY: OnceLock<Vec<BenchmarkDef>> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            serde_json::from_str(REGISTRY_JSON).expect("embedded registry must parse")
        })
    }

    /// Benchmarks evxl actually shows: `hidden != true`. This is the same
    /// visibility rule evxl applies on user pages, so the app's card list can
    /// match it benchmark-for-benchmark.
    pub fn visible(&self) -> impl Iterator<Item = &BenchmarkDef> {
        self.all().iter().filter(|b| !b.hidden)
    }

    /// Find a difficulty by benchmark name + difficulty name
    /// (case/whitespace-tolerant), e.g. `("Voltaic S5", "Novice")` → id 459.
    pub fn find(&self, benchmark_name: &str, difficulty_name: &str) -> Option<Difficulty> {
        let needle = difficulty_name.trim().to_lowercase();
        self.by_name(benchmark_name)?
            .difficulties
            .iter()
            .find(|d| d.name.trim().to_lowercase() == needle)
            .cloned()
    }

    /// Find a benchmark by exact name (case/whitespace-tolerant).
    pub fn by_name(&self, benchmark_name: &str) -> Option<&BenchmarkDef> {
        let needle = benchmark_name.trim().to_lowercase();
        self.all()
            .iter()
            .find(|b| b.name.trim().to_lowercase() == needle)
    }

    /// Find a difficulty by its KovaaK's benchmark id (e.g. 459), returning
    /// the owning benchmark too (ids are unique across the registry).
    pub fn by_id(&self, kovaaks_benchmark_id: u64) -> Option<(&BenchmarkDef, Difficulty)> {
        self.all().iter().find_map(|b| {
            b.difficulties
                .iter()
                .find(|d| d.kovaaks_benchmark_id == kovaaks_benchmark_id)
                .map(|d| (b, d.clone()))
        })
    }

    /// Case-insensitive substring search over benchmark names.
    pub fn search(&self, query: &str) -> Vec<&BenchmarkDef> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.all()
            .iter()
            .filter(|b| b.name.to_lowercase().contains(&needle))
            .collect()
    }
}

/// Shared zero-sized registry handle (`Registry::default()`).
pub fn registry() -> Registry {
    Registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_embeds_all_125_benchmarks() {
        assert_eq!(registry().all().len(), 125);
    }

    #[test]
    fn meta_records_fetch_provenance() {
        let meta = registry().meta();
        assert_eq!(meta.fetched, "2026-09-02");
        assert_eq!(meta.source, "https://evxl.app");
    }

    #[test]
    fn finds_voltaic_s5_novice_as_benchmark_459() {
        let d = registry()
            .find("Voltaic S5", "Novice")
            .expect("VT S5 Novice must exist");
        assert_eq!(d.kovaaks_benchmark_id, 459);
        assert_eq!(d.name, "Novice");
    }

    #[test]
    fn voltaic_s5_novice_tiers_are_sorted_worst_to_best() {
        let d = registry().find("Voltaic S5", "Novice").unwrap();
        // Verified ladder from the registry JSON (plan Task 1.2 ground truth).
        let expected = [
            ("Iron", "#999999"),
            ("Bronze", "#FF9900"),
            ("Silver", "#CBD9E6"),
            ("Gold", "#CAB148"),
        ];
        assert_eq!(d.rank_colors.len(), expected.len());
        for (tier, (name, color)) in d.rank_colors.iter().zip(expected) {
            assert_eq!(tier.name, name);
            assert_eq!(tier.color, color);
        }
        // Plan assertion: tier[0] is Iron #999999.
        assert_eq!(d.rank_colors[0].name, "Iron");
        assert_eq!(d.rank_colors[0].color, "#999999");
    }

    #[test]
    fn find_is_whitespace_and_case_tolerant_on_difficulty() {
        let exact = registry().find("Voltaic S5", "Novice").unwrap();
        let sloppy = registry().find("voltaic s5", " novice ").unwrap();
        assert_eq!(exact.kovaaks_benchmark_id, sloppy.kovaaks_benchmark_id);
    }

    #[test]
    fn by_id_finds_difficulty_459() {
        let reg = registry();
        let (bench, d) = reg.by_id(459).expect("id 459 must resolve to VT S5 Novice");
        assert_eq!(bench.name, "Voltaic S5");
        assert_eq!(d.kovaaks_benchmark_id, 459);
        assert!(reg.by_id(0).is_none());
    }

    #[test]
    fn search_finds_viscose_benchmarks() {
        let reg = registry();
        let hits = reg.search("viscose");
        assert!(!hits.is_empty());
        assert!(hits
            .iter()
            .all(|b| b.name.to_lowercase().contains("viscose")));
    }

    #[test]
    fn major_families_lists_the_15_planned_families() {
        let families = Registry::major_families();
        assert_eq!(
            families,
            &[
                "Voltaic",
                "Viscose",
                "Revosect",
                "cAt",
                "MIRA",
                "XYZ",
                "Aimerz+",
                "TSK",
                "Snakbox",
                "Stellar",
                "Tosoku",
                "RXZU",
                "Jade Palace",
                "Point Zero",
                "Continium",
            ]
        );
    }

    #[test]
    fn every_difficulty_has_a_non_empty_rank_ladder() {
        for bench in registry().all() {
            for d in &bench.difficulties {
                assert!(
                    !d.rank_colors.is_empty(),
                    "{} / {} has no rank tiers",
                    bench.name,
                    d.name
                );
            }
        }
    }
}
