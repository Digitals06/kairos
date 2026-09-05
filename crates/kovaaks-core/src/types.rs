//! Domain types mirroring the KovaaK's webapp-backend payloads and the
//! embedded evxl benchmark registry (plan Task 1.1).
//!
//! Numeric scores are `f64` because the API reports benchmark progress and
//! scenario scores as floats (e.g. `"Score:,959.120239"` in stats CSVs); ranks
//! are 1-based indices into a difficulty's `rank_colors`.

use serde::{Deserialize, Serialize};

/// Serde helper: the webapp-backend emits explicit `null` for rank/id/threshold
/// fields on never-played scenarios (e.g. `"leaderboard_rank": null` with
/// `"score": 0`). Decode those as `T::default()` (0 / empty vec) instead of
/// failing the whole payload.
pub(crate) mod null_default {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de> + Default,
    {
        Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
    }
}

/// Serde helper: some scenarios report numerics as strings (observed live:
/// benchmark 2108 `"score": "3750.00"`). Accept f64, integers, numeric
/// strings, or null (→ 0.0) instead of failing the whole benchmark payload.
pub(crate) mod flex_f64 {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(d: D) -> Result<f64, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Num {
            F(f64),
            I(i64),
            S(String),
        }
        match Option::<Num>::deserialize(d)? {
            None => Ok(0.0),
            Some(Num::F(v)) => Ok(v),
            Some(Num::I(v)) => Ok(v as f64),
            Some(Num::S(s)) => s.trim().parse::<f64>().map_err(serde::de::Error::custom),
        }
    }

    /// Same flexibility for threshold arrays (same payload shape, same risk).
    pub fn deserialize_vec<'de, D>(d: D) -> Result<Vec<f64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Num {
            F(f64),
            I(i64),
            S(String),
        }
        fn one(n: Num) -> Result<f64, String> {
            match n {
                Num::F(v) => Ok(v),
                Num::I(v) => Ok(v as f64),
                Num::S(s) => s.trim().parse::<f64>().map_err(|e| format!("{e}")),
            }
        }
        match Option::<Vec<Num>>::deserialize(d)? {
            None => Ok(Vec::new()),
            Some(v) => v
                .into_iter()
                .map(one)
                .collect::<Result<Vec<_>, _>>()
                .map_err(serde::de::Error::custom),
        }
    }
}

/// Serde helper: decode a JSON object into an order-preserving `Vec<(String, T)>`.
/// `serde_json`'s default map is a BTreeMap (alphabetical), which would lose the
/// API's scenario ordering (author/evxl order). `MapAccess` yields entries in
/// document order, so this keeps it.
pub(crate) mod ordered_map {
    use serde::{Deserialize, Deserializer};
    use std::marker::PhantomData;

    pub fn deserialize<'de, D, T>(d: D) -> Result<Vec<(String, T)>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        struct VM<T>(PhantomData<T>);
        impl<'de, T> serde::de::Visitor<'de> for VM<T>
        where
            T: Deserialize<'de>,
        {
            type Value = Vec<(String, T)>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<Self::Value, A::Error> {
                let mut v = Vec::new();
                while let Some((k, val)) = access.next_entry::<String, T>()? {
                    v.push((k, val));
                }
                Ok(v)
            }
        }
        d.deserialize_map(VM(PhantomData))
    }
}

/// Current progress on one benchmark for one player (public
/// `benchmark-progress-rank-benchmark` payload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkProgress {
    /// Overall benchmark progress score, in in-game display units
    /// (raw API value ÷ 100 — see [`BenchmarkProgress::normalize_scale`]).
    #[serde(rename = "benchmark_progress")]
    pub benchmark_progress: f64,
    /// 1-based rank index into the benchmark's difficulty `rank_colors`.
    #[serde(rename = "overall_rank")]
    pub overall_rank: u32,
    /// Per-category progress in API document order (Clicking, Tracking, ... —
    /// the order the benchmark author defined, which evxl displays).
    #[serde(rename = "categories", deserialize_with = "ordered_map::deserialize")]
    pub categories: Vec<(String, CategoryProgress)>,
}

