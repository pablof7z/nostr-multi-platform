//! ADR-0063 (#1671 Lane B) — unit tests for the kernel-owned `RefResolver`
//! primitive: the `resolve_ref` / `release_ref` seam, dedup + refcount, the
//! Live-wins liveness merge, shape↔liveness orthogonality, the per-key rev
//! bumped at all three sites (resolve / release / ingest), and the new
//! addressable-event `Live` tailing path.
//!
//! These tests drive the primitive directly — no actor, no relay traffic, no
//! FFI — and assert on the resolver's own kernel state (`profile_claims` /
//! `event_claims` refcounts, the interest lifecycle, `live_event_claims`, the
//! per-key rev, and the widest-demanded-shape record).

use super::nostr::NostrEvent;
use super::refs::{
    EventShape, ProfileShape, RefLiveness, RefNamespace, RefResolver, RefShape,
};
use super::refs::{EventNs, ProfileNs};
use super::*;
use crate::nip19::{encode_naddr, encode_nevent, NaddrData, NeventData};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

fn naddr_uri(kind: u32, author: &str, d_tag: &str) -> String {
    let bech = encode_naddr(&NaddrData {
        identifier: d_tag.to_string(),
        pubkey: author.to_string(),
        kind,
        relays: vec![],
    })
    .expect("encode_naddr");
    format!("nostr:{bech}")
}

fn nevent_uri(event_id: &str) -> String {
    let bech = encode_nevent(&NeventData {
        event_id: event_id.to_string(),
        relays: vec![],
        author: None,
        kind: Some(1),
    })
    .expect("encode_nevent");
    format!("nostr:{bech}")
}

fn inject_kind0(kernel: &mut Kernel, pubkey: &str, display_name: &str) {
    let content = serde_json::json!({
        "display_name": display_name,
        "picture": "https://example.com/a.png",
    })
    .to_string();
    kernel.inject_profile(NostrEvent {
        id: "0".repeat(64),
        pubkey: pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 0,
        tags: Vec::new(),
        content,
        sig: String::new(),
    });
}

/// A real signed addressable (kind:30023) event carrying `d_tag`. Built with a
/// valid signature so it passes the production `verify_and_persist` chokepoint
/// (where the event-ingest per-key rev bump lives).
fn signed_addressable(keys: &::nostr::Keys, kind: u32, d_tag: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let ev = EventBuilder::new(Kind::from(kind as u16), "body")
        .tags([Tag::identifier(d_tag.to_string())])
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

fn profile_card(kernel: &mut Kernel, pk: &str, consumer: &str, liveness: RefLiveness) {
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.to_string(),
        consumer.to_string(),
        RefShape::Profile(ProfileShape::Card),
        liveness,
        false,
        Vec::new(),
    );
}

// ─── trait contract ──────────────────────────────────────────────────────────

#[test]
fn namespace_const_matches_marker() {
    assert_eq!(ProfileNs::NAMESPACE, RefNamespace::Profile);
    assert_eq!(EventNs::NAMESPACE, RefNamespace::Event);
}

#[test]
fn liveness_from_ffi_and_round_trips_through_profile_liveness() {
    assert_eq!(RefLiveness::from_ffi(0), RefLiveness::CacheOk);
    assert_eq!(RefLiveness::from_ffi(1), RefLiveness::Live);
    // Orthogonal conversions used by the scaffold delegators.
    assert_eq!(RefLiveness::from(ProfileLiveness::Live), RefLiveness::Live);
    assert_eq!(ProfileLiveness::from(RefLiveness::CacheOk), ProfileLiveness::CacheOk);
}

// ─── dedup + refcount-to-zero ────────────────────────────────────────────────

