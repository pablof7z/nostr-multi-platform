//! Cache-serve scope resolution assertions for the cache FTS integration gate.

use super::*;

#[test]
fn installed_scopes_are_surfaced_for_cache_serve() {
    run_both(|h| {
        install_all_scopes(h);
        let cache_scopes = h.store.cache_search_scopes();
        // All four crate scopes are cache-eligible (Both / CacheOnly), so the
        // cache-serve hook can resolve a search shape to the local index.
        let installed: BTreeSet<SearchScopeId> = cache_scopes.iter().map(|(s, _)| *s).collect();
        for label in [
            SCOPE_LABEL_PROFILES,
            SCOPE_LABEL_NOTES,
            SCOPE_LABEL_LONGFORM,
            GROUP_SEARCH_SCOPE_LABEL,
        ] {
            assert!(
                installed.contains(&SearchScopeId::from_label(label)),
                "scope {label} must be surfaced for cache-serve resolution"
            );
        }
        // The kinds reported per scope are the indexable (public) kinds only.
        for (scope, kinds) in &cache_scopes {
            assert!(
                !kinds.is_empty(),
                "scope {scope:?} reports its indexable kinds for shape intersection"
            );
            for k in kinds {
                assert!(
                    ![4u32, 13, 14, 15, 1059, 1060].contains(k),
                    "cache-serve scope kinds never include a private kind"
                );
            }
        }
    });
}
