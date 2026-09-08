//! Benchmark-type classification: map each benchmark to evxl-style filter
//! types (Clicking, Tracking, Switching, …) from the registry's free-text
//! category/subcategory names.
//!
//! Why keyword rules: registry category/subcategory strings are author-written
//! and wildly inconsistent ("Clicking", "CLICKING", "Click", "Static Clicking",
//! "Click Timing", "动态点击" …). A keyword table over the lowercased
//! concatenation handles the variance; the table is validated against the full
//! embedded registry so every benchmark-classification decision is grounded.

use crate::registry::Registry;

/// The evxl-style benchmark types, in the order the UI shows the filter tabs.
pub const BENCHMARK_TYPES: &[&str] = &[
    "Mixed",
    "Clicking",
    "Tracking",
    "Switching",
    "Micro",
    "Static",
    "Dynamic",
    "Smoothness",
    "Precise",
    "Reactive",
    "Speed",
    "Control",
    "Evasive",
    "Flick",
    "Other",
];

/// Classify one (category, subcategory) pair into a single type.
fn classify_pair(category: &str, subcategory: &str) -> &'static str {
    // Order matters: first match wins. Specific families before generic ones.
    let c = category.to_lowercase();
    let s = subcategory.to_lowercase();
    let t = format!("{c} {s}");

    // Mixed / multiform first — benchmarks that blend families.
    if re_contains(
        &t,
        &[
            "mixed",
            "multiform",
            "hybrid",
            "hyb",
            "blend",
            "assorted",
            "varied",
            "diverse",
            "triad",
            "gauntlet",
            "fun",
        ],
    ) {
        return "Mixed";
    }
    // Micro before tracking/clicking (micro variants are their own tab).
    if re_contains(&s, &["micro", "tiny", "small", "fingertip", "fing"]) {
        return "Micro";
    }
    // Speed before tracking (speed subcats inside switching/tracking).
    if re_contains(&s, &["speed", "fast"]) && !s.contains("smooth") {
        return "Speed";
    }
    // Switching family.
    if re_contains(
        &t,
        &[
            "switch",
            "target switch",
            "ts",
            "transfer",
            "dodge",
            "evasive",
            "anti move",
            "anti m",
        ],
    ) {
        return "Switching";
    }
    // Evasive movement.
    if re_contains(
        &t,
        &[
            "strafe",
            "evasive",
            "dodge",
            "movement",
            "ground",
            "air",
            "bounce",
            "vertical",
            "horizontal",
            "parabola",
        ],
    ) {
        return "Evasive";
    }
    // Tracking subfamilies — order: reactive, smooth, precise, control, generic.
    if re_contains(
        &t,
        &[
            "reactiv", "react", "rxn", "reaction", "reflex", "reading", "read",
        ],
    ) {
        return "Reactive";
    }
    if re_contains(
        &t,
        &["smooth", "smo", "fluid", "flow", "raw smooth", "raw ctrl"],
    ) {
        return "Smoothness";
    }
    if re_contains(&t, &["precis", "prec", "precision"]) {
        return "Precise";
    }
    // Static / Dynamic aim styles (evxl tabs) — subcat-level.
    if re_contains(&s, &["static"]) {
        return "Static";
    }
    if re_contains(&s, &["dynamic"]) {
        return "Dynamic";
    }
    if re_contains(
        &t,
        &[
            "control",
            "ctrl",
            "stability",
            "steady",
            "centering",
            "holding",
            "hold",
        ],
    ) {
        return "Control";
    }
    if re_contains(&t, &["track", "traking"]) {
        return "Tracking";
    }
    // Clicking family.
    if re_contains(&t, &["click", "cps", "timing"]) {
        return "Clicking";
    }
    // Flick / technique.
    if re_contains(&t, &["flick", "tech"]) {
        return "Flick";
    }
    // Weapon / non-aim.
    if re_contains(&t, &["smg", "shotgun", "weapon", "no aim"]) {
        return "Other";
    }
    // Whole-category fallbacks for single-word categories.
    if re_contains(&c, &["track"]) {
        return "Tracking";
    }
    if re_contains(&c, &["click"]) {
        return "Clicking";
    }
    if re_contains(&c, &["switch"]) {
        return "Switching";
    }
    "Other"
}

