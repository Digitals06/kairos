//! Domain types mirroring the KovaaK's webapp-backend payloads and the
//! embedded evxl benchmark registry (plan Task 1.1).
//!
//! Numeric scores are `f64` because the API reports benchmark progress and
//! scenario scores as floats (e.g. `"Score:,959.120239"` in stats CSVs); ranks
//! are 1-based indices into a difficulty's `rank_colors`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Current progress on one benchmark for one player (public
/// `benchmark-progress-rank-benchmark` payload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkProgress {
    /// Overall benchmark progress score (e.g. 180000).
    #[serde(rename = "benchmark_progress")]
    pub benchmark_progress: f64,
    /// 1-based rank index into the benchmark's difficulty `rank_colors`.
    #[serde(rename = "overall_rank")]
    pub overall_rank: u32,
    /// Per-category progress keyed by category name (Clicking, Tracking, ...).
    pub categories: HashMap<String, CategoryProgress>,
}

/// Progress within a single category of a benchmark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryProgress {
    #[serde(rename = "benchmark_progress")]
    pub benchmark_progress: f64,
    /// 1-based rank index into the same `rank_maxes` ladder.
    #[serde(rename = "category_rank")]
    pub category_rank: u32,
    /// Category score thresholds, ascending, one per rank tier.
    #[serde(rename = "rank_maxes")]
    pub rank_maxes: Vec<f64>,
    /// Scenario results keyed by scenario name.
    pub scenarios: HashMap<String, ScenarioEntry>,
}

/// One played scenario inside a category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioEntry {
    pub score: f64,
    /// Position on the scenario's public leaderboard.
    #[serde(rename = "leaderboard_rank")]
    pub leaderboard_rank: u64,
    /// 1-based tier index for this scenario (index into `rank_colors`).
    #[serde(rename = "scenario_rank")]
    pub scenario_rank: u32,
    /// Scenario score thresholds, ascending, one per rank tier.
    #[serde(rename = "rank_maxes")]
    pub rank_maxes: Vec<f64>,
    /// KovaaK's leaderboard id for pulling the global scores list.
    #[serde(rename = "leaderboard_id")]
    pub leaderboard_id: u64,
}

/// One rank tier: a display name and its official hex color.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankTier {
    pub name: String,
    /// Hex color string as published by evxl (e.g. `#999999`).
    pub color: String,
}

/// Serde helpers: the registry encodes `rankColors` as a JSON object whose
/// insertion order is meaningful (worst tier first). serde_json's default map
/// types would reorder keys, so we (de)serialize the field to an ordered
/// `Vec<RankTier>` directly. A sequence of `{name, color}` is also accepted.
pub(crate) mod ordered_rank_tiers {
    use serde::de::{MapAccess, SeqAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};

    use super::RankTier;

    pub fn deserialize<'de, D>(d: D) -> Result<Vec<RankTier>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Vec<RankTier>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object mapping tier name to hex color")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut tiers = Vec::new();
                while let Some((name, color)) = map.next_entry::<String, String>()? {
                    tiers.push(RankTier { name, color });
                }
                Ok(tiers)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut tiers = Vec::new();
                while let Some(tier) = seq.next_element::<RankTier>()? {
                    tiers.push(tier);
                }
                Ok(tiers)
            }
        }
        d.deserialize_any(V)
    }

    pub fn serialize<S: Serializer>(tiers: &Vec<RankTier>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(tiers.len()))?;
        for tier in tiers {
            map.serialize_entry(&tier.name, &tier.color)?;
        }
        map.end()
    }
}

/// One difficulty of a benchmark (e.g. VT S5 "Novice" → kovaaks id 459).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Difficulty {
    /// Registry `difficultyName` (e.g. "Novice", "Elite (Unofficial)").
    #[serde(rename = "difficultyName", alias = "name")]
    pub name: String,
    /// KovaaK's webapp-backend benchmark id to query progress with.
    #[serde(rename = "kovaaksBenchmarkId", alias = "kovaaks_benchmark_id")]
    pub kovaaks_benchmark_id: u64,
    /// In-game share code (e.g. `KovaaKsQuestingSunsetorangeSmg`).
    #[serde(rename = "sharecode")]
    pub sharecode: String,
    /// Rank tiers ordered worst → best (registry document order, preserved
    /// verbatim from the evxl JSON object).
    #[serde(
        rename = "rankColors",
        alias = "rank_colors",
        with = "ordered_rank_tiers"
    )]
    pub rank_colors: Vec<RankTier>,
    /// Scenario categories in this difficulty.
    #[serde(rename = "categories", default)]
    pub categories: Vec<serde_json::Value>,
}

/// A benchmark definition from the evxl registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkDef {
    /// Registry `benchmarkName` (e.g. "Voltaic S5").
    #[serde(rename = "benchmarkName", alias = "name")]
    pub name: String,
    /// Short tag used in the UI (e.g. "VT").
    #[serde(rename = "abbreviation", alias = "abbreviation")]
    pub abbreviation: String,
    /// Benchmark accent hex color (e.g. `#02A2DA`).
    #[serde(rename = "color")]
    pub color: String,
    /// Rank aggregation mode (e.g. "basic", "vt-energy").
    #[serde(rename = "rankCalculation", alias = "rank_calculation")]
    pub rank_calculation: String,
    /// Link to the community spreadsheet for this benchmark.
    #[serde(rename = "spreadsheetURL", alias = "spreadsheet_url")]
    pub spreadsheet_url: String,
    /// Difficulties, in registry order.
    #[serde(rename = "difficulties")]
    pub difficulties: Vec<Difficulty>,
}

