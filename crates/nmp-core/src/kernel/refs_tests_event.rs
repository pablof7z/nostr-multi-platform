//! ADR-0063 (#1671 Lane B) — event-resolver unit tests for the kernel-owned
//! `RefResolver` primitive: the event Live tailing path, per-consumer owner
//! lifecycle, CacheOk/Live dedup to one slot per key, and the event-ingest
//! per-key rev bump.
//!
//! **Lane D merge target** — the following tests will receive edits at
//! integration merge: `per_key_rev_advances_on_event_ingest_for_claimed_coord`,
//! `event_live_addressable_registers_and_releases_tailing_slot`,
//! `event_live_immutable_id_degrades_to_oneshot`.
//!
//! Profile-resolver + shared lifecycle/dedup/rev tests live in
//! `refs_tests_profile.rs`.

use super::nostr::NostrEvent;
use super::refs::{EventShape, ProfileShape, RefLiveness, RefNamespace, RefShape};
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

/// ADR-0063 (#1671 Lane B) — count active registry interests addressing the
/// addressable coordinate `(kind, author, d_tag)`. Both the `CacheOk`
/// one-shot and the `Live` tailing slot carry this same addressable filter,
/// so this counts how many distinct interests / wire REQs exist for one event
/// key. Used to assert exactly ONE per key across the CacheOk/Live dedup
/// (BLOCKING 3) and ZERO after a `Live` teardown (BLOCKING 1, no owner leak).
impl Kernel {
    fn event_claim_interest_count_for_test(&self, kind: u32, author: &str, d_tag: &str) -> usize {
        self.lifecycle
            .registry()
            .iter_active()
            .into_iter()
            .filter(|i| {
                i.shape.kinds.contains(&kind)
                    && i.shape.authors.contains(author)
                    && i.shape.tags.get("d").is_some_and(|v| v.contains(d_tag))
            })
            .count()
    }
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

// ─── HIGH 4: shape narrows on release of the widest event consumer ────────────

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
