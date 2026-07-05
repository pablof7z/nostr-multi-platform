//! #3057 ROUND 2 — the FULL invitee-side ingest path, isolated from
//! subscription/relay delivery.
//!
//! The #3058 fix corrected WHICH relay the gift-wrap inbox interest routes to
//! (kind:10050 DM relays, not kind:10002 read relays). A controlled live
//! reverify proved the Welcome now lands on the right relay and the client is
//! connected to it — yet the invitee's `pendingWelcomes` STILL stayed empty.
//! So the failure is deeper than relay selection: it is in the CLIENT-SIDE
//! ingest of a delivered kind:1059 gift-wrap.
//!
//! This module isolates exactly that: it takes a REAL kind:1059 Marmot Welcome
//! gift-wrap (built through the real `nmp_nip59` seal/wrap path) addressed to
//! Bob, and drives it straight into Bob's `MarmotProjection` through
//! `ops::ingest_signed_event_core` — the EXACT call
//! `crate::projection::tap::MarmotIngestParser::parse_at` makes for every
//! accepted inbound signed event. No relay, no subscription, no kernel: if the
//! Welcome is handed to the ingest handler and `pendingWelcomes` still does not
//! populate, the bug is client-side ingest (this module reproduces it); if it
//! DOES populate, the bug is upstream in subscription/relay delivery.

use mdk_core::prelude::NostrGroupConfigData;
use nostr::{EventBuilder, Keys, Kind};

use crate::projection::ops;
use crate::projection::state::MarmotProjection;
use crate::service::MarmotService;

use super::fixtures::{in_memory_service, test_relays};

/// Build a real kind:1059 Marmot Welcome gift-wrap addressed to `bob`, created
/// by `alice` inviting `bob` — the exact wire artifact the kernel would
/// deliver to bob's ingest parser after `alice` calls `create_group`.
///
/// `bob` MUST be the SAME `MarmotService` that later ingests the Welcome:
/// `publish_key_package` stores the KeyPackage's PRIVATE key material in bob's
/// MLS key store, and `process_welcome` looks that private key up in the SAME
/// store to decrypt the group secret. Modeling this with two separate bob
/// stores would be a test artifact, not the production path (in production
/// bob's persistent SQLite MLS store spans both publish and receive).
fn alice_welcome_giftwrap_for_bob(alice: &MarmotService, bob: &MarmotService) -> nostr::Event {
    // Bob publishes a KeyPackage (kind:30443) through his OWN service so the
    // matching private key lands in his key store.
    let bob_kp = bob
        .publish_key_package(test_relays())
        .expect("bob key package");

    let config = NostrGroupConfigData::new(
        "Marmot Ingest Test".to_string(),
        "welcome-ingest".to_string(),
        None,
        None,
        None,
        test_relays(),
        vec![alice.public_key()],
    );
    let (_group, pending) = alice
        .create_group(vec![bob_kp.event_30443.clone()], config)
        .expect("alice creates group");
    assert_eq!(pending.welcome_rumors.len(), 1, "one welcome for Bob");
    let rumor = pending.welcome_rumors[0].clone();

    // Real NIP-59 gift-wrap of the Welcome addressed to Bob.
    let gift = alice
        .wrap_welcome(&bob.public_key(), rumor)
        .expect("alice gift-wraps welcome");
    assert_eq!(gift.kind, Kind::GiftWrap, "must be a kind:1059 gift-wrap");
    pending.commit().expect("alice merges create commit");
    gift
}

/// DECISIVE #3057 diagnostic: a real kind:1059 Welcome gift-wrap handed to
/// Bob's ingest handler MUST populate `pendingWelcomes` in the very next
/// snapshot.
///
/// This drives `ops::ingest_signed_event_core` — the exact entry point
/// `MarmotIngestParser::parse_at` calls — with a gift-wrap addressed to Bob,
/// then reads Bob's projection snapshot. If `pending_welcomes` is empty after
/// this, the client-side ingest path itself is broken (independent of any
/// relay/subscription question the #3058 fix addressed).
#[test]
fn ingesting_a_real_welcome_giftwrap_populates_pending_welcomes() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice = in_memory_service(alice_keys.clone());
    // Bob's service — publishes his KeyPackage AND (wrapped in the projection)
    // ingests the Welcome, so the KeyPackage private key is in the same store.
    let bob = in_memory_service(bob_keys.clone());
    let gift = alice_welcome_giftwrap_for_bob(&alice, &bob);

    // Bob's projection wraps that SAME service. Drive the gift-wrap straight
    // into the ingest handler, exactly as the kernel's ingest parser would.
    let bob_proj = MarmotProjection::new(bob, None);
    let ingest = bob_proj
        .with_inner(|h| ops::ingest_signed_event_core(h, &gift, 1_000))
        .expect("projection lock available");

    // The ingest MUST NOT silently swallow an error (D6-surface): if it fails,
    // that failure is itself the bug and must be visible, not hidden.
    let ingest = ingest.expect("ingesting bob's Welcome gift-wrap must not error");
    assert!(
        ingest.is_some(),
        "kind:1059 ingest must return a payload, not a silent skip: {ingest:?}"
    );

    // The load-bearing assertion: Bob's next snapshot surfaces the pending
    // Welcome. This is what the Chirp InvitesView / pendingWelcomes reads.
    let snap = bob_proj.snapshot(1_001);
    assert_eq!(
        snap.pending_welcomes.len(),
        1,
        "Bob's snapshot must surface exactly one pending Welcome after \
         ingesting the delivered kind:1059 gift-wrap; got: {:?}",
        snap.pending_welcomes
    );
    let row = &snap.pending_welcomes[0];
    assert_eq!(row.id_hex, gift.id.to_hex(), "row keyed by gift-wrap id");
    assert_eq!(
        row.inviter_npub,
        alice_keys.public_key().to_hex(),
        "pending Welcome must attribute the inviter (Alice)"
    );
}

