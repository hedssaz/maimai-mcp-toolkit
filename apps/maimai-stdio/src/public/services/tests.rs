#[test]
fn composition_root_has_single_catalog_state_and_provider_open_sites() {
    let source = include_str!("../services.rs");
    let catalog_open = ["CatalogStore", "::load("].concat();
    let state_open = ["StateStore", "::open("].concat();
    let diving_fish = ["DivingFishClient", "::with_http_client("].concat();
    let napcat = ["NapCatClient", "::new("].concat();
    let public_scores = ["PlayerScoreService", "::diving_fish_only("].concat();
    assert_eq!(source.matches(&catalog_open).count(), 1);
    assert_eq!(source.matches(&state_open).count(), 1);
    assert_eq!(source.matches(&diving_fish).count(), 1);
    assert_eq!(source.matches(&napcat).count(), 1);
    assert_eq!(source.matches(&public_scores).count(), 1);
    assert!(!source.contains("PlayerScoreService::with_lxns"));
    assert!(source.matches("Arc::clone(&catalog)").count() >= 8);
    assert!(source.matches("state.clone()").count() >= 5);
}
