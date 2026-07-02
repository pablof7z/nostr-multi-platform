use super::*;
use crate::stable_hash::stable_hash64;
use std::collections::{BTreeMap, BTreeSet};

/// Deterministic 64-char hex pubkey/event-id fixture from a single byte.
fn hex(byte: &str) -> String {
    byte.repeat(32)
}

// ─── matches_event (ADR-0042 §5.1 store-admission predicate) ─────────────

#[test]
fn matches_event_default_shape_is_wildcard() {
    // An all-default shape is the `{}` REQ filter: matches everything.
    let shape = InterestShape::default();
    assert!(shape.matches_event(&hex("aa"), 1, 100, &[]));
    assert!(shape.matches_event(&hex("bb"), 30023, 0, &[vec!["t".into(), "x".into()]]));
}

#[test]
fn matches_event_author_and_kind_and() {
    let mut shape = InterestShape::default();
    shape.authors.insert(hex("aa"));
    shape.kinds.insert(1);

    // Both dimensions satisfied.
    assert!(shape.matches_event(&hex("aa"), 1, 100, &[]));
    // Wrong author.
    assert!(!shape.matches_event(&hex("bb"), 1, 100, &[]));
    // Wrong kind.
    assert!(!shape.matches_event(&hex("aa"), 6, 100, &[]));
}

#[test]
fn matches_event_hashtag_or_within_dimension() {
    let mut shape = InterestShape::default();
    shape.kinds.insert(1);
    shape.tags.insert(
        "t".to_string(),
        ["nostr".into(), "bitcoin".into()].into_iter().collect(),
    );

    // Event carrying one of the wanted #t values matches.
    assert!(shape.matches_event(&hex("aa"), 1, 100, &[vec!["t".into(), "bitcoin".into()]]));
    // Event with a #t value NOT in the set does not match.
    assert!(!shape.matches_event(&hex("aa"), 1, 100, &[vec!["t".into(), "ethereum".into()]]));
    // Event with no #t tag at all does not match a required #t dimension.
    assert!(!shape.matches_event(&hex("aa"), 1, 100, &[vec!["e".into(), hex("cc")]]));
}

#[test]
fn matches_event_since_until_bounds() {
    let mut shape = InterestShape::default();
    shape.since = Some(100);
    shape.until = Some(200);

    assert!(shape.matches_event(&hex("aa"), 1, 150, &[]));
    assert!(shape.matches_event(&hex("aa"), 1, 100, &[])); // inclusive lower
    assert!(shape.matches_event(&hex("aa"), 1, 200, &[])); // inclusive upper
    assert!(!shape.matches_event(&hex("aa"), 1, 99, &[]));
    assert!(!shape.matches_event(&hex("aa"), 1, 201, &[]));
}

#[test]
fn matches_event_with_id_honours_ids_dimension() {
    let mut shape = InterestShape::default();
    shape.event_ids.insert(hex("11"));

    assert!(shape.matches_event_with_id(&hex("11"), &hex("aa"), 1, 100, &[]));
    assert!(!shape.matches_event_with_id(&hex("22"), &hex("aa"), 1, 100, &[]));
    // `matches_event` (no id dimension) ignores event_ids — the wire-tag
    // path is what thread feeds actually use.
    assert!(shape.matches_event(&hex("22"), 1, 100, &[]));
}

#[test]
fn matches_event_ignores_client_side_only_fields() {
    // `limit` is a client-side cap, never a relay match predicate.
    let mut shape = InterestShape::default();
    shape.kinds.insert(1);
    shape.limit = Some(1);
    // Two events both match despite limit=1 — limit must not gate admission.
    assert!(shape.matches_event(&hex("aa"), 1, 100, &[]));
    assert!(shape.matches_event(&hex("bb"), 1, 101, &[]));
}