#[test]
fn resolve_ref_dedups_consumers_then_tears_down_on_last_release() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("a11ce");

    profile_card(&mut kernel, &alice, "view-a", RefLiveness::CacheOk);
    profile_card(&mut kernel, &alice, "view-b", RefLiveness::CacheOk);

    assert_eq!(
        kernel.profile_claims.get(&alice).map(|s| s.len()),
        Some(2),
        "two consumers of one key share one refcounted slot"
    );
    assert!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice).is_some(),
        "one deduped kind:0 interest is live"
    );

    // First release: slot survives.
    kernel.release_ref(RefNamespace::Profile, &alice, "view-a");
    assert_eq!(kernel.profile_claims.get(&alice).map(|s| s.len()), Some(1));
    assert!(kernel.profile_claim_interest_lifecycle_for_test(&alice).is_some());

    // Last release: refcount → zero, the slot + shape record disappear.
    kernel.release_ref(RefNamespace::Profile, &alice, "view-b");
    assert!(
        kernel.profile_claims.get(&alice).is_none(),
        "the last release removes the refcount entry"
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        None,
        "the deduped interest is dropped on the last release"
    );
    assert_eq!(
        kernel.ref_demanded_profile_shape(&alice),
        None,
        "the widest-shape record is dropped on full teardown (D5)"
    );
}

// ─── Live wins ───────────────────────────────────────────────────────────────

#[test]
fn live_wins_over_cacheok_regardless_of_order() {
    // CacheOk then Live → Tailing.
    let mut k1 = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let a = hex64("a1");
    profile_card(&mut k1, &a, "feed", RefLiveness::CacheOk);
    assert_eq!(
        k1.profile_claim_interest_lifecycle_for_test(&a),
        Some(false),
        "a lone CacheOk claim is OneShot"
    );
    profile_card(&mut k1, &a, "screen", RefLiveness::Live);
    assert_eq!(
        k1.profile_claim_interest_lifecycle_for_test(&a),
        Some(true),
        "a later Live claim upgrades the slot to Tailing (Live wins)"
    );

    // Live then CacheOk → stays Tailing.
    let mut k2 = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let b = hex64("b2");
    profile_card(&mut k2, &b, "screen", RefLiveness::Live);
    profile_card(&mut k2, &b, "feed", RefLiveness::CacheOk);
    assert_eq!(
        k2.profile_claim_interest_lifecycle_for_test(&b),
        Some(true),
        "a later CacheOk claim must NOT downgrade a Tailing slot"
    );
}

// ─── shape ⟂ liveness ────────────────────────────────────────────────────────

#[test]
fn shape_is_independent_of_liveness() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("5ha9e");

    // Ref + CacheOk: narrow shape, OneShot.
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "feed".into(),
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(kernel.ref_demanded_profile_shape(&pk), Some(ProfileShape::Ref));
    assert_eq!(kernel.profile_claim_interest_lifecycle_for_test(&pk), Some(false));

    // Widen the shape to Card WITHOUT touching liveness (still CacheOk): shape
    // changes, liveness does not.
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "detail".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(kernel.ref_demanded_profile_shape(&pk), Some(ProfileShape::Card));
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&pk),
        Some(false),
        "widening the shape must not change liveness"
    );

    // Raise liveness to Live with the NARROW shape: liveness changes, the
    // widest demanded shape is unaffected (monotonic widen — not narrowed).
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "screen".into(),
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    assert_eq!(
        kernel.ref_demanded_profile_shape(&pk),
        Some(ProfileShape::Card),
        "raising liveness must not narrow the widest demanded shape"
    );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&pk),
        Some(true),
        "liveness moves orthogonally to shape"
    );
}

// ─── per-key rev: resolve + release sites ────────────────────────────────────

#[test]
fn per_key_rev_advances_on_resolve_and_release() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("e7");

    assert_eq!(kernel.ref_row_rev(RefNamespace::Profile, &pk), 0);

    profile_card(&mut kernel, &pk, "c1", RefLiveness::CacheOk);
    let r1 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r1 > 0, "resolve must advance the per-key rev (site 1)");

    profile_card(&mut kernel, &pk, "c2", RefLiveness::CacheOk);
    let r2 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r2 > r1, "a second resolve advances it again");

    kernel.release_ref(RefNamespace::Profile, &pk, "c1");
    let r3 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r3 > r2, "release must advance the per-key rev (site 2)");

    kernel.release_ref(RefNamespace::Profile, &pk, "c2");
    let r4 = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(r4 > r3, "the last release advances it once more");
}