/// A resolved KovaaK's player profile (evxl `/api/steam` response).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerProfile {
    /// 17-digit SteamID64.
    #[serde(rename = "steam_id", alias = "steamid")]
    pub steam_id: String,
    /// Persona name (e.g. "Digitals").
    #[serde(rename = "persona", alias = "personaname")]
    pub persona: String,
    /// Full avatar URL (may be empty).
    #[serde(rename = "avatar_url", alias = "avatarfull")]
    pub avatar_url: String,
    /// ISO country code (e.g. "FR"); empty when unknown.
    #[serde(rename = "country", alias = "loccountrycode")]
    pub country: String,
}

/// One local play parsed from a KovaaK's stats CSV.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayRecord {
    /// Scenario name parsed from the CSV filename.
    pub scenario: String,
    /// Play timestamp parsed from the filename (UTC).
    pub played_at: chrono::DateTime<chrono::Utc>,
    /// Footer `Score:` value.
    pub score: f64,
    /// Footer `Hit Count:` value.
    pub hit_count: u64,
    /// Footer `Avg FPS:` value.
    pub avg_fps: f64,
    /// Ingest provenance; currently always `"csv"`.
    pub source: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Verified against the live KovaaK's webapp-backend during recon
    /// (2026-09-02), plan Task 1.1 ground truth: VT S5 Novice progress sample —
    /// benchmark_progress 180000, overall_rank 4, VT Pasu scenario score
    /// 128161, leaderboard_id 98059. Scenario/category map keys beyond the
    /// verified fields are fixture placeholders.
    const VT_S5_NOVICE_PROGRESS: &str = r#"{
        "benchmark_progress": 180000,
        "overall_rank": 4,
        "categories": {
            "Clicking": {
                "benchmark_progress": 180000,
                "category_rank": 4,
                "rank_maxes": [40000, 80000, 120000, 160000],
                "scenarios": {}
            },
            "Tracking": {
                "benchmark_progress": 180000,
                "category_rank": 4,
                "rank_maxes": [40000, 80000, 120000, 160000],
                "scenarios": {
                    "VT Pasu Novice S5": {
                        "score": 128161,
                        "leaderboard_rank": 102,
                        "scenario_rank": 4,
                        "rank_maxes": [40000, 80000, 120000, 160000],
                        "leaderboard_id": 98059
                    }
                }
            },
            "Switching": {
                "benchmark_progress": 180000,
                "category_rank": 4,
                "rank_maxes": [40000, 80000, 120000, 160000],
                "scenarios": {}
            }
        }
    }"#;

    #[test]
    fn deserializes_verified_vt_s5_novice_progress_sample() {
        let p: BenchmarkProgress =
            serde_json::from_str(VT_S5_NOVICE_PROGRESS).expect("sample must parse");
        assert_eq!(p.benchmark_progress, 180000.0);
        assert_eq!(p.overall_rank, 4);
        let pasu = &p.categories["Tracking"].scenarios["VT Pasu Novice S5"];
        assert_eq!(pasu.score, 128161.0);
        assert_eq!(pasu.leaderboard_id, 98059);
        assert_eq!(pasu.scenario_rank, 4);
    }

    #[test]
    fn difficulty_rank_colors_keep_document_order_worst_to_best() {
        // Keys deliberately NOT alphabetical: a BTreeMap-backed parse would
        // flip Silver/Iron and silently break worst→best tier ordering.
        // (r## needed: the JSON contains `"#` hex colors.)
        const D: &str = r##"{
            "difficultyName": "Novice",
            "kovaaksBenchmarkId": 459,
            "sharecode": "KovaaKsExample",
            "rankColors": {
                "Silver": "#CBD9E6",
                "Iron": "#999999"
            },
            "categories": []
        }"##;
        let d: Difficulty = serde_json::from_str(D).expect("difficulty must parse");
        assert_eq!(d.name, "Novice");
        assert_eq!(d.kovaaks_benchmark_id, 459);
        assert_eq!(d.rank_colors.len(), 2);
        assert_eq!(d.rank_colors[0].name, "Silver");
        assert_eq!(d.rank_colors[1].name, "Iron");
    }

    #[test]
    fn play_record_round_trips_through_serde() {
        let rec = PlayRecord {
            scenario: "Gridshot".into(),
            played_at: chrono::Utc.with_ymd_and_hms(2026, 9, 2, 12, 0, 0).unwrap(),
            score: 959.120239,
            hit_count: 145,
            avg_fps: 239.5,
            source: "csv".into(),
        };
        let json = serde_json::to_string(&rec).expect("serialize");
        let back: PlayRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, rec);
        assert!(json.contains("csv"));
    }
}