fn re_contains(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// The set of types for one benchmark: every distinct classification across its
/// categories' subcategories. A benchmark can span multiple types (evxl shows
/// benchmark rows under each matching filter tab).
pub fn benchmark_types(registry: &Registry, benchmark_id: i64) -> Vec<&'static str> {
    let Some((bench, _)) = registry.by_id(benchmark_id as u64) else {
        return vec![];
    };
    benchmark_types_for_def(bench)
}

/// Same classification from a benchmark definition the caller already holds.
pub fn benchmark_types_for_def(bench: &crate::types::BenchmarkDef) -> Vec<&'static str> {
    let mut out = Vec::new();
    for difficulty in &bench.difficulties {
        for cat in &difficulty.categories {
            let Some(obj) = cat.as_object() else { continue };
            let cat_name = obj
                .get("categoryName")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let subs: Vec<String> = obj
                .get("subcategories")
                .and_then(|s| s.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|s| {
                            s.get("subcategoryName")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();
            let sub_refs: Vec<&str> = subs.iter().map(|s| s.as_str()).collect();
            // One type per category (evxl groups by category, not per subcat).
            let ty = classify_category_of_benchmark(cat_name, &bench.name, &sub_refs);
            if !out.contains(&ty) {
                out.push(ty);
            }
        }
    }
    if out.is_empty() {
        out.push("Other");
    }
    // Order by the canonical tab order for stable UI display.
    // Family tabs from category names: a benchmark with Tracking/Precise also
    // belongs under the Tracking tab (evxl shows benchmarks under family AND
    // subfamily tabs).
    let mut categories = std::collections::HashSet::new();
    for difficulty in &bench.difficulties {
        for cat in &difficulty.categories {
            if let Some(obj) = cat.as_object() {
                if let Some(n) = obj.get("categoryName").and_then(|v| v.as_str()) {
                    categories.insert(n.to_lowercase());
                }
            }
        }
    }
    if categories.iter().any(|c| c.contains("track")) && !out.contains(&"Tracking") {
        out.push("Tracking");
    }
    if categories.iter().any(|c| c.contains("click")) && !out.contains(&"Clicking") {
        out.push("Clicking");
    }
    if categories.iter().any(|c| c.contains("switch")) && !out.contains(&"Switching") {
        out.push("Switching");
    }

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

/// Family tabs: matching is "features this family" (union semantics, evxl shows
/// any benchmark containing the family). All other tabs are STYLE tabs: matching is
/// "the benchmark is purely this style" — every category must classify to the style
/// (evxl's Static tab shows only benchmarks consisting of static scenarios).
pub fn is_family_type(ty: &str) -> bool {
    matches!(ty, "Mixed" | "Clicking" | "Tracking" | "Switching")
}

/// The pure style of a benchmark: Some(T) when every category across all
/// difficulties classifies to the same style T (style tabs only match these).
/// Benchmarks spanning several styles return None (they only match family tabs).
pub fn pure_style(bench: &crate::types::BenchmarkDef) -> Option<&'static str> {
    let mut style: Option<&'static str> = None;
    for difficulty in &bench.difficulties {
        for cat in &difficulty.categories {
            let Some(obj) = cat.as_object() else { continue };
            let cat_name = obj
                .get("categoryName")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let subs: Vec<String> = obj
                .get("subcategories")
                .and_then(|s| s.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|s| {
                            s.get("subcategoryName")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();
            let sub_refs: Vec<&str> = subs.iter().map(|s| s.as_str()).collect();
            let ty = classify_category_of_benchmark(cat_name, &bench.name, &sub_refs);
            // "Other" is the catch-all, not a real style: never a pure style.
            if ty == "Other" {
                return None;
            }
            match style {
                None => style = Some(ty),
                Some(prev) if prev == ty => {}
                Some(_) => return None,
            }
        }
    }
    style
}

/// Classify a top-level category name (the style carrier for pure benchmarks:
/// "Static", "Static Clicking", "Dynamic", "Micro", "SpeedTS", ...). Style words
/// win over family words here: "Static Clicking" is Static, not Clicking.
fn classify_category(category: &str) -> &'static str {
    let c = category.to_lowercase();
    if re_contains(&c, &["static"]) {
        return "Static";
    }
    if re_contains(&c, &["dynamic"]) {
        return "Dynamic";
    }
    if re_contains(&c, &["micro", "tiny", "fingertip"]) {
        return "Micro";
    }
    if re_contains(&c, &["speed", "fast"]) && !c.contains("smooth") {
        return "Speed";
    }
    classify_pair(category, "")
}

/// Category names are author-written and often junk ("", "<:", "Category",
/// "cs/val", or the benchmark's own name repeated). Those carry no type signal.
fn category_name_is_junk(category: &str, benchmark_name: &str) -> bool {
    let c = category.trim();
    let l = c.to_lowercase();
    if l.len() < 2 {
        return true;
    }
    if l == "category" || l == "specific" {
        return true;
    }
    if l.contains('<') || l.contains('/') {
        return true;
    }
    !benchmark_name.is_empty() && c.eq_ignore_ascii_case(benchmark_name.trim())
}

/// Famous pure-static scenario families (gridshot-style clicking bots).
const STATIC_SCENARIO_KEYWORDS: &[&str] = &[
    "gridshot",
    "frenzy",
    "6 sphere",
    "six sphere",
    "popping",
    "spheric",
    "switchback",
];

/// Full per-category type resolution for a benchmark's category: categoryName
/// first (when it carries signal), then a majority vote over subcategories (in
/// both the category and benchmark-name context), then the benchmark name, then
/// classic static scenario names. This mirrors how evxl's own grouping treats
/// the benchmark name as the type carrier when categories are unnamed.
fn classify_category_of_benchmark(
    category: &str,
    benchmark_name: &str,
    subcategories: &[&str],
) -> &'static str {
    if !category_name_is_junk(category, benchmark_name) {
        let t = classify_category(category);
        if t != "Other" {
            return t;
        }
    }
    // Majority vote over subcategories. Try the benchmark name as context first
    // (unnamed categories), then the raw category name.
    let mut votes: Vec<&'static str> = Vec::new();
    for context in [benchmark_name, category] {
        for sub in subcategories {
            let v = classify_pair(context, sub);
            if v != "Other" {
                votes.push(v);
            }
        }
    }
    if !votes.is_empty() {
        let mut counts = std::collections::BTreeMap::new();
        for v in &votes {
            *counts.entry(v).or_insert(0) += 1;
        }
        let best = counts
            .iter()
            .max_by_key(|(ty, n)| (**n, std::cmp::Reverse(**ty)))
            .map(|(ty, _)| **ty);
        let best_count = counts.values().copied().max().unwrap_or(0);
        // A strict majority of subcategories, or a unanimous small vote.
        if let Some(v) = best {
            if best_count * 2 > votes.len()
                || (best_count == votes.len() && subcategories.len() <= 2)
            {
                return v;
            }
        }
    }
    let name_type = classify_category(benchmark_name);
    if name_type != "Other" {
        return name_type;
    }
    let joined = format!("{} {}", benchmark_name, subcategories.join(" ")).to_lowercase();
    if re_contains(&joined, STATIC_SCENARIO_KEYWORDS) {
        return "Static";
    }
    "Other"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Registry;

    #[test]
    fn every_played_registry_benchmark_classifies() {
        // Ground truth over the FULL embedded registry: every benchmark gets at
        // least one type, every type is a known tab label.
        let registry = Registry;
        for bench in registry.all() {
            let types = benchmark_types_for_def(bench);
            assert!(!types.is_empty(), "{} has no types", bench.name);
            for ty in &types {
                assert!(
                    BENCHMARK_TYPES.contains(ty),
                    "{} produced unknown type {ty}",
                    bench.name
                );
            }
        }
    }

    #[test]
    fn known_benchmarks_map_to_expected_primary_types() {
        let registry = Registry;
        // Voltaic S5: clicking/tracking/switching families.
        let vt5 = registry.by_id(460).expect("VT S5").0;
        let types = benchmark_types_for_def(vt5);
        assert!(types.contains(&"Clicking"), "{types:?}");
        assert!(types.contains(&"Tracking"), "{types:?}");
        assert!(types.contains(&"Switching"), "{types:?}");

        // Avasive S2 (evasive strafing) — Evasive or Switching, not Static.
        let avasive = registry.by_id(2843).expect("Avasive S2").0;
        let types = benchmark_types_for_def(avasive);
        assert!(
            types.contains(&"Evasive") || types.contains(&"Switching"),
            "{types:?}"
        );
    }

    #[test]
    fn pure_style_matches_evxl_tab_semantics() {
        let registry = Registry;
        // Pure static benchmarks classify cleanly to Static.
        let ca = registry
            .all()
            .iter()
            .find(|b| b.name.contains("cA Static"))
            .expect("cA Static");
        assert_eq!(pure_style(ca), Some("Static"));
        let setsunai = registry
            .all()
            .iter()
            .find(|b| b.name.contains("Setsunai"))
            .expect("Setsunai");
        assert_eq!(pure_style(setsunai), Some("Static"));
        // Multi-family benchmarks are not pure (they only match family tabs).
        let vt5 = registry.by_id(460).expect("VT S5").0;
        assert_eq!(pure_style(vt5), None);
        // Pure tracking benchmark.
        let precise = registry
            .all()
            .iter()
            .find(|b| b.name.contains("Aimerz+ Precise"))
            .expect("Aimerz+ Precise");
        assert_eq!(pure_style(precise), Some("Precise"));
    }

    #[test]
    fn unnamed_categories_fall_back_to_benchmark_name() {
        let registry = Registry;
        // "Deadman's Static Benchmarks S1" has an EMPTY categoryName; the type
        // lives in the benchmark name.
        let deadman = registry
            .all()
            .iter()
            .find(|b| b.name.contains("Deadman"))
            .expect("Deadman's");
        assert_eq!(pure_style(deadman), Some("Static"));
        // Junk category names ("Category", "<:", "cs/val") fall back to subcats
        // / benchmark name. Mastering Gridshot is pure static clicking.
        let grid = registry
            .all()
            .iter()
            .find(|b| b.name.contains("Mastering Gridshot"))
            .expect("Mastering Gridshot");
        assert_eq!(pure_style(grid), Some("Static"));
        // 350fs: junk categories "cs/val", subcats click/switch/track -> multi.
        let fs = registry
            .all()
            .iter()
            .find(|b| b.name == "350fs")
            .expect("350fs");
        assert_eq!(pure_style(fs), None);
    }

    #[test]
    fn other_is_never_a_pure_style() {
        // "Other" is the catch-all, not a style: style purity always resolves
        // to a concrete type or None.
        let registry = Registry;
        let n = registry
            .all()
            .iter()
            .filter(|b| pure_style(b) == Some("Other"))
            .count();
        assert_eq!(n, 0);
    }

    #[test]
    fn every_style_tab_has_at_least_one_pure_benchmark() {
        let registry = Registry;
        for ty in BENCHMARK_TYPES {
            if is_family_type(ty) || *ty == "Other" {
                continue;
            }
            let n = registry
                .all()
                .iter()
                .filter(|b| pure_style(b) == Some(*ty))
                .count();
            assert!(n > 0, "style tab {ty} has no pure benchmark");
        }
    }

    #[test]
    fn classification_distribution_is_not_degenerate() {
        // Guard against an over-eager rule: no single type may swallow > 60%
        // of registry benchmarks, and every tab must match something.
        let registry = Registry;
        let mut counts = std::collections::HashMap::new();
        let mut total = 0usize;
        for bench in registry.visible() {
            total += 1;
            for ty in benchmark_types_for_def(bench) {
                *counts.entry(ty).or_insert(0) += 1;
            }
        }
        assert!(total > 100, "registry sanity: {total} benchmarks");
        for (ty, n) in &counts {
            assert!(
                *n as f64 / total as f64 <= 0.60,
                "type {ty} swallowed {n}/{total} benchmarks"
            );
        }
        for ty in BENCHMARK_TYPES {
            assert!(counts.contains_key(*ty), "tab {ty} matches nothing");
        }
    }
}
