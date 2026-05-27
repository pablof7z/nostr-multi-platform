//! T131 integration tests — `ingest_timeline_event` reaches the per-URL
//! `EventProvenance` counters on every `InsertOutcome` arm.
//!
//! These tests complement the unit tests in `provenance.rs` (which exercise
//! `EventProvenance` directly). They drive the **full ingest path** —
//! `try_from_raw` sig verify → `store.insert` → match arm → counter bump —
//! to catch wire-up regressions that pure unit tests can't (e.g. a future
//! refactor that re-orders the match or accidentally short-circuits the
//! `Duplicate` arm before the bump).
//!
//! Real Schnorr-signed events are used (`nostr::Keys::generate() +
//! EventBuilder::text_note + sign_with_keys`) — the same fixture pattern as
//! `lib.rs::inject_signed_events`. Sign cost is ~30–50 µs per event; the
//! suite produces a handful of events total so end-to-end runtime is in
//! single-digit milliseconds.
//!
//! Design contract: `docs/design/outbox-explorer-diagnostics.md` §3 line 188
//! — counter bump happens in the same match arms as `InsertOutcome`, no per-
//! event allocation on the hot path.

use super::nostr::NostrEvent;
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};

const RELAY_A: &str = "wss://a.example/";
const RELAY_B: &str = "wss://b.example/";

/// Build one real Schnorr-signed kind:1 event using the supplied fixture
/// key. Returns the `NostrEvent` shape the kernel ingest path consumes
/// after JSON decoding (mirrors `lib.rs::inject_signed_events`).
///
/// The kernel's `nostr.rs::NostrEvent` is module-private (`pub(super)`), so
/// the conversion lives here — building one through `VerifiedEvent::into_raw`
/// would lose the sig-verification step that's load-bearing for this test
/// (the wire-up only fires if `try_from_raw` succeeds).
///
/// `#[cfg(test)]`-only helper — the doctrine-lint walker (`walker.rs`)
/// skips D6/D8 inside test-cfg modules, so `.expect(...)` here is
/// authorised. `sign_with_keys` cannot fail with a freshly-generated
/// keypair; the `expect` is documentation, not a hot-path concern.
fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Timestamp};
    let nostr_event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    }
}

