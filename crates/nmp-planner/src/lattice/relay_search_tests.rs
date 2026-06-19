use super::*;
use crate::interest::{InterestLifecycle, InterestShape};
use std::collections::{BTreeMap, BTreeSet};

fn tailing() -> InterestLifecycle {
    InterestLifecycle::Tailing
}

fn shape_with_kinds(kinds: &[u32]) -> InterestShape {
    InterestShape {
        kinds: kinds.iter().copied().collect(),
        ..Default::default()
    }
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

// ── Rule 10 — search ───────────────────────────────────────────────────

#[test]
fn rule10_identical_search_merges() {
    let mut a = shape_with_kinds(&[1]);
    a.search = Some("nostr rust".to_string());
    let b = a.clone();

    let r = merge(&a, &b, &tailing(), &tailing());
    assert!(matches!(r, MergeOutcome::Merged(ref s) if s.search.as_deref() == Some("nostr rust")));
}

#[test]
fn rule10_search_does_not_absorb_non_search() {
    let mut search = shape_with_kinds(&[1]);
    search.search = Some("nostr rust".to_string());
    let plain = shape_with_kinds(&[1]);

    assert_eq!(
        merge(&search, &plain, &tailing(), &tailing()),
        MergeOutcome::Refused
    );
    assert_eq!(
        merge(&plain, &search, &tailing(), &tailing()),
        MergeOutcome::Refused
    );
}

#[test]
fn rule10_different_search_refuses() {
    let mut a = shape_with_kinds(&[1]);
    a.search = Some("nostr rust".to_string());
    let mut b = shape_with_kinds(&[1]);
    b.search = Some("nostr relay".to_string());

    assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
}
