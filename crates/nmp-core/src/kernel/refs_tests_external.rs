//! #1654 — NIP-73 external-reference (`i:<external-id>`) end-to-end coverage for
//! the `refs.event` resolver. These tests drive the SAME `resolve_ref` Event
//! seam the id / coordinate forms use — there is NO parallel resolver — and
//! prove that an `i:<external-id>` key:
//!
//! * surfaces a cached referencing event through `lookup_for_primary_id` (the
//!   projection payload accessor) and `event_already_known` (the cache short-
//!   circuit), keyed off the event's `["i", <external-id>]` tag;
//! * advances its per-key `refs.event` rev when the referencing event is
//!   ingested while the ref is claimed (so a host re-renders the preview);
//! * fails closed (no payload, no false-positive) when no matching event is
//!   cached — the resolver never guesses a coordinate.
//!
//! Split into its own file (vs `refs_tests_event.rs`) to keep each test file
//! under the 500-LOC hard cap (AGENTS.md).

use super::nostr::NostrEvent;
use super::refs::{EventShape, RefLiveness, RefNamespace, RefShape};
use super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

/// A real signed event (any kind) carrying a NIP-73 `["i", external_id]` tag —
/// the referencing event an `i:<external-id>` ref resolves to (e.g. a kind:1111
/// NIP-22 comment on a podcast episode, or a kind:1 note tagging an isbn). Built
/// with a valid signature so it passes the production `verify_and_persist`
/// chokepoint (where the event-ingest per-key rev bump lives).
fn signed_external(keys: &::nostr::Keys, kind: u32, external_id: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let ev = EventBuilder::new(Kind::from(kind as u16), "body")
        .tags([Tag::parse(["i", external_id]).expect("parse i tag")])
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys");
    NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

/// Claim an external ref and ingest a matching referencing event through the
/// production chokepoint; return the kernel + the (key, ingested event id).
fn claim_then_ingest(external_id: &str, kind: u32) -> (Kernel, String, String) {
    let keys = ::nostr::Keys::generate();
    let key = format!("i:{external_id}");
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.resolve_ref(
        RefNamespace::Event,
        key.clone(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    let ev = signed_external(&keys, kind, external_id, 1_700_000_000);
    let ev_id = ev.id.clone();
    // The referencing event arrives on the claim's discovery oneshot, so
    // `should_store_event`'s discovery / open-interest clause admits it into the
    // `self.events` read-cache the external-ref lookup scans.
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.example/", "view", ev);
    (kernel, key, ev_id)
}

#[test]
fn external_ref_surfaces_cached_event_in_projection() {
    // #1654 — the payoff: a claimed `i:<external-id>` ref whose referencing event
    // is cached resolves to that event through `lookup_for_primary_id` (the
    // `refs.event` payload accessor) — keyed by the `["i", external_id]` tag, NOT
    // by id or coordinate. Proven-red: before the `i:` arm in
    // `lookup_for_primary_id`, the key fell through the coordinate split (kind="i"
    // rejected) → `None`, so the preview stayed permanently pending.
    let external_id = "podcast:item:guid:e1d2c3b4-0000-0000-0000-aaaabbbbcccc";
    let (kernel, key, ev_id) = claim_then_ingest(external_id, 1111);

    let stored = kernel
        .lookup_for_primary_id(&key)
        .unwrap_or_else(|| panic!("external ref {key:?} must resolve a cached event, got None"));
    assert_eq!(
        stored.id, ev_id,
        "the resolved event is the one carrying the matching `i` tag"
    );
    assert!(
        stored
            .tags
            .iter()
            .any(|t| t.len() >= 2 && t[0] == "i" && t[1] == external_id),
        "the resolved event carries the `[\"i\", {external_id:?}]` tag"
    );

    // The cache short-circuit agrees with the projection accessor (one truth).
    assert!(
        kernel.event_already_known(&key),
        "event_already_known must agree the external ref is cached"
    );
}

#[test]
fn external_ref_per_key_rev_advances_on_ingest() {
    // #1654 — a host re-renders the preview only when the row's per-key rev
    // advances. Ingesting the referencing event while the ref is claimed must
    // bump the `refs.event` row rev for the `i:` key. Proven-red: without the
    // `i:` arm the resolve recorded a claim row but the key never resolved, so
    // the ingest chokepoint's per-key bump (which matches on the resolved
    // primary_id) never fired for the external-ref key.
    let external_id = "isbn:9780375704024";
    let key = format!("i:{external_id}");
    let keys = ::nostr::Keys::generate();
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.resolve_ref(
        RefNamespace::Event,
        key.clone(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    let before = kernel.ref_row_rev(RefNamespace::Event, &key);
    assert!(
        before > 0,
        "the resolve site bumped the external-ref row rev"
    );

    let ev = signed_external(&keys, 1, external_id, 1_700_000_000);
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.example/", "view", ev);
    let after = kernel.ref_row_rev(RefNamespace::Event, &key);
    assert!(
        after > before,
        "ingesting the referencing event advances the external-ref per-key rev \
         (before={before}, after={after})"
    );
}

#[test]
fn external_ref_unresolved_fails_closed() {
    // #1654 / fail-closed: a claimed external ref with NO matching cached event
    // must NOT resolve — `lookup_for_primary_id` returns `None` and
    // `event_already_known` is false. The resolver never guesses; the preview
    // stays pending (absence ⇒ Unchanged on the carrier, never a fabricated row).
    let key = "i:doi:10.1000/never-seen".to_string();
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.resolve_ref(
        RefNamespace::Event,
        key.clone(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert!(
        kernel.event_claims.contains_key(&key),
        "the ref is claimed (a discovery REQ is in flight)"
    );
    assert!(
        kernel.lookup_for_primary_id(&key).is_none(),
        "no matching cached event ⇒ the projection payload is absent (not fabricated)"
    );
    assert!(
        !kernel.event_already_known(&key),
        "no matching cached event ⇒ event_already_known is false"
    );
}

#[test]
fn external_ref_does_not_match_different_external_id() {
    // #1654 — a claimed external ref must resolve ONLY its own external id. A
    // cached event tagging a DIFFERENT `i` value must not satisfy it (no
    // cross-id bleed through the tag scan).
    let claimed = "podcast:item:guid:wanted";
    let key = format!("i:{claimed}");
    let keys = ::nostr::Keys::generate();
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.resolve_ref(
        RefNamespace::Event,
        key.clone(),
        "view".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    // Ingest an event tagging a DIFFERENT external id.
    let other = signed_external(&keys, 1111, "podcast:item:guid:other", 1_700_000_000);
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.example/", "view", other);
    assert!(
        kernel.lookup_for_primary_id(&key).is_none(),
        "a cached event with a DIFFERENT `i` value must not satisfy the claimed ref"
    );
    assert!(
        !kernel.event_already_known(&key),
        "event_already_known must stay false for a non-matching external id"
    );
}
