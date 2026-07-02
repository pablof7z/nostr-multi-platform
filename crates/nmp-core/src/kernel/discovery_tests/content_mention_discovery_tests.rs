//! V-56: content-level profile mention discovery.
//!
//! A `nostr:npub1…` mention that appears only in note content (no matching
//! `p`-tag) must reach `UnknownIds` and subsequently be emitted as a
//! profiles-arm discovery REQ, with fast-path and dedup guards so the common
//! (no-mention) case stays allocation-free (D8) and an already-known /
//! already-pending pubkey is never double-counted.

use super::discovery_fixtures_support::{drain_and_register, install_bootstrap_relays, planner_req_filters, tag, MENTIONED_PK};
use crate::kernel::{Kernel, NostrEvent};
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Driven through `ingest_timeline_event` (the production hot path) so this
/// test proves that the production ingest path connects content-extracted
/// pubkeys to `UnknownIds` — not just the helper in isolation.
///
/// Uses real Schnorr-signed events (same pattern as `provenance_wire_tests`)
/// so `VerifiedEvent::try_from_raw` passes and discovery code is reached.
/// Uses `diag-firehose-stress` sub_id to bypass the `timeline_authors` gate.
#[test]
fn v56_content_only_npub_mention_feeds_profile_discovery() {
    use nmp_nostr_id::encode_npub;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_bootstrap_relays(&mut kernel);

    // A pubkey that is NOT in `profiles` and NOT in any `p`-tag.
    let content_only_pk = MENTIONED_PK;
    let npub = encode_npub(content_only_pk).expect("encode_npub must succeed for a 64-hex string");
    let content = format!("Check out nostr:{npub} — great follow");

    // Real Schnorr-signed event: `VerifiedEvent::try_from_raw` verifies the
    // signature before the event can proceed to discovery. The tags list has
    // NO p-tag for MENTIONED_PK — that's the V-56 regression scenario.
    let keys = ::nostr::Keys::generate();
    let nostr_event = ::nostr::EventBuilder::text_note(content)
        .custom_created_at(::nostr::Timestamp::from(1_000u64))
        .sign_with_keys(&keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    let note = NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: Vec::new(), // deliberately NO p-tag for MENTIONED_PK
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    };

    // Nothing in UnknownIds before ingest.
    assert_eq!(
        kernel.unknown_ids.pending_len(),
        0,
        "precondition: unknown_ids must be empty before ingest"
    );

    // Ingest through the production path (diag-firehose bypasses author gate).
    kernel.ingest_timeline_event(
        nmp_network::role::RelayRole::Content,
        "wss://test.relay/",
        "diag-firehose-stress",
        note,
    );

    // The content-extracted pubkey must have landed in unknown_ids.
    assert!(
        kernel.unknown_ids.pending_len() > 0,
        "V-56: content-only nostr:npub1 mention must add pubkey to UnknownIds \
         pending set after ingest"
    );

    // Drain + planner tick must produce a profiles-arm REQ carrying the pubkey.
    let _ = kernel.drain_unknown_oneshots();
    let frames = drain_and_register(&mut kernel);
    let filters = planner_req_filters(&frames);
    let joined = filters.join("\n");
    assert!(
        joined.contains(content_only_pk),
        "V-56: planner must emit a profiles-arm REQ carrying the content-only \
         mention pubkey; got filters: {filters:?}"
    );
}

/// V-56 fast-path: content with no `nostr:` substring must not record anything
/// in UnknownIds (D8 — zero allocation on the common path).
#[test]
fn v56_no_nostr_uri_in_content_skips_fast_path() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.collect_content_mention_pubkeys("hello world, no mentions here");
    assert_eq!(
        kernel.unknown_ids.pending_len(),
        0,
        "V-56 D8 fast-path: content without nostr: must leave UnknownIds empty"
    );
}

/// V-56 dedup: a pubkey that is both in a `p`-tag (already fed via
/// `collect_unknown_refs`) and in content must not double-count in UnknownIds.
#[test]
fn v56_content_mention_dedups_with_p_tag() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    use nmp_nostr_id::encode_npub;

    let pk = MENTIONED_PK;
    let npub = encode_npub(pk).expect("encode_npub must succeed");

    // First, record via the p-tag path.
    kernel.collect_unknown_refs(&[tag(&["p", pk])]);
    let len_after_ptag = kernel.unknown_ids.pending_len();

    // Then scan content that mentions the same pubkey.
    let content = format!("nostr:{npub}");
    kernel.collect_content_mention_pubkeys(&content);

    assert_eq!(
        kernel.unknown_ids.pending_len(),
        len_after_ptag,
        "V-56: content mention of a pubkey already pending via p-tag must not \
         increase the UnknownIds pending count (dedup)"
    );
}

/// V-56 known profile: a pubkey already in the `profiles` projection must not
/// be re-added (D8 dedup gate in `note_pubkey`).
#[test]
fn v56_known_profile_not_re_added() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    use nmp_nostr_id::encode_npub;

    let pk = MENTIONED_PK;
    let npub = encode_npub(pk).expect("encode_npub must succeed");

    // Pre-seed the profile cache so the pubkey is "known".
    kernel.seed_profile_view_for_test(pk, crate::substrate::ProfileView::default());

    let content = format!("Say hello to nostr:{npub}");
    kernel.collect_content_mention_pubkeys(&content);

    assert_eq!(
        kernel.unknown_ids.pending_len(),
        0,
        "V-56: a pubkey already in profiles must not be added to UnknownIds \
         (D8 dedup guard in note_pubkey)"
    );
}