/// Pre-store a kind:1 directly via the store interface so that ingesting
/// the same id again through `ingest_timeline_event` lands as `Duplicate`.
/// Bypasses sig verify via `from_raw_unchecked` — the test asserts the
/// kernel-side wire-up, not the store's verification.
fn preload_into_store(kernel: &mut Kernel, event: &NostrEvent, relay_url: &str) {
    let raw = RawEvent {
        id: event.id.clone(),
        pubkey: event.pubkey.clone(),
        created_at: event.created_at,
        kind: event.kind,
        tags: event.tags.clone(),
        content: event.content.clone(),
        sig: event.sig.clone(),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    let _ = kernel.store.insert(verified, &relay_url.to_string(), 0);
}

#[test]
fn timeline_ingest_credits_novel_to_first_source_url() {
    // The `diag-firehose-` sub_id is the cleanest test seam: it bypasses the
    // `timeline_authors` gate so any signed kind:1 from any pubkey reaches
    // `store.insert` and the new T131 counter bump.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "first event", 1_000);

    kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", event);

    let counters = kernel
        .event_provenance
        .counters_for(RELAY_A)
        .expect("RELAY_A must be credited");
    assert_eq!(counters.novel, 1, "Inserted arm bumps novel on RELAY_A");
    assert_eq!(counters.duplicate, 0);
    assert_eq!(counters.replaced, 0);
    assert_eq!(counters.rejected, 0);
    assert!(
        kernel.event_provenance.counters_for(RELAY_B).is_none(),
        "RELAY_B must NOT have a row — it never delivered anything"
    );
}

#[test]
fn timeline_ingest_credits_duplicate_to_second_url_not_first() {
    // Determinism test for the §1 single-socket invariant: relay A is the
    // first source; relay B's redelivery is charged a duplicate WITHOUT
    // touching A's counters. This is the runtime witness for
    // `RelayUsefulness.novelty_ratio` correctness.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "shared event", 2_000);

    // Pre-load the store from RELAY_A so the kernel's first ingest goes
    // through the Inserted arm, then a second ingest from RELAY_B lands as
    // Duplicate. (Pre-loading lets us avoid the per-event Schnorr cost of
    // signing twice with the same key — same id, different relay path.)
    preload_into_store(&mut kernel, &event, RELAY_A);
    // Mirror the kernel's RELAY_A credit that the preload bypassed —
    // otherwise the assertion would be testing the preload, not the wire.
    kernel
        .event_provenance
        .record_first_source(&event.id, RELAY_A);

    // Now drive the live wire-up: same event, different relay.
    kernel.ingest_timeline_event(RelayRole::Content, RELAY_B, "diag-firehose-stress", event);

    let a = kernel
        .event_provenance
        .counters_for(RELAY_A)
        .expect("RELAY_A credit must be preserved");
    let b = kernel
        .event_provenance
        .counters_for(RELAY_B)
        .expect("RELAY_B must be charged");
    assert_eq!(a.novel, 1, "RELAY_A keeps its first-source credit");
    assert_eq!(a.duplicate, 0, "RELAY_A is NOT charged for B's redelivery");
    assert_eq!(b.novel, 0, "RELAY_B was second, not first");
    assert_eq!(b.duplicate, 1, "RELAY_B charged for one duplicate");
}

#[test]
fn replaced_arm_records_through_helper() {
    // Replaceable kinds (0, 3, 10002, 30000+) supersede on (pubkey, kind).
    // kind:1 is NOT replaceable, so to hit the `Replaced` arm we need a
    // replaceable kind. The kernel's `ingest_timeline_event` is the kind:1/6
    // path though — it only handles those two kinds (see `ingest/mod.rs:250`).
    // The Replaced arm on this specific code path is therefore unreachable
    // through the live `ingest_timeline_event` driver in production; the
    // bump exists for completeness against future kind additions. We assert
    // the counter mechanics here via direct helper call; the structural
    // wiring is reviewed at the match site (`ingest/timeline.rs:80–82`).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.event_provenance.record_replaced(RELAY_A);
    let counters = kernel
        .event_provenance
        .counters_for(RELAY_A)
        .expect("RELAY_A credited");
    assert_eq!(counters.replaced, 1);
    assert_eq!(counters.novel, 0);
}

#[test]
fn rejected_arm_records_through_helper() {
    // The `Rejected` arm at `ingest/timeline.rs:86` fires when the store
    // rejects the event (sig-fail / structural). But `ingest_timeline_event`
    // itself calls `try_from_raw` BEFORE the store — a bad sig is caught
    // there and the early `return` runs, never reaching the bump (see
    // `timeline_ingest_skips_provenance_on_pre_verify_failure` for the
    // wire-level proof of that early-return path).
    //
    // So the `Rejected` arm of the T131 wire-up is reachable only when
    // `try_from_raw` succeeds (sig is valid) but the store later rejects
    // for a different reason (NIP-40 expired, malformed content). That's a
    // narrow integration window. We assert the counter mechanics here via
    // direct helper call; the structural wiring is reviewed at the match
    // site (`ingest/timeline.rs:86–88`).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.event_provenance.record_rejected(RELAY_A);
    let counters = kernel
        .event_provenance
        .counters_for(RELAY_A)
        .expect("RELAY_A credited");
    assert_eq!(counters.rejected, 1);
    assert_eq!(counters.novel, 0);
}

#[test]
fn timeline_ingest_skips_provenance_on_pre_verify_failure() {
    // Belt-and-braces: an event that fails `try_from_raw` (bad sig) is
    // dropped BEFORE the counter bump — confirms the wire-up does not
    // double-count rejections that the verifier already filtered.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let mut event = signed_note(&keys, "tampered", 3_000);
    // Corrupt the signature so `try_from_raw` rejects.
    event.sig = "0".repeat(128);

    kernel.ingest_timeline_event(RelayRole::Content, RELAY_A, "diag-firehose-stress", event);

    assert!(
        kernel.event_provenance.counters_for(RELAY_A).is_none(),
        "verifier-rejected events must NOT create a per-URL row"
    );
}