// ─── per-key rev: profile ingest site ────────────────────────────────────────

#[test]
fn per_key_rev_advances_on_kind0_ingest_only_for_claimed_authors() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let claimed = hex64("c1a1med");
    let stranger = hex64("57ra9er");

    profile_card(&mut kernel, &claimed, "view", RefLiveness::CacheOk);
    let before = kernel.ref_row_rev(RefNamespace::Profile, &claimed);

    // A kind:0 rewriting the CLAIMED author's row bumps ITS per-key rev (site 3).
    inject_kind0(&mut kernel, &claimed, "Claimed Author");
    let after = kernel.ref_row_rev(RefNamespace::Profile, &claimed);
    assert!(
        after > before,
        "a kind:0 for a claimed author advances its per-key rev (ingest site)"
    );

    // A kind:0 for an UNCLAIMED author creates no row rev — the per-key map stays
    // bounded to resolved refs.
    inject_kind0(&mut kernel, &stranger, "Stranger");
    assert_eq!(
        kernel.ref_row_rev(RefNamespace::Profile, &stranger),
        0,
        "an unclaimed author never enters the per-key rev map"
    );
}

// ─── per-key rev: event ingest site ──────────────────────────────────────────

#[test]
fn per_key_rev_advances_on_event_ingest_for_claimed_coord() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let d_tag = "my-article";
    let kind = 30023u32;
    let primary_id = format!("{kind}:{author}:{d_tag}");

    kernel.resolve_ref(
        RefNamespace::Event,
        naddr_uri(kind, &author, d_tag),
        "embed".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    let before = kernel.ref_row_rev(RefNamespace::Event, &primary_id);
    assert!(before > 0, "the resolve site bumped the event row rev");

    // A freshly persisted, signed kind:30023 matching the claimed coord bumps
    // its row through the production `verify_and_persist` chokepoint.
    let article = signed_addressable(&keys, kind, d_tag, 1_700_000_000);
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.example/", "refs-test", article);
    let after = kernel.ref_row_rev(RefNamespace::Event, &primary_id);
    assert!(
        after > before,
        "an ingested event matching a claimed coord advances its per-key rev"
    );
}

// ─── event Live (addressable tailing) ────────────────────────────────────────

#[test]
fn event_live_addressable_registers_and_releases_tailing_slot() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let author = hex64("11e");
    let d_tag = "live-doc";
    let kind = 30023u32;
    let primary_id = format!("{kind}:{author}:{d_tag}");
    let uri = naddr_uri(kind, &author, d_tag);

    kernel.resolve_ref(
        RefNamespace::Event,
        uri.clone(),
        "screen".into(),
        RefShape::Event(EventShape::Raw),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    assert!(
        kernel.live_event_claims.contains_key(&primary_id),
        "a Live claim on an addressable coord registers a tailing slot"
    );

    kernel.release_ref(RefNamespace::Event, &uri, "screen");
    assert!(
        !kernel.live_event_claims.contains_key(&primary_id),
        "the last release tears the tailing slot down"
    );
}

#[test]
fn event_live_immutable_id_degrades_to_oneshot() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let id = hex64("1dd");
    kernel.resolve_ref(
        RefNamespace::Event,
        nevent_uri(&id),
        "screen".into(),
        RefShape::Event(EventShape::Raw),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    assert!(
        !kernel.live_event_claims.contains_key(&id),
        "an immutable nevent id can never change, so Live degrades to one-shot"
    );
}

// ─── BLOCKING 1: event Live teardown — per-consumer owner lifecycle ──────────