impl BenchmarkProgress {
    /// Normalize the API's mixed units into in-game display units.
    ///
    /// Verified ground truth (2026-09-02, kovaaks.com): scenario scores come
    /// as centi-scale integers (`VT Pasu Novice S5` raw `128161` ↔ in-game
    /// `1281.61`; leaderboard WR `1704.91` on the same scenario), and so do
    /// benchmark/category progress values and their `rank_maxes` ladders
    /// (user progress 60000 == category rank_maxes[3] 60000). Scenario-level
    /// `rank_maxes`, however, are ALREADY display-scale (555..800 for the
    /// same scenario) — they must not be scaled.
    ///
    /// Call once at the client boundary (`KovaaksClient::benchmark_progress`);
    /// everything downstream (store, ranks, UI) works in display units only.
    pub fn normalize_scale(&mut self) {
        const CENTI: f64 = 100.0;
        self.benchmark_progress /= CENTI;
        for (_, cat) in self.categories.iter_mut() {
            cat.benchmark_progress /= CENTI;
            for max in &mut cat.rank_maxes {
                *max /= CENTI;
            }
            for (_, scen) in cat.scenarios.iter_mut() {
                scen.score /= CENTI;
                // scen.rank_maxes already display-scale — deliberately untouched.
            }
        }
    }
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
    /// Scenario results in API document order (author/evxl ordering).
    #[serde(rename = "scenarios", deserialize_with = "ordered_map::deserialize")]
    pub scenarios: Vec<(String, ScenarioEntry)>,
}

/// One played scenario inside a category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioEntry {
    #[serde(deserialize_with = "flex_f64::deserialize")]
    pub score: f64,
    /// Position on the scenario's public leaderboard; `0` when the payload
    /// reports `null` (never played / unranked).
    #[serde(
        rename = "leaderboard_rank",
        deserialize_with = "null_default::deserialize"
    )]
    pub leaderboard_rank: u64,
    /// 1-based tier index for this scenario (index into `rank_colors`); `0`
    /// when unplayed, mirroring the API's own `scenario_rank: 0` convention.
    #[serde(
        rename = "scenario_rank",
        deserialize_with = "null_default::deserialize"
    )]
    pub scenario_rank: u32,
    /// Scenario score thresholds, ascending, one per rank tier.
    #[serde(rename = "rank_maxes", deserialize_with = "flex_f64::deserialize_vec")]
    pub rank_maxes: Vec<f64>,
    /// KovaaK's leaderboard id for pulling the global scores list; `0` when
    /// the payload reports `null`.
    #[serde(
        rename = "leaderboard_id",
        deserialize_with = "null_default::deserialize"
    )]
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
    /// evxl's own visibility flag: `true` excludes the benchmark from its
    /// user pages (2 hidden entries in the embedded registry). We mirror that
    /// rule in [`crate::registry::Registry::visible`].
    #[serde(
        rename = "hidden",
        alias = "is_hidden",
        default,
        deserialize_with = "null_default::deserialize"
    )]
    pub hidden: bool,
}

/// A resolved KovaaK's player profile (evxl `/api/steam` response).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerProfile {
    /// 17-digit SteamID64.
    #[serde(rename = "steam_id", alias = "steamid")]
    pub steam_id: String,
    /// Persona name (the player's Steam display name).
    #[serde(rename = "persona", alias = "personaname")]
    pub persona: String,
    /// Full avatar URL (may be empty).
    #[serde(rename = "avatar_url", alias = "avatarfull")]
    pub avatar_url: String,
    /// ISO country code (e.g. "FR"); defaults to empty when the profile has
    /// no country set (evxl omits the key entirely in that case).
    #[serde(rename = "country", alias = "loccountrycode", default)]
    pub country: String,
}