#[test]
fn timeline_for_carries_caller_kinds_verbatim() {
    let authors: BTreeSet<Pubkey> = [hex("aa"), hex("bb")].into_iter().collect();
    // V-68: pass an ARBITRARY, non-social kind set to prove the
    // constructor is kind-agnostic — it must not inject {1, 6} or any
    // other app default. A long-form host would declare {30023}.
    let caller_kinds: BTreeSet<u32> = [30023u32, 9999u32].into_iter().collect();
    let shape = InterestShape::timeline_for(authors.clone(), caller_kinds.clone());

    // Authors carried through verbatim.
    assert_eq!(shape.authors, authors);
    // Kinds are exactly what the caller supplied — no substrate policy.
    assert_eq!(shape.kinds, caller_kinds);
    // Every other dimension stays at its wildcard / default.
    assert!(shape.tags.is_empty());
    assert!(shape.event_ids.is_empty());
    assert!(shape.addresses.is_empty());
    assert_eq!(shape.since, None);
    assert_eq!(shape.until, None);
    assert_eq!(shape.limit, None);
    assert_eq!(shape.relay_pin, None);
}

#[test]
fn profile_for_has_exactly_one_author_and_indexer_kinds() {
    let pubkey = hex("cc");
    let shape = InterestShape::profile_for(pubkey.clone());

    // Exactly one author — the requested pubkey.
    assert_eq!(shape.authors.len(), 1);
    assert!(shape.authors.contains(&pubkey));
    // kind:0 profile + kind:3 contacts + kind:10002 NIP-65 relay list.
    assert_eq!(
        shape.kinds,
        [0u32, 3u32, 10002u32]
            .into_iter()
            .collect::<BTreeSet<u32>>()
    );
    // One-shot profile fetch caps at 3 replaceable events.
    assert_eq!(shape.limit, Some(3));
    // No tags / pointers / time bounds / routing pin.
    assert!(shape.tags.is_empty());
    assert!(shape.event_ids.is_empty());
    assert!(shape.addresses.is_empty());
    assert_eq!(shape.since, None);
    assert_eq!(shape.until, None);
    assert_eq!(shape.relay_pin, None);
}

#[test]
fn naddr_coord_equality_depends_on_all_three_fields() {
    let base = NaddrCoord {
        pubkey: hex("aa"),
        kind: 30023,
        d_tag: "my-article".to_string(),
    };
    // Identical triple → equal.
    let same = NaddrCoord {
        pubkey: hex("aa"),
        kind: 30023,
        d_tag: "my-article".to_string(),
    };
    assert_eq!(base, same);

    // Differing pubkey → not equal.
    let other_pubkey = NaddrCoord {
        pubkey: hex("bb"),
        ..base.clone()
    };
    assert_ne!(base, other_pubkey);

    // Differing kind → not equal.
    let other_kind = NaddrCoord {
        kind: 30024,
        ..base.clone()
    };
    assert_ne!(base, other_kind);

    // Differing d_tag → not equal.
    let other_d_tag = NaddrCoord {
        d_tag: "another-article".to_string(),
        ..base.clone()
    };
    assert_ne!(base, other_d_tag);
}

#[test]
fn naddr_coord_dedup_in_btreeset_keys_on_full_triple() {
    // Two coords that share kind+d_tag but differ on pubkey must NOT
    // collapse — the D8 composite index relies on the full triple as key.
    let mut set: BTreeSet<NaddrCoord> = BTreeSet::new();
    set.insert(NaddrCoord {
        pubkey: hex("aa"),
        kind: 30023,
        d_tag: "post".to_string(),
    });
    set.insert(NaddrCoord {
        pubkey: hex("bb"),
        kind: 30023,
        d_tag: "post".to_string(),
    });
    // Re-inserting an exact duplicate is a no-op.
    set.insert(NaddrCoord {
        pubkey: hex("aa"),
        kind: 30023,
        d_tag: "post".to_string(),
    });
    assert_eq!(set.len(), 2);
}