/// Two `Live` consumers share one tailing slot; releasing the FIRST keeps the
/// slot (its owner is detached per-consumer), releasing the LAST tears the slot
/// down with no leaked owner / sub.
#[test]
fn event_live_plus_live_tears_down_exactly_on_last_release() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let author = hex64("11e2");
    let d_tag = "doc";
    let kind = 30023u32;
    let primary_id = format!("{kind}:{author}:{d_tag}");
    let uri = naddr_uri(kind, &author, d_tag);

    for c in ["c1", "c2"] {
        kernel.resolve_ref(
            RefNamespace::Event,
            uri.clone(),
            c.into(),
            RefShape::Event(EventShape::Raw),
            RefLiveness::Live,
            false,
            Vec::new(),
        );
    }
    assert_eq!(
        kernel.event_claim_interest_count_for_test(kind, &author, d_tag),
        1,
        "two Live consumers share ONE tailing slot"
    );
    assert_eq!(
        kernel.live_event_claims.get(&primary_id).map(|s| s.len()),
        Some(2),
        "both Live consumers hold a tracked live owner"
    );

    // Release the FIRST Live consumer: slot survives, its owner is detached.
    kernel.release_ref(RefNamespace::Event, &uri, "c1");
    assert_eq!(
        kernel.event_claim_interest_count_for_test(kind, &author, d_tag),
        1,
        "the tailing slot survives while a Live consumer remains"
    );
    assert_eq!(
        kernel.live_event_claims.get(&primary_id).map(|s| s.len()),
        Some(1),
        "the first consumer's live owner is detached on its own release (no leak)"
    );

    // Release the LAST Live consumer: slot torn down, no owner leaks.
    kernel.release_ref(RefNamespace::Event, &uri, "c2");
    assert_eq!(
        kernel.event_claim_interest_count_for_test(kind, &author, d_tag),
        0,
        "the tailing slot tears down exactly when the last Live consumer leaves"
    );
    assert!(
        !kernel.live_event_claims.contains_key(&primary_id),
        "no live-owner record leaks past the last release"
    );
}