/// Where a play record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaySource {
    /// Parsed from a KovaaK's stats CSV on disk.
    Csv,
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
    /// Ingest provenance; currently always [`PlaySource::Csv`].
    pub source: PlaySource,
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
        let pasu = &p
            .categories
            .iter()
            .find(|(n, _)| n == "Tracking")
            .unwrap()
            .1
            .scenarios
            .iter()
            .find(|(n, _)| n == "VT Pasu Novice S5")
            .unwrap()
            .1;
        assert_eq!(pasu.score, 128161.0);
        assert_eq!(pasu.leaderboard_id, 98059);
        assert_eq!(pasu.scenario_rank, 4);
    }

    /// The live API emits explicit `null` rank/id/threshold fields for
    /// never-played scenarios inside an otherwise-played benchmark (observed
    /// live 2026-09-02, e.g. benchmark 227 → "SYV Altered POV Easy Slow"):
    /// `"score": 0, "leaderboard_rank": null, "scenario_rank": 0`. The whole
    /// payload must still decode.
    #[test]
    fn null_rank_fields_for_unplayed_scenarios_decode_as_zero() {
        const NULL_RANK_SAMPLE: &str = r#"{
            "benchmark_progress": 110000,
            "overall_rank": 4,
            "categories": {
                "Vertical": {
                    "benchmark_progress": 20000,
                    "category_rank": 3,
                    "rank_maxes": [6000, 12000, 18000, 24000, 30000],
                    "scenarios": {
                        "SYV Altered POV Easy Slow": {
                            "score": 0,
                            "leaderboard_rank": null,
                            "scenario_rank": 0,
                            "rank_maxes": [3500, 4500, 5500, 6000, 6300],
                            "leaderboard_id": 47632
                        },
                        "SYV Altered POV Slow": {
                            "score": 484800,
                            "leaderboard_rank": null,
                            "scenario_rank": 5,
                            "rank_maxes": null,
                            "leaderboard_id": null
                        }
                    }
                }
            }
        }"#;
        let p: BenchmarkProgress =
            serde_json::from_str(NULL_RANK_SAMPLE).expect("null ranks must decode");
        let unplayed = &p
            .categories
            .iter()
            .find(|(n, _)| n == "Vertical")
            .unwrap()
            .1
            .scenarios
            .iter()
            .find(|(n, _)| n == "SYV Altered POV Easy Slow")
            .unwrap()
            .1;
        assert_eq!(unplayed.score, 0.0);
        assert_eq!(unplayed.leaderboard_rank, 0);
        assert_eq!(unplayed.scenario_rank, 0);
        let partially_null = &p
            .categories
            .iter()
            .find(|(n, _)| n == "Vertical")
            .unwrap()
            .1
            .scenarios
            .iter()
            .find(|(n, _)| n == "SYV Altered POV Slow")
            .unwrap()
            .1;
        assert_eq!(partially_null.leaderboard_rank, 0);
        assert!(partially_null.rank_maxes.is_empty());
        assert_eq!(partially_null.leaderboard_id, 0);
    }

    /// REGRESSION: profiles with no country set omit the key entirely —
    /// connect must not fail on them.
    #[test]
    fn profile_without_country_decodes_to_empty() {
        let raw = r#"{
            "steamid": "76561199000000001",
            "personaname": "NoCountry",
            "avatarfull": "https://avatars.steamstatic.com/39bd99b50b945f092592f3671b52dabf877372bc_full.jpg"
        }"#;
        let p: PlayerProfile = serde_json::from_str(raw).expect("missing country must default");
        assert_eq!(p.steam_id, "76561199000000001");
        assert_eq!(p.persona, "NoCountry");
        assert_eq!(p.country, "");
    }
    /// REGRESSION (deep sweep 2026-09-03): benchmarks 2108/2487 report
    /// `"score": "3750.00"` as a string. That must decode instead of killing
    /// the whole benchmark payload.
    #[test]
    fn string_encoded_scores_decode_as_floats() {
        const STRING_SCORE_SAMPLE: &str = r#"{
            "benchmark_progress": 100000,
            "overall_rank": 3,
            "categories": {
                "Clicking": {
                    "benchmark_progress": 50000,
                    "category_rank": 3,
                    "rank_maxes": [10000, 20000, 30000, 40000],
                    "scenarios": {
                        "Stringy Scenario": {
                            "score": "3750.00",
                            "leaderboard_rank": 12,
                            "scenario_rank": 2,
                            "rank_maxes": ["1000.5", 2000, 3000, 4000],
                            "leaderboard_id": 2108
                        }
                    }
                }
            }
        }"#;
        let p: BenchmarkProgress =
            serde_json::from_str(STRING_SCORE_SAMPLE).expect("string scores must decode");
        let scen = &p
            .categories
            .iter()
            .find(|(n, _)| n == "Clicking")
            .unwrap()
            .1
            .scenarios
            .iter()
            .find(|(n, _)| n == "Stringy Scenario")
            .unwrap()
            .1;
        assert_eq!(scen.score, 3750.0);
        assert_eq!(scen.rank_maxes, vec![1000.5, 2000.0, 3000.0, 4000.0]);
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
            source: PlaySource::Csv,
        };
        let json = serde_json::to_string(&rec).expect("serialize");
        let back: PlayRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, rec);
        assert!(json.contains("csv"));
    }
}
