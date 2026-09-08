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
            let subs = obj.get("subcategories").and_then(|s| s.as_array());
            for sub in subs.into_iter().flatten() {
                let sub_name = sub
                    .get("subcategoryName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let ty = classify_pair(cat_name, sub_name);
                if !out.contains(&ty) {
                    out.push(ty);
                }
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
