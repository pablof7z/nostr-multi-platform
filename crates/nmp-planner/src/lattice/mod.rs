//! The filter-merge lattice: `merge()` implements Rules 1–10 from the compiler
//! design. Only shapes that pass all ten rules are merged; otherwise the
//! caller emits two distinct REQs.
//!
//! ## Module structure
//!
//! - `rules` — the 9 individual rule functions (pub(super)).
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.3
//! Doctrine: D8 (zero per-event allocs on the hot path after warmup).
//!
//! ## Rules summary
//! 1. `kinds` — equal or one wildcard; wildcard absorbs.
//! 2. `tags` — same key dimensions; per-dimension value union ≤ limit
//!    (the "h-tag coalesce" workhorse: when two shapes share a `relay_pin`,
//!    this is what collapses their per-room tag values into one REQ).
//! 3. `since` — `min(a, b)` iff both present or both absent; mixed = refuse.
//! 4. `until` — `max(a, b)` iff both present or both absent; mixed = refuse.
//! 5. `limit` — merge only if both absent.
//! 6. `lifecycle` — identical lifecycles only.
//! 7. `event_ids` — union, capped.
//! 8. `addresses` — union, capped; requires other fields mergeable per 1–7.
//! 9. `relay_pin` — host-relay-pin equality; `None` does NOT absorb `Some(_)`.
//!    Generic third-routing-lane contract for any protocol that requires
//!    addressing a single host relay.
//! 10. `p_tag_routing` — equality; NIP-17 DM-relay inbox routing must not
//!     merge with generic NIP-65 `#p` routing.

mod rules;

use crate::interest::{InterestLifecycle, InterestShape};
use rules::{
    rule1_kinds, rule2_tags, rule3_since, rule4_until, rule5_limit, rule6_lifecycle,
    rule7_event_ids, rule8_addresses, rule9_relay_pin,
};

/// Per-relay cap for merged value sets (tags, ids, addresses).
/// This mirrors the relay default of 1000 per filter.
const DEFAULT_VALUE_LIMIT: usize = 1000;

/// Outcome of attempting to merge two `InterestShape`s on a single relay.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeOutcome {
    /// Shapes were merged; the returned shape is the result.
    Merged(InterestShape),
    /// Shapes cannot be merged without changing semantics.
    Refused,
}