#[test]
fn logical_interest_default_is_one_shot_global_empty() {
    let interest = LogicalInterest::default();

    // Default lifecycle is OneShot (CLOSE on EOSE), not a tailing sub.
    assert_eq!(interest.lifecycle, InterestLifecycle::OneShot);
    // Default scope is Global — no account context.
    assert_eq!(interest.scope, InterestScope::Global);
    // Registry-assigned id starts at the sentinel 0.
    assert_eq!(interest.id, InterestId(0));
    // No hints, and the shape is the empty wildcard default.
    assert!(interest.hints.is_empty());
    assert_eq!(interest.shape, InterestShape::default());
}

#[test]
fn interest_shape_multi_field_round_trips_field_contents() {
    // Build a richly-populated shape and verify each dimension lands.
    let mut tags: BTreeMap<TagKey, BTreeSet<String>> = BTreeMap::new();
    tags.insert(
        "t".to_string(),
        ["nostr".to_string(), "rust".to_string()]
            .into_iter()
            .collect(),
    );

    let addr = NaddrCoord {
        pubkey: hex("dd"),
        kind: 30023,
        d_tag: "long-form".to_string(),
    };

    let shape = InterestShape {
        authors: [hex("aa")].into_iter().collect(),
        kinds: [1u32, 7u32].into_iter().collect(),
        tags: tags.clone(),
        since: Some(1_700_000_000),
        until: Some(1_700_086_400),
        limit: Some(50),
        search: Some("nostr rust".to_string()),
        event_ids: [hex("ee")].into_iter().collect(),
        addresses: [addr.clone()].into_iter().collect(),
        relay_pin: Some("wss://relay.example.com".to_string()),
        p_tag_routing: PTagRouting::Nip65ReadRelays,
    };

    assert_eq!(shape.authors.len(), 1);
    assert!(shape.authors.contains(&hex("aa")));
    assert_eq!(
        shape.kinds,
        [1u32, 7u32].into_iter().collect::<BTreeSet<u32>>()
    );
    assert_eq!(shape.tags.get("t").map(|v| v.len()), Some(2),);
    assert!(shape.tags["t"].contains("nostr"));
    assert!(shape.tags["t"].contains("rust"));
    assert_eq!(shape.since, Some(1_700_000_000));
    assert_eq!(shape.until, Some(1_700_086_400));
    assert_eq!(shape.limit, Some(50));
    assert_eq!(shape.search.as_deref(), Some("nostr rust"));
    assert!(shape.event_ids.contains(&hex("ee")));
    assert!(shape.addresses.contains(&addr));
    assert_eq!(shape.relay_pin.as_deref(), Some("wss://relay.example.com"));
}

#[test]
fn from_filter_json_maps_every_nip01_field() {
    let json = format!(
        r##"{{"kinds":[1,6],"authors":["{}"],"ids":["{}"],"#e":["{}"],"#t":["bitcoin","nostr"],"since":100,"until":200,"limit":50,"search":"  nostr rust  "}}"##,
        hex("aa"),
        hex("bb"),
        hex("cc"),
    );
    let shape = InterestShape::from_filter_json(&json).expect("valid object");

    assert_eq!(shape.kinds, [1u32, 6u32].into_iter().collect());
    assert_eq!(shape.authors, [hex("aa")].into_iter().collect());
    assert_eq!(shape.event_ids, [hex("bb")].into_iter().collect());
    assert_eq!(
        shape
            .tags
            .get("e")
            .map(|s| s.iter().cloned().collect::<Vec<_>>()),
        Some(vec![hex("cc")])
    );
    assert_eq!(shape.tags.get("t").map(|s| s.len()), Some(2));
    assert!(shape.tags["t"].contains("bitcoin"));
    assert!(shape.tags["t"].contains("nostr"));
    assert_eq!(shape.since, Some(100));
    assert_eq!(shape.until, Some(200));
    assert_eq!(shape.limit, Some(50));
    assert_eq!(shape.search.as_deref(), Some("nostr rust"));
    // Client-side-only fields are never set by the parser.
    assert!(shape.addresses.is_empty());
    assert_eq!(shape.relay_pin, None);
}

