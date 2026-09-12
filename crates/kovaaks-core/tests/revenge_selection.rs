#[test]
fn embedded_registry_has_revenge_selection() {
    let reg = kovaaks_core::registry::Registry;
    let (_, diff) = reg.by_id(2725).expect("REVENGE Main");
    let sel = diff
        .scenario_selection
        .as_ref()
        .expect("scenarioSelection parsed");
    assert!(sel.enabled);
    assert_eq!(sel.select_count, 24);
    assert_eq!(sel.base_rank_score_count, 9);
    assert_eq!(sel.full_pool_rank_score_count, Some(18));
    assert_eq!(sel.min_per_category, 8);
    assert_eq!(sel.min_per_subcategory, 4);
}
