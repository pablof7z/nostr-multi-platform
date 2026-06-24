//! ADR-0063 (#1671 Lane B/D) — event-resolver unit tests for the kernel-owned
//! `RefResolver` primitive: the event Live tailing path, per-consumer owner
//! lifecycle, CacheOk/Live dedup to one slot per key, the event-ingest per-key
//! rev bump, and event shape-narrowing.
//!
//! These tests drive the `resolve_ref` Event seam with RAW event keys
//! (ADR-0063 / FFI contract: a lowercase-64-hex id or a `kind:pubkey:d`
//! coordinate) — NOT `nostr:` URIs. Raw-key PARSE coverage (well-formed +
//! malformed fail-closed) lives in `refs_tests_key.rs`; profile-resolver +
//! shared lifecycle/dedup/rev tests live in `refs_tests_profile.rs`.

use super::nostr::NostrEvent;
use super::refs::{EventShape, RefLiveness, RefNamespace, RefResolveMetadata, RefShape};
use super::*;
use crate::relay::{DEFAULT_VISIBLE_LIMIT, RelayRole};

fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

/// The canonical raw `kind:pubkey:d` coordinate key (ADR-0063 / FFI contract).
/// The `resolve_ref` Event seam takes this, NOT a `nostr:` URI.
fn coord_key(kind: u32, author: &str, d_tag: &str) -> String {
    format!("{kind}:{author}:{d_tag}")
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
        tags: ev
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
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
fn event_id_metadata_resolve_preserves_nevent_author_and_relay_hints() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let event_id = hex64("e");
    let author = hex64("a");
    let relay = "wss://metadata-hint.test".to_string();

    kernel.resolve_ref_with_metadata(
        RefNamespace::Event,
        event_id.clone(),
        "embed".into(),
        RefShape::Event(EventShape::Embed),
        RefLiveness::CacheOk,
        false,
        RefResolveMetadata {
            hints: vec![relay.clone()],
            event_author: Some(author.clone()),
        },
    );

    let interest_id = kernel
        .test_claim_interest_id(&event_id)
        .expect("metadata resolve must register claim expansion");
    let claim = kernel
        .pending_claims
        .get(&interest_id)
        .expect("claim expansion row must exist");
    assert_eq!(claim.author.as_deref(), Some(author.as_str()));
    assert!(
        claim.candidate_queue.contains(&relay),
        "relay TLV must seed the claim-expansion candidate queue"
    );
}

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
        coord_key(kind, &author, d_tag),
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
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "refs-test",
        article,
    );
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
    kernel.relay_connected(RelayRole::Content);
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let d_tag = "live-doc";
    let kind = 30023u32;
    let primary_id = format!("{kind}:{author}:{d_tag}");
    // Raw `kind:pubkey:d` coordinate key (ADR-0063 Event key, not a URI).
    let uri = coord_key(kind, &author, d_tag);

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

    // A replacement (newer kind:30023 at the same coord) ingested while Live must
    // update the coord row (per-key rev advances at the ingest chokepoint).
    let rev_before = kernel.ref_row_rev(RefNamespace::Event, &primary_id);
    let replacement = signed_addressable(&keys, kind, d_tag, 1_700_000_500);
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "refs-test",
        replacement,
    );
    assert!(
        kernel.ref_row_rev(RefNamespace::Event, &primary_id) > rev_before,
        "a replacement ingest while Live updates the coord row (per-key rev)"
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
    // Raw 64-hex event-id key — immutable, so Live degrades to one-shot.
    let id = hex64("1dd");
    kernel.resolve_ref(
        RefNamespace::Event,
        id.clone(),
        "screen".into(),
        RefShape::Event(EventShape::Raw),
        RefLiveness::Live,
        false,
        Vec::new(),
    );
    assert!(
        !kernel.live_event_claims.contains_key(&id),
        "an immutable event-id can never change, so Live degrades to one-shot"
    );
    assert!(
        kernel.live_event_claims.is_empty(),
        "an immutable id registers no live tailing slot at all"
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
    // Raw `kind:pubkey:d` coordinate key (ADR-0063 Event key, not a URI).
    let uri = coord_key(kind, &author, d_tag);

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
    // Raw `kind:pubkey:d` coordinate key (ADR-0063 Event key, not a URI).
    let uri = coord_key(kind, &author, d_tag);

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
    // Raw `kind:pubkey:d` coordinate key (ADR-0063 Event key, not a URI).
    let uri = coord_key(kind, &author, d_tag);

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
    // Raw `kind:pubkey:d` coordinate key (ADR-0063 Event key, not a URI).
    let uri = coord_key(kind, &author, d_tag);

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
    // Raw 64-hex event-id key (ADR-0063 Event key, not a URI).
    let uri = id.clone();

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