#[test]
fn from_filter_json_bounds_search_query() {
    let long = "x".repeat(MAX_SEARCH_QUERY_CHARS + 50);
    let json = format!(r#"{{"search":"{long}"}}"#);
    let shape = InterestShape::from_filter_json(&json).expect("valid object");
    assert_eq!(
        shape.search.as_ref().map(|s| s.chars().count()),
        Some(MAX_SEARCH_QUERY_CHARS)
    );

    let empty = InterestShape::from_filter_json(r#"{"search":"   "}"#).expect("valid object");
    assert_eq!(empty.search, None);
}

#[test]
fn from_filter_json_is_order_independent_for_dedup() {
    // The whole point of the InterestShape-hash dedup: two filter strings
    // that differ only in JSON key order AND array element order must parse
    // to byte-identical shapes so the registry collapses them to one slot.
    let a = InterestShape::from_filter_json(r#"{"kinds":[1,6],"authors":["aa","bb"]}"#).unwrap();
    let b = InterestShape::from_filter_json(r#"{"authors":["bb","aa"],"kinds":[6,1]}"#).unwrap();
    assert_eq!(a, b, "key/element ordering must not affect the shape");
}

#[test]
fn from_filter_json_rejects_non_object() {
    assert!(InterestShape::from_filter_json("[]").is_none());
    assert!(InterestShape::from_filter_json("42").is_none());
    assert!(InterestShape::from_filter_json("not json").is_none());
    assert!(InterestShape::from_filter_json("\"a string\"").is_none());
}

#[test]
fn from_filter_json_tolerates_malformed_and_unknown_fields() {
    // Non-array kinds is skipped; unknown top-level key ignored; the valid
    // subset still lands. Multi-char tag keys (`#foo`) are not NIP-01 and
    // are dropped.
    let shape = InterestShape::from_filter_json(
        r##"{"kinds":"oops","authors":["aa"],"weird":true,"#foo":["x"]}"##,
    )
    .expect("still a valid object");
    assert!(shape.kinds.is_empty());
    assert_eq!(shape.authors, ["aa".to_string()].into_iter().collect());
    assert!(shape.tags.is_empty(), "multi-char tag key dropped");
}

#[test]
fn from_filter_json_empty_object_is_wildcard_default() {
    let shape = InterestShape::from_filter_json("{}").unwrap();
    assert_eq!(shape, InterestShape::default());
}

#[test]
fn interest_shape_hash_adds_search_only_when_present() {
    let default_hash = stable_hash64(&InterestShape::default());
    let blank_search = InterestShape::from_filter_json(r#"{"search":"   "}"#).unwrap();
    assert_eq!(stable_hash64(&blank_search), default_hash);

    let mut search = InterestShape::default();
    search.search = Some("nostr rust".to_string());
    assert_ne!(stable_hash64(&search), default_hash);

    let same_search = InterestShape {
        search: Some("nostr rust".to_string()),
        ..Default::default()
    };
    assert_eq!(stable_hash64(&search), stable_hash64(&same_search));
}

#[test]
fn interest_shape_equality_is_field_wise_and_deterministic() {
    // Two shapes built independently with the same field values must be
    // equal — the §3.4 plan-id stability contract depends on this.
    let kinds: BTreeSet<u32> = [30023u32].into_iter().collect();
    let a = InterestShape::timeline_for([hex("aa")].into_iter().collect(), kinds.clone());
    let b = InterestShape::timeline_for([hex("aa")].into_iter().collect(), kinds.clone());
    assert_eq!(a, b);

    // A different author set breaks equality.
    let c = InterestShape::timeline_for([hex("bb")].into_iter().collect(), kinds);
    assert_ne!(a, c);
}