/// #3057 ROUND 2 — a delivered kind:444 Welcome that Bob CANNOT process must
/// SURFACE an error, never vanish.
///
/// Reproduces the production black hole: a genuine Marmot Welcome addressed to
/// Bob is delivered and unwraps cleanly (it IS his — decrypts with his key,
/// inner rumor is kind:444), but MDK's `process_welcome` fails because the
/// matching KeyPackage private key is not in Bob's MLS key store ("No matching
/// key package was found in the key store"). This is the shape of the on-device
/// failure the controlled reverify hit: the Welcome reaches the ingest handler
/// but is dropped.
///
/// Pre-#3057-round-2, `ingest_signed_event_core` returned `Err` and the tap
/// SILENTLY SWALLOWED it — so `pendingWelcomes` stayed empty AND
/// `last_op_error` stayed `None` (no invite, no error: a black hole). The fix
/// records the failure to the snapshot-visible `last_op_error` banner. This
/// test asserts that banner is populated — it FAILS on master (banner `None`)
/// and PASSES with the fix.
#[test]
fn a_welcome_that_fails_to_process_surfaces_last_op_error_not_a_silent_swallow() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice = in_memory_service(alice_keys.clone());
    // Bob PUBLISHES his KeyPackage from a throwaway store — so the matching
    // private key lands THERE, not in the projection store below. This models
    // the production failure where Bob's welcome-ingest store lacks the private
    // key for the KeyPackage Alice invited with (rotation / store mismatch /
    // single-use consumption). `alice_welcome_giftwrap_for_bob` uses this
    // publisher's KP, so the resulting Welcome is genuinely for Bob's identity
    // yet unprocessable by his projection store.
    let bob_publisher = in_memory_service(bob_keys.clone());
    let gift = alice_welcome_giftwrap_for_bob(&alice, &bob_publisher);

    // Bob's projection uses a SEPARATE store (same identity keys). Unwrap will
    // succeed (his key), the rumor IS a kind:444 Welcome, but process_welcome
    // fails: the KeyPackage private key isn't in THIS store.
    let bob_proj = MarmotProjection::new(in_memory_service(bob_keys.clone()), None);
    let ingest = bob_proj
        .with_inner(|h| ops::ingest_signed_event_core(h, &gift, 4_242))
        .expect("projection lock available");

    // The ingest returns Err (a genuine, un-swallowed Welcome failure)…
    let err = ingest.expect_err("an unprocessable Welcome must return Err, not Ok/skip");
    assert!(
        err.to_lowercase().contains("key package") || err.to_lowercase().contains("welcome"),
        "the surfaced error must name the real MDK failure; got: {err}"
    );

    // …and no phantom pending welcome was cached.
    let snap = bob_proj.snapshot(4_243);
    assert!(
        snap.pending_welcomes.is_empty(),
        "a Welcome that failed to process must NOT appear as pending: {:?}",
        snap.pending_welcomes
    );

    // THE LOAD-BEARING #3057 ASSERTION: the failure is visible in the snapshot
    // banner, not swallowed. On master this is `None` (silent swallow).
    let banner = snap
        .last_op_error
        .expect("a dropped Welcome MUST surface a last_op_error banner (#3057), not vanish");
    assert_eq!(
        banner.op, "welcome_ingest",
        "banner must attribute the failure to Welcome ingest"
    );
    assert_eq!(
        banner.correlation_id,
        gift.id.to_hex(),
        "banner correlation id must be the gift-wrap event id (retry handle)"
    );
}

/// The fix must NOT over-surface: a kind:1059 gift-wrap whose inner rumor is
/// NOT a Marmot Welcome (e.g. a NIP-17 DM sharing the kind:1059 envelope) is a
/// deliberate silent skip — the sibling `nip17.dm_inbox` parser owns it. A
/// Marmot "not a welcome" is NOT a Marmot error and must never raise the
/// `last_op_error` banner (otherwise every DM would spam it — the exact
/// conflation the #3057 triage removes).
#[test]
fn a_non_welcome_giftwrap_is_silently_skipped_without_a_banner() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice = in_memory_service(alice_keys.clone());

    // A gift-wrap whose inner rumor is a NIP-17-style DM (kind:14), NOT a
    // kind:444 Welcome. `wrap_welcome` is just the NIP-59 gift-wrap primitive;
    // here we hand it a non-Welcome rumor to model a DM addressed to Bob.
    let dm_rumor = EventBuilder::new(Kind::Custom(14), "hey bob").build(alice_keys.public_key());
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), dm_rumor)
        .expect("gift-wrap the DM rumor to bob");

    let bob_proj = MarmotProjection::new(in_memory_service(bob_keys.clone()), None);
    let ingest = bob_proj
        .with_inner(|h| ops::ingest_signed_event_core(h, &gift, 5_000))
        .expect("projection lock available")
        .expect("a non-Welcome gift-wrap must not error");

    assert!(
        ingest.is_none(),
        "a non-Welcome (kind != 444) gift-wrap must be a silent skip (Ok(None)); got: {ingest:?}"
    );

    let snap = bob_proj.snapshot(5_001);
    assert!(
        snap.pending_welcomes.is_empty(),
        "a DM must not become a pending Welcome"
    );
    assert!(
        snap.last_op_error.is_none(),
        "a non-Welcome gift-wrap must NOT raise the error banner (no DM spam); got: {:?}",
        snap.last_op_error
    );
}