/// Merges two filter shapes into one.
///
/// Attempt to merge shape `b` into shape `a` on a given relay.
///
/// Returns `Merged(result)` iff all 9 rules pass; `Refused` otherwise.
/// Neither `a` nor `b` is modified on refusal.
///
/// # ⚠ Superset semantics
///
/// The merged shape is a **superset** of both inputs — it matches everything
/// either input matches, *plus* combinations neither input asked for (e.g.,
/// cross-products of author sets and tag sets). This is correct for
/// wire-coalescing (fewer REQs is better), but callers must not assume the
/// merged sub-shape is a tight filter. Store ingest applies author-gating
/// independently to filter over-delivered events.
///
/// Design: §3.3 Rules 1–9
#[must_use]
pub fn merge(
    a: &InterestShape,
    b: &InterestShape,
    lifecycle_a: &InterestLifecycle,
    lifecycle_b: &InterestLifecycle,
) -> MergeOutcome {
    // Rule 6 first — cheapest check, prune early.
    if !rule6_lifecycle(lifecycle_a, lifecycle_b) {
        return MergeOutcome::Refused;
    }

    // Rule 9 second — also cheap (Option equality), and a refusal here means
    // the two interests will definitely be sent to different relays. Pruning
    // before the more expensive set unions saves work on host-pinned views.
    if !rule9_relay_pin(a, b) {
        return MergeOutcome::Refused;
    }

    // Rule 10 — p-tag routing mode. This is not a wire filter field, but it
    // decides which relay set is used for Case C; keep merged shapes tied to
    // one routing policy.
    if a.p_tag_routing != b.p_tag_routing {
        return MergeOutcome::Refused;
    }

    // Rule 1 — kinds
    let Some(merged_kinds) = rule1_kinds(a, b) else {
        return MergeOutcome::Refused;
    };

    // Rule 2 — tag dimensions
    let Some(merged_tags) = rule2_tags(a, b, DEFAULT_VALUE_LIMIT) else {
        return MergeOutcome::Refused;
    };

    // Rule 3 — since
    let Some(merged_since) = rule3_since(a, b) else {
        return MergeOutcome::Refused;
    };

    // Rule 4 — until
    let Some(merged_until) = rule4_until(a, b) else {
        return MergeOutcome::Refused;
    };

    // Rule 5 — limit
    if !rule5_limit(a, b) {
        return MergeOutcome::Refused;
    }

    // Rule 7 — event_ids union
    let Some(merged_event_ids) = rule7_event_ids(a, b, DEFAULT_VALUE_LIMIT) else {
        return MergeOutcome::Refused;
    };

    // Rule 8 — addresses union (requires prior rules to have passed)
    let Some(merged_addresses) = rule8_addresses(a, b, DEFAULT_VALUE_LIMIT) else {
        return MergeOutcome::Refused;
    };

    MergeOutcome::Merged(InterestShape {
        authors: a.authors.union(&b.authors).cloned().collect(),
        kinds: merged_kinds,
        tags: merged_tags,
        since: merged_since,
        until: merged_until,
        limit: None, // Rule 5 guarantees both are None
        event_ids: merged_event_ids,
        addresses: merged_addresses,
        // Rule 9 guaranteed equality above; either side carries the result.
        relay_pin: a.relay_pin.clone(),
        // Rule 10 guaranteed equality above; either side carries the result.
        p_tag_routing: a.p_tag_routing,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interest::{InterestLifecycle, InterestShape, NaddrCoord};
    use std::collections::{BTreeMap, BTreeSet};

    fn tailing() -> InterestLifecycle {
        InterestLifecycle::Tailing
    }
    fn one_shot() -> InterestLifecycle {
        InterestLifecycle::OneShot
    }

    fn shape_with_kinds(kinds: &[u32]) -> InterestShape {
        InterestShape {
            kinds: kinds.iter().copied().collect(),
            ..Default::default()
        }
    }

    // ── Rule 1 — kinds ───────────────────────────────────────────────────────

    #[test]
    fn rule1_equal_kinds_merge() {
        let a = shape_with_kinds(&[1, 6]);
        let b = shape_with_kinds(&[1, 6]);
        let r = merge(&a, &b, &tailing(), &tailing());
        assert!(matches!(r, MergeOutcome::Merged(ref s) if s.kinds == a.kinds));
    }

    #[test]
    fn rule1_different_kinds_refuse() {
        let a = shape_with_kinds(&[1]);
        let b = shape_with_kinds(&[6]);
        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
    }

    #[test]
    fn rule1_wildcard_absorbs_specific() {
        // a is wildcard (empty), b is specific — result MUST be wildcard (empty),
        // NOT b.kinds. Returning b.kinds would narrow the merged subscription,
        // causing the relay to miss kinds that the wildcard side intended to match.
        let a = InterestShape::default(); // kinds = empty (wildcard)
        let b = shape_with_kinds(&[1, 6]);
        let r = merge(&a, &b, &tailing(), &tailing());
        assert!(
            matches!(r, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
            "wildcard ∪ {{1,6}} must be wildcard (empty set), not {{1,6}}"
        );
    }

    #[test]
    fn wildcard_unions_with_anything_stays_wildcard() {
        // Negative-direction: wildcard merged with ANY concrete set must stay wildcard.
        // This is the correctness test the T30 codex review flagged as missing.
        let wildcard = InterestShape::default(); // kinds = empty
        for concrete_kinds in [
            vec![1u32],
            vec![6],
            vec![1, 6],
            vec![0, 1, 3, 4, 5, 6, 7, 9, 10, 30023],
        ] {
            let concrete = shape_with_kinds(&concrete_kinds);
            let r_ab = merge(&wildcard, &concrete, &tailing(), &tailing());
            let r_ba = merge(&concrete, &wildcard, &tailing(), &tailing());
            assert!(
                matches!(r_ab, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
                "wildcard ∪ {:?} must be wildcard (a=wildcard)",
                concrete_kinds
            );
            assert!(
                matches!(r_ba, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
                "wildcard ∪ {:?} must be wildcard (b=wildcard)",
                concrete_kinds
            );
        }
        // wildcard ∪ wildcard = wildcard
        let r = merge(&wildcard, &wildcard, &tailing(), &tailing());
        assert!(
            matches!(r, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
            "wildcard ∪ wildcard must be wildcard"
        );
    }

    // ── Rule 2 — tags ────────────────────────────────────────────────────────

    #[test]
    fn rule2_same_tag_dimensions_merge() {
        let mut tags_a = BTreeMap::new();
        tags_a.insert(
            "t".to_string(),
            ["bitcoin".to_string()].into_iter().collect::<BTreeSet<_>>(),
        );
        let mut tags_b = BTreeMap::new();
        tags_b.insert(
            "t".to_string(),
            ["nostr".to_string()].into_iter().collect::<BTreeSet<_>>(),
        );
        let a = InterestShape {
            tags: tags_a,
            kinds: [1].into_iter().collect(),
            ..Default::default()
        };
        let b = InterestShape {
            tags: tags_b,
            kinds: [1].into_iter().collect(),
            ..Default::default()
        };
        let r = merge(&a, &b, &tailing(), &tailing());
        if let MergeOutcome::Merged(s) = r {
            let t = s.tags.get("t").unwrap();
            assert!(t.contains("bitcoin"));
            assert!(t.contains("nostr"));
        } else {
            panic!("expected Merged");
        }
    }

    #[test]
    fn rule2_different_tag_dimensions_refuse() {
        let mut tags_a = BTreeMap::new();
        tags_a.insert(
            "t".to_string(),
            ["bitcoin".to_string()].into_iter().collect::<BTreeSet<_>>(),
        );
        let tags_b = BTreeMap::new();
        let a = InterestShape {
            tags: tags_a,
            ..Default::default()
        };
        let b = InterestShape {
            tags: tags_b,
            ..Default::default()
        };
        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
    }

    // ── Rule 3 — since ───────────────────────────────────────────────────────

    #[test]
    fn rule3_both_since_take_min() {
        let a = InterestShape {
            kinds: [1].into_iter().collect(),
            since: Some(1000),
            ..Default::default()
        };
        let b = InterestShape {
            kinds: [1].into_iter().collect(),
            since: Some(500),
            ..Default::default()
        };
        let r = merge(&a, &b, &tailing(), &tailing());
        if let MergeOutcome::Merged(s) = r {
            assert_eq!(s.since, Some(500));
        } else {
            panic!("expected Merged");
        }
    }

    #[test]
    fn rule3_mixed_since_refuse() {
        let a = InterestShape {
            kinds: [1].into_iter().collect(),
            since: Some(1000),
            ..Default::default()
        };
        let b = InterestShape {
            kinds: [1].into_iter().collect(),
            since: None,
            ..Default::default()
        };
        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
    }

    // ── Rule 4 — until ───────────────────────────────────────────────────────

    #[test]
    fn rule4_both_until_take_max() {
        let a = InterestShape {
            kinds: [1].into_iter().collect(),
            until: Some(2000),
            ..Default::default()
        };
        let b = InterestShape {
            kinds: [1].into_iter().collect(),
            until: Some(3000),
            ..Default::default()
        };
        let r = merge(&a, &b, &tailing(), &tailing());
        if let MergeOutcome::Merged(s) = r {
            assert_eq!(s.until, Some(3000));
        } else {
            panic!("expected Merged");
        }
    }

    #[test]
    fn rule4_mixed_until_refuse() {
        let a = InterestShape {
            kinds: [1].into_iter().collect(),
            until: Some(2000),
            ..Default::default()
        };
        let b = InterestShape {
            kinds: [1].into_iter().collect(),
            until: None,
            ..Default::default()
        };
        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
    }

    // ── Rule 5 — limit ───────────────────────────────────────────────────────

    #[test]
    fn rule5_both_absent_limit_merge() {
        let a = InterestShape {
            kinds: [1].into_iter().collect(),
            limit: None,
            ..Default::default()
        };
        let b = InterestShape {
            kinds: [1].into_iter().collect(),
            limit: None,
            ..Default::default()
        };
        assert!(matches!(
            merge(&a, &b, &tailing(), &tailing()),
            MergeOutcome::Merged(_)
        ));
    }

    #[test]
    fn rule5_any_limit_refuse() {
        let a = InterestShape {
            kinds: [1].into_iter().collect(),
            limit: Some(100),
            ..Default::default()
        };
        let b = InterestShape {
            kinds: [1].into_iter().collect(),
            limit: None,
            ..Default::default()
        };
        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);

        let c = InterestShape {
            kinds: [1].into_iter().collect(),
            limit: Some(200),
            ..Default::default()
        };
        let d = InterestShape {
            kinds: [1].into_iter().collect(),
            limit: Some(200),
            ..Default::default()
        };
        assert_eq!(merge(&c, &d, &tailing(), &tailing()), MergeOutcome::Refused);
    }

    // ── Rule 6 — lifecycle ───────────────────────────────────────────────────

    #[test]
    fn rule6_identical_lifecycle_merge() {
        let a = InterestShape {
            kinds: [1].into_iter().collect(),
            ..Default::default()
        };
        let b = InterestShape {
            kinds: [1].into_iter().collect(),
            ..Default::default()
        };
        assert!(matches!(
            merge(&a, &b, &tailing(), &tailing()),
            MergeOutcome::Merged(_)
        ));
        assert!(matches!(
            merge(&a, &b, &one_shot(), &one_shot()),
            MergeOutcome::Merged(_)
        ));
    }

    #[test]
    fn rule6_mixed_lifecycle_refuse() {
        let a = InterestShape {
            kinds: [1].into_iter().collect(),
            ..Default::default()
        };
        let b = InterestShape {
            kinds: [1].into_iter().collect(),
            ..Default::default()
        };
        assert_eq!(
            merge(&a, &b, &tailing(), &one_shot()),
            MergeOutcome::Refused
        );
    }

    // ── Rule 7 — event_ids ───────────────────────────────────────────────────

    #[test]
    fn rule7_event_ids_union() {
        let a = InterestShape {
            event_ids: ["aaa".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let b = InterestShape {
            event_ids: ["bbb".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let r = merge(&a, &b, &one_shot(), &one_shot());
        if let MergeOutcome::Merged(s) = r {
            assert!(s.event_ids.contains("aaa"));
            assert!(s.event_ids.contains("bbb"));
        } else {
            panic!("expected Merged");
        }
    }

    #[test]
    fn rule7_event_ids_cap_refuse() {
        let ids_a: BTreeSet<String> = (0u32..600).map(|i| format!("{i:064x}")).collect();
        let ids_b: BTreeSet<String> = (500u32..1100).map(|i| format!("{i:064x}")).collect();
        let a = InterestShape {
            event_ids: ids_a,
            ..Default::default()
        };
        let b = InterestShape {
            event_ids: ids_b,
            ..Default::default()
        };
        assert_eq!(
            merge(&a, &b, &one_shot(), &one_shot()),
            MergeOutcome::Refused
        );
    }

    // ── Rule 8 — addresses ───────────────────────────────────────────────────

    #[test]
    fn rule8_address_union_merges() {
        let coord_a = NaddrCoord {
            pubkey: "a".repeat(64),
            kind: 30023,
            d_tag: "post-a".to_string(),
        };
        let coord_b = NaddrCoord {
            pubkey: "b".repeat(64),
            kind: 30023,
            d_tag: "post-b".to_string(),
        };
        let a = InterestShape {
            kinds: [30023].into_iter().collect(),
            addresses: [coord_a.clone()].into_iter().collect(),
            ..Default::default()
        };
        let b = InterestShape {
            kinds: [30023].into_iter().collect(),
            addresses: [coord_b.clone()].into_iter().collect(),
            ..Default::default()
        };
        let r = merge(&a, &b, &one_shot(), &one_shot());
        if let MergeOutcome::Merged(s) = r {
            assert!(s.addresses.contains(&coord_a));
            assert!(s.addresses.contains(&coord_b));
        } else {
            panic!("expected Merged");
        }
    }

    #[test]
    fn rule8_address_dedup_identical_coord() {
        let coord = NaddrCoord {
            pubkey: "a".repeat(64),
            kind: 30023,
            d_tag: "my-post".to_string(),
        };
        let a = InterestShape {
            kinds: [30023].into_iter().collect(),
            addresses: [coord.clone()].into_iter().collect(),
            ..Default::default()
        };
        let b = a.clone();
        let r = merge(&a, &b, &one_shot(), &one_shot());
        if let MergeOutcome::Merged(s) = r {
            assert_eq!(s.addresses.len(), 1);
        } else {
            panic!("expected Merged");
        }
    }

    #[test]
    fn rule8_addresses_respect_other_rules() {
        let coord = NaddrCoord {
            pubkey: "a".repeat(64),
            kind: 30023,
            d_tag: "post".to_string(),
        };
        let a = InterestShape {
            kinds: [30023].into_iter().collect(),
            addresses: [coord.clone()].into_iter().collect(),
            ..Default::default()
        };
        let b = a.clone();
        assert_eq!(
            merge(&a, &b, &tailing(), &one_shot()),
            MergeOutcome::Refused
        );
    }

    // ── Rule 9 — relay_pin (host-relay-pin / h-tag coalesce) ─────────────────
    //
    // A `Some(host)` `relay_pin` is a hard routing override: must NOT merge
    // across different hosts, must NOT merge with `None`, and must merge with
    // identical `Some(host)`. When the pin matches, the rest of the lattice
    // (chiefly Rule 2) coalesces same-host shapes that differ only in their
    // per-room `h` tag values into a single per-host REQ.

    #[test]
    fn rule9_identical_relay_pin_coalesces_h_tags() {
        // Two interests pinned to the same host but carrying different `h`
        // tag values must merge into one per-host REQ whose `h` set is the
        // union — this is the generic h-tag-coalesce behavior.
        let mut a = InterestShape {
            kinds: [9].into_iter().collect(),
            ..Default::default()
        };
        a.relay_pin = Some("wss://host.example.com".into());
        let mut b = a.clone();
        // Different `h` value per side — Rule 2 must union them.
        let mut tags = BTreeMap::new();
        tags.insert(
            "h".to_string(),
            ["room-a".to_string()].into_iter().collect::<BTreeSet<_>>(),
        );
        a.tags = tags;
        let mut tags_b = BTreeMap::new();
        tags_b.insert(
            "h".to_string(),
            ["room-b".to_string()].into_iter().collect::<BTreeSet<_>>(),
        );
        b.tags = tags_b;
        let r = merge(&a, &b, &tailing(), &tailing());
        if let MergeOutcome::Merged(s) = r {
            assert_eq!(s.relay_pin.as_deref(), Some("wss://host.example.com"));
            // Tag values union across the same dimension (Rule 2).
            assert_eq!(s.tags.get("h").unwrap().len(), 2);
        } else {
            panic!("expected Merged; identical relay_pin must coalesce h-tag values");
        }
    }

    #[test]
    fn rule9_different_relay_pin_refuses() {
        // Two host-pinned interests targeting DIFFERENT hosts must NOT collapse
        // into a single wire frame — they're literally going to different relays.
        let mut a = InterestShape {
            kinds: [9].into_iter().collect(),
            ..Default::default()
        };
        a.relay_pin = Some("wss://host-a.example.com".into());
        let mut b = InterestShape {
            kinds: [9].into_iter().collect(),
            ..Default::default()
        };
        b.relay_pin = Some("wss://host-b.example.com".into());
        assert_eq!(
            merge(&a, &b, &tailing(), &tailing()),
            MergeOutcome::Refused,
            "different relay_pin must refuse — they go to different relays"
        );
    }

    #[test]
    fn rule9_pinned_does_not_absorb_unpinned() {
        // Unlike Rule 1's wildcard kinds, `None` does NOT absorb `Some(_)`:
        // mixing pinned + unpinned would either leak pinned content or narrow
        // the unpinned scope — both correctness regressions.
        let mut pinned = InterestShape {
            kinds: [9].into_iter().collect(),
            ..Default::default()
        };
        pinned.relay_pin = Some("wss://host.example.com".into());
        let unpinned = InterestShape {
            kinds: [9].into_iter().collect(),
            ..Default::default()
        };
        // pinned ∪ unpinned must refuse in BOTH directions (symmetric refusal).
        assert_eq!(
            merge(&pinned, &unpinned, &tailing(), &tailing()),
            MergeOutcome::Refused
        );
        assert_eq!(
            merge(&unpinned, &pinned, &tailing(), &tailing()),
            MergeOutcome::Refused
        );
    }

    #[test]
    fn rule9_both_none_merges() {
        // The common case (no pin on either side) is unaffected by Rule 9.
        let a = shape_with_kinds(&[1, 6]);
        let b = shape_with_kinds(&[1, 6]);
        assert!(matches!(
            merge(&a, &b, &tailing(), &tailing()),
            MergeOutcome::Merged(_)
        ));
    }
}
