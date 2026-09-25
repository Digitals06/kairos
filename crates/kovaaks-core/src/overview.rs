//! Family fold (v0.2): one card per benchmark family from per-difficulty rows.
//!
//! The shown rank/metrics are those of the played difficulty with the
//! deepest tier in its own ladder; the card keeps the winner's kovaaks id
//! so click-through opens that detail page. Pure logic, no I/O: the view
//! layer derives per-row [`FamilyMeta`] from its registry and this module
//! owns the folding rule (difficulty order beats tier depth; ties broken
//! by depth fraction; unranked never beats ranked).

/// Per-row fold inputs the view layer derives from its registry/rank types.
#[derive(Debug, Clone)]
pub struct FoldRow {
    pub benchmark_id: i64,
    pub family_name: String,
    pub difficulty_name: String,
    pub rank_name: Option<String>,
    pub is_favorite: bool,
}

/// View-layer knowledge of one row's family: registry difficulty order,
/// tier depth fraction (0..1 within its difficulty's ladder), and the
/// difficulty's tier names (for the variant's tier strip).
#[derive(Debug, Clone, Default)]
pub struct FamilyMeta {
    pub order: isize,
    pub tier_depth: f64,
    pub tier_names: Vec<String>,
}

/// One folded variant row: the family member shown after the fold.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldVariant {
    pub benchmark_id: i64,
    pub difficulty_name: String,
    pub current_rank_index: i64,
    pub tier_names: Vec<String>,
}

/// Folded family card: fields the view layer copies back onto its card type.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldedFamily {
    /// The kept member (deepest tier row) — its benchmark_id opens detail.
    pub kept_benchmark_id: i64,
    pub difficulty_count: u32,
    pub variants: Vec<FoldVariant>,
}

/// Fold per-difficulty rows into one card per family.
///
/// `rows` and `meta` are indexed in parallel (meta[i] describes rows[i]);
/// `tier_index_of` maps a row's rank name onto its difficulty's tier vector
/// (-1 when unranked — the view layer knows its own registry case rules).
pub fn fold_families(rows: Vec<FoldRow>, meta: Vec<FamilyMeta>) -> Vec<FoldedFamily> {
    debug_assert_eq!(rows.len(), meta.len());
    if rows.is_empty() {
        return Vec::new();
    }
    // member counts per family
    let mut counts: std::collections::HashMap<&str, u32> = Default::default();
    for r in &rows {
        *counts.entry(r.family_name.as_str()).or_insert(0) += 1;
    }

    // difficulty order: hardest difficulty that has a RANKED row wins;
    // tier depth fraction breaks ties. Unranked (-1 order sentinel) never
    // beats a ranked easier difficulty.
    let strength = |(row, mt): (&FoldRow, &FamilyMeta)| -> (isize, f64) {
        if row.rank_name.is_none() {
            (-1, 0.0)
        } else {
            (mt.order, mt.tier_depth)
        }
    };

    let mut idx: Vec<usize> = (0..rows.len()).collect();
    idx.sort_by(|a, b| rows[*a].family_name.cmp(&rows[*b].family_name));

    let mut out = Vec::new();
    let mut i = 0;
    while i < idx.len() {
        let family = rows[idx[i]].family_name.clone();
        let start = i;
        while i < idx.len() && rows[idx[i]].family_name == family {
            i += 1;
        }
        let members: Vec<usize> = idx[start..i].to_vec();
        let mut members = members;
        members.sort_by(|a, b| {
            let (ga, fa) = strength((&rows[*a], &meta[*a]));
            let (gb, fb) = strength((&rows[*b], &meta[*b]));
            gb.cmp(&ga)
                .then(fb.partial_cmp(&fa).unwrap_or(std::cmp::Ordering::Equal))
        });
        let kept = members[0];
        let variants = members
            .iter()
            .map(|&k| {
                let mt = &meta[k];
                let current_rank_index = rows[k]
                    .rank_name
                    .as_ref()
                    .and_then(|name| mt.tier_names.iter().position(|n| n == name))
                    .map(|p| p as i64)
                    .unwrap_or(-1);
                FoldVariant {
                    benchmark_id: rows[k].benchmark_id,
                    difficulty_name: rows[k].difficulty_name.clone(),
                    current_rank_index,
                    tier_names: mt.tier_names.clone(),
                }
            })
            .collect();
        out.push(FoldedFamily {
            kept_benchmark_id: rows[kept].benchmark_id,
            difficulty_count: counts.get(family.as_str()).copied().unwrap_or(1),
            variants,
        });
    }
    // favorites pinned on top (the view layer sorts their final order, but
    // the fold preserves the input's alphabetical or favorite-first basis).
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(bid: i64, fam: &str, rank: Option<&str>, fav: bool) -> FoldRow {
        FoldRow {
            benchmark_id: bid,
            family_name: fam.to_string(),
            difficulty_name: format!("d{bid}"),
            rank_name: rank.map(String::from),
            is_favorite: fav,
        }
    }

    fn meta(order: isize, depth: f64, tiers: &[&str]) -> FamilyMeta {
        FamilyMeta {
            order,
            tier_depth: depth,
            tier_names: tiers.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn fold_keeps_deepest_tier_row() {
        let rows = vec![
            row(1, "Avasive S2", Some("Gold"), false),
            row(2, "Avasive S2", Some("Diamond"), false),
        ];
        let metas = vec![
            meta(0, 0.5, &["Gold", "Diamond"]),
            meta(1, 0.2, &["Iron", "Gold", "Diamond", "Legend"]),
        ];
        let out = fold_families(rows, metas);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kept_benchmark_id, 2, "harder difficulty wins");
        assert_eq!(out[0].difficulty_count, 2);
        assert_eq!(out[0].variants.len(), 2);
    }

    #[test]
    fn unranked_never_beats_ranked() {
        let rows = vec![row(1, "F", None, false), row(2, "F", Some("Bronze"), false)];
        let metas = vec![meta(1, 0.0, &[]), meta(0, 0.1, &["Bronze"])];
        let out = fold_families(rows, metas);
        assert_eq!(
            out[0].kept_benchmark_id, 2,
            "unranked harder difficulty loses"
        );
    }

    #[test]
    fn tie_breaks_on_tier_depth() {
        let rows = vec![
            row(1, "F", Some("Nova"), false),
            row(2, "F", Some("Nova"), false),
        ];
        let metas = vec![meta(0, 0.4, &[]), meta(0, 0.8, &[])];
        let out = fold_families(rows, metas);
        assert_eq!(out[0].kept_benchmark_id, 2, "same tier stronger depth wins");
    }

    #[test]
    fn families_stay_separate() {
        let rows = vec![
            row(1, "Avasive", Some("Gold"), false),
            row(2, "Voltaic", Some("Nova"), false),
        ];
        let metas = vec![meta(0, 0.9, &[]), meta(0, 0.1, &[])];
        let out = fold_families(rows, metas);
        assert_eq!(out.len(), 2);
    }
}
