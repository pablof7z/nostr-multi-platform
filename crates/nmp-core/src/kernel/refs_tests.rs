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
        kernel.live_event_claims.contains(&primary_id),
        "a Live claim on an addressable coord registers a tailing slot"
    );

    kernel.release_ref(RefNamespace::Event, &uri, "screen");
    assert!(
        !kernel.live_event_claims.contains(&primary_id),
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
        !kernel.live_event_claims.contains(&id),
        "an immutable nevent id can never change, so Live degrades to one-shot"
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
