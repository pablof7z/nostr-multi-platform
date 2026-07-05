//! #3057 ROUND 4 — the UNTESTED delivery link.
//!
//! Rounds 2-3 injected at the `MarmotProjection` / `ingest_signed_event_core`
//! level, proving ingest LOGIC. NONE tested the step BEFORE that: a kind:1059
//! gift-wrap arriving from the kernel's ingest dispatcher actually reaching
//! `MarmotIngestParser` and being reconstructed + triaged + processed.
//!
//! This drives the REAL parser-dispatch wiring: register the production
//! `MarmotIngestParser` into a real [`EventIngestDispatcher`] (exactly as
//! `crate::install` does), then call `dispatch_at_source` with a genuine
//! kind:1059 Welcome — the exact call the kernel makes from
//! `project_accepted_event` / `ingest_pre_verified_event`. The parser must
//! reconstruct the signed event from `VerifiedEvent::raw()`, unwrap it, triage
//! the inner rumor as kind:444, `process_welcome` it, and surface it in the
//! Marmot snapshot's `pending_welcomes`.
//!
//! If this FAILS, the delivery link (dispatcher → parser → ingest) is broken —
//! that is the on-device S51 symptom (Welcome lands on the relay, climbing
//! event count, but never surfaces and no error fires). If it PASSES, the
//! kernel→parser wiring is sound and any residual failure is upstream in the
//! live subscription FILTER / relay delivery.

use std::sync::Arc;

use mdk_core::prelude::NostrGroupConfigData;
use nmp_core::substrate::{EventIngestDispatcher, IngestParser};
use nmp_store::{RawEvent, VerifiedEvent};
use nostr::{Keys, Kind};

use crate::projection::state::MarmotProjection;
use crate::projection::tap::MarmotIngestParser;
use crate::runtime::MarmotRuntime;

use super::fixtures::{in_memory_service, test_relays};

fn raw_of(event: &nostr::Event) -> RawEvent {
    RawEvent {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        kind: u32::from(event.kind.as_u16()),
        tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: event.content.clone(),
        sig: event.sig.to_string(),
    }
}

/// THE ROUND-4 DELIVERY TEST — a kind:1059 Welcome dispatched through the real
/// `EventIngestDispatcher` (as the kernel does) must reach `process_welcome`
/// and populate `pending_welcomes`.
///
/// This exercises the link rounds 2-3 skipped: the kernel-side parser dispatch
/// (`dispatch_at_source` → `MarmotIngestParser::parse_at_source` →
/// `parse_at` → `ingest_signed_event_core` → `ingest_giftwrap`), INCLUDING the
/// parser's `VerifiedEvent::raw()` → JSON → `nostr::Event` reconstruction that
/// the direct-injection tests bypassed.
#[test]
fn kind1059_dispatched_through_the_real_ingest_dispatcher_reaches_pending_welcomes() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice = in_memory_service(alice_keys.clone());
    // Bob's service publishes his KeyPackage (private key in this store) AND is
    // wrapped by the projection the parser drives — the production shape.
    let bob = in_memory_service(bob_keys.clone());
    let bob_kp = bob.publish_key_package(test_relays()).expect("bob kp");

    // Alice invites Bob and gift-wraps the Welcome (real nmp_nip59 path).
    let config = NostrGroupConfigData::new(
        "Round4 Delivery".to_string(),
        "round4".to_string(),
        None,
        None,
        None,
        test_relays(),
        vec![alice_keys.public_key()],
    );
    let (_g, pending) = alice
        .create_group(vec![bob_kp.event_30443.clone()], config)
        .expect("alice creates group");
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), pending.welcome_rumors[0].clone())
        .expect("alice gift-wraps welcome");
    assert_eq!(gift.kind, Kind::GiftWrap);
    pending.commit().expect("alice merges create commit");

    // Build Bob's projection + runtime + the PRODUCTION ingest parser, wired
    // into a real dispatcher exactly as `crate::install` does.
    let bob_proj = Arc::new(MarmotProjection::new(bob, None));
    let runtime = MarmotRuntime::from_projection_for_tests(Arc::clone(&bob_proj));
    let parser = Arc::new(MarmotIngestParser::new(runtime)) as Arc<dyn IngestParser>;

    let mut dispatcher = EventIngestDispatcher::new();
    dispatcher.register_kind(u32::from(gift.kind.as_u16()), Arc::clone(&parser));

    // The exact kernel call: hand the parser a VERIFIED kind:1059 with relay
    // provenance. try_from_raw runs real Schnorr verification (the gift-wrap is
    // genuinely signed by its ephemeral key), so this is not a bypass.
    let verified = VerifiedEvent::try_from_raw(raw_of(&gift)).expect("gift-wrap verifies");
    dispatcher.dispatch_at_source(&verified, 1_000, Some("wss://nos.lol"));

    // The load-bearing assertion: the Welcome flowed dispatcher → parser →
    // reconstruct → unwrap → triage(444) → process_welcome → cache → snapshot.
    let snap = bob_proj.snapshot(1_001);
    assert_eq!(
        snap.pending_welcomes.len(),
        1,
        "a kind:1059 Welcome dispatched through the real EventIngestDispatcher \
         must surface as a pending Welcome; got: {:?}. If this is empty, the \
         delivery link (dispatcher → MarmotIngestParser → ingest) is the S51 \
         bug — the Welcome never reaches process_welcome.",
        snap.pending_welcomes
    );
    assert_eq!(
        snap.pending_welcomes[0].id_hex,
        gift.id.to_hex(),
        "pending Welcome keyed by the gift-wrap id"
    );
    // And no spurious error banner on the happy path.
    assert!(
        snap.last_op_error.is_none(),
        "a successfully ingested Welcome must not raise an error banner: {:?}",
        snap.last_op_error
    );
}