/// A `Live` consumer released AHEAD of a surviving `CacheOk` consumer tears the
/// tailing slot down (last live owner gone) without trying to drop an owner the
/// `CacheOk` consumer never registered. The final `CacheOk` release is a clean
/// no-op on the already-gone slot.
#[test]
fn event_live_released_before_cacheok_consumer_no_leak() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let author = hex64("11e3");
    let d_tag = "doc";
    let kind = 30023u32;
    let primary_id = format!("{kind}:{author}:{d_tag}");
    let uri = naddr_uri(kind, &author, d_tag);

    // Live first (tailing slot), then CacheOk dedups onto the same key.
    kernel.resolve_ref(
        RefNamespace::Event,
        uri.clone(),
        "live".into(),
        RefShape::Event(EventShape::Raw),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    kernel.resolve_ref(
        RefNamespace::Event,
        uri.clone(),
        "cache".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(
        kernel.event_claim_interest_count_for_test(kind, &author, d_tag),
        1,
        "Live + CacheOk on one key share ONE slot"
    );

    // Release the Live consumer FIRST: the tailing slot tears down (no live owner
    // remains) even though the CacheOk consumer still holds the key.
    kernel.release_ref(RefNamespace::Event, &uri, "live");
    assert_eq!(
        kernel.event_claim_interest_count_for_test(kind, &author, d_tag),
        0,
        "the tailing slot tears down when the last Live consumer leaves"
    );
    assert!(!kernel.live_event_claims.contains_key(&primary_id));
    assert!(
        kernel
            .event_claims
            .get(&primary_id)
            .is_some_and(|s| s.contains("cache")),
        "the CacheOk consumer still holds the refcount"
    );

    // Final CacheOk release: clean no-op on the already-gone tailing slot.
    kernel.release_ref(RefNamespace::Event, &uri, "cache");
    assert!(
        kernel.event_claims.get(&primary_id).is_none(),
        "the last release removes the refcount entry"
    );
}

// ─── BLOCKING 3: event CacheOk/Live dedup to ONE slot per key ─────────────────

#[test]
fn event_cacheok_then_live_dedups_to_one_slot() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let author = hex64("a47c");
    let d_tag = "doc";
    let kind = 30023u32;
    let uri = naddr_uri(kind, &author, d_tag);

    // Cold CacheOk naddr claim registers ONE OneshotApi interest.
    kernel.resolve_ref(
        RefNamespace::Event,
        uri.clone(),
        "feed".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(
        kernel.event_claim_interest_count_for_test(kind, &author, d_tag),
        1,
        "a cold CacheOk naddr claim registers exactly one interest"
    );

    // A later Live claim must UPGRADE that slot (retire the one-shot), not add a
    // second tailing slot → still exactly ONE interest / REQ (Live wins).
    kernel.resolve_ref(
        RefNamespace::Event,
        uri.clone(),
        "screen".into(),
        RefShape::Event(EventShape::Raw),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    assert_eq!(
        kernel.event_claim_interest_count_for_test(kind, &author, d_tag),
        1,
        "CacheOk-then-Live dedups to ONE interest (Live upgrades in place)"
    );
}

#[test]
fn event_live_then_cacheok_dedups_to_one_slot() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let author = hex64("a47d");
    let d_tag = "doc";
    let kind = 30023u32;
    let uri = naddr_uri(kind, &author, d_tag);

    kernel.resolve_ref(
        RefNamespace::Event,
        uri.clone(),
        "screen".into(),
        RefShape::Event(EventShape::Raw),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    assert_eq!(
        kernel.event_claim_interest_count_for_test(kind, &author, d_tag),
        1,
        "a Live claim registers one tailing slot"
    );

    // A later CacheOk claim dedups onto the existing Live slot (Live wins) — it
    // must NOT register a second one-shot interest.
    kernel.resolve_ref(
        RefNamespace::Event,
        uri.clone(),
        "feed".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(
        kernel.event_claim_interest_count_for_test(kind, &author, d_tag),
        1,
        "Live-then-CacheOk stays ONE interest (CacheOk dedups onto the Live slot)"
    );
}

// ─── HIGH 4: shape narrows on release of the widest consumer ──────────────────

#[test]
fn profile_shape_narrows_when_widest_consumer_releases() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("5ha9e2");

    // Card consumer + Ref consumer → the row carries the widest (Card).
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "detail".into(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "feed".into(),
        RefShape::Profile(ProfileShape::Ref),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(kernel.ref_demanded_profile_shape(&pk), Some(ProfileShape::Card));
    let rev_before = kernel.ref_row_rev(RefNamespace::Profile, &pk);

    // Release the Card (widest) consumer while the Ref consumer remains: the row
    // must NARROW to Ref and the per-key rev must bump so the wire carries it.
    kernel.release_ref(RefNamespace::Profile, &pk, "detail");
    assert_eq!(
        kernel.ref_demanded_profile_shape(&pk),
        Some(ProfileShape::Ref),
        "releasing the widest (Card) consumer narrows the row to Ref (HIGH 4)"
    );
    assert!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk) > rev_before,
        "the narrowing bumps the per-key rev"
    );
}

#[test]
fn event_shape_narrows_when_widest_consumer_releases() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let id = hex64("e57a");
    let uri = nevent_uri(&id);

    kernel.resolve_ref(
        RefNamespace::Event,
        uri.clone(),
        "raw".into(),
        RefShape::Event(EventShape::Raw),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    kernel.resolve_ref(
        RefNamespace::Event,
        uri.clone(),
        "embed".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert_eq!(kernel.ref_demanded_event_shape(&id), Some(EventShape::Raw));

    kernel.release_ref(RefNamespace::Event, &uri, "raw");
    assert_eq!(
        kernel.ref_demanded_event_shape(&id),
        Some(EventShape::Embed),
        "releasing the widest (Raw) event consumer narrows the row to Embed (HIGH 4)"
    );
}

// ─── BLOCKING 2: per-key rev map stays bounded ───────────────────────────────

/// A spurious release of a never-claimed key must NOT create a permanent row-rev
/// entry — the map only ever holds keys that were actually claimed.
#[test]
fn spurious_release_of_never_claimed_key_creates_no_row() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("9e7e6");
    kernel.release_ref(RefNamespace::Profile, &pk, "ghost");
    assert_eq!(kernel.ref_row_rev(RefNamespace::Profile, &pk), 0);
    assert!(
        kernel
            .projection_rev_tracker
            .source_versions
            .profile_row_revs
            .is_empty(),
        "a spurious profile release must not grow the rev map (BLOCKING 2 (a))"
    );

    let uri = nevent_uri(&hex64("9e7e7"));
    kernel.release_ref(RefNamespace::Event, &uri, "ghost");
    assert!(
        kernel
            .projection_rev_tracker
            .source_versions
            .event_row_revs
            .is_empty(),
        "a spurious event release must not grow the rev map (BLOCKING 2 (a))"
    );
}

/// resolve→release→reap churn of the same key keeps the per-key rev map bounded:
/// after Lane A emits each Cleared row and reaps, the map returns to empty.
#[test]
fn per_key_rev_map_stays_bounded_under_resolve_release_churn() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("ba6c");
    for _ in 0..5 {
        profile_card(&mut kernel, &pk, "c", RefLiveness::CacheOk);
        // At most one live key while held.
        assert_eq!(
            kernel
                .projection_rev_tracker
                .source_versions
                .profile_row_revs
                .len(),
            1
        );
        kernel.release_ref(RefNamespace::Profile, &pk, "c");
        // Lane A emits the Cleared frame for this key, then reaps it.
        let reaped = kernel
            .projection_rev_tracker
            .source_versions
            .reap_cleared_profile_rows();
        assert_eq!(reaped, vec![pk.clone()], "the cleared key is reaped");
    }
    assert_eq!(
        kernel
            .projection_rev_tracker
            .source_versions
            .profile_row_revs
            .len(),
        0,
        "resolve→release→reap churn leaves the per-key rev map empty (BLOCKING 2 (b))"
    );
}

/// A key re-resolved BEFORE Lane A reaps it keeps its monotonic rev (the bump
/// cancels the pending clear), and the reap never drops a re-resolved live key.
/// This is the stale-low-rev guard coordinating with Lane A's rev consumption.
#[test]
fn re_resolve_before_reap_keeps_monotonic_rev_and_is_not_reaped() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("ab12");
    profile_card(&mut kernel, &pk, "c", RefLiveness::CacheOk);
    kernel.release_ref(RefNamespace::Profile, &pk, "c"); // row Cleared; rev retained
    let rev_at_clear = kernel.ref_row_rev(RefNamespace::Profile, &pk);
    assert!(rev_at_clear > 0);

    // Re-resolve BEFORE the reap: cancels the pending clear, rev stays monotonic.
    profile_card(&mut kernel, &pk, "c2", RefLiveness::CacheOk);
    assert!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk) > rev_at_clear,
        "a re-resolve advances the rev monotonically (no stale-low reuse)"
    );

    // The reap must leave the now-live key alone.
    let reaped = kernel
        .projection_rev_tracker
        .source_versions
        .reap_cleared_profile_rows();
    assert!(
        reaped.is_empty(),
        "a re-resolved key is no longer pending-clear, so reap leaves it"
    );
    assert!(
        kernel.ref_row_rev(RefNamespace::Profile, &pk) > 0,
        "the live re-resolved row keeps its rev (not reaped)"
    );
}

// ─── fail-closed on (namespace, shape) mismatch ──────────────────────────────

#[test]
fn namespace_shape_mismatch_is_a_silent_noop() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let pk = hex64("m15ma7ch");
    let out = kernel.resolve_ref(
        RefNamespace::Profile,
        pk.clone(),
        "v".into(),
        RefShape::Event(EventShape::Embed), // event shape under the profile namespace
        RefLiveness::CacheOk,
        false,
        Vec::new(),
    );
    assert!(out.is_empty());
    assert!(
        kernel.profile_claims.is_empty() && kernel.event_claims.is_empty(),
        "a shape/namespace mismatch must record no claim (D6 fail-closed)"
    );
}
