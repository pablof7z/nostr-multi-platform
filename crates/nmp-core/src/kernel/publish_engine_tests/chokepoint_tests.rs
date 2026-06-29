#![cfg(test)]
//! D10 chokepoint tests — private-envelope (gift-wrap / kind:1059) routing.
//!
//! T-publish-resolver-indexer: fail-closed semantics for unroutable authors and
//! private envelopes. Covers:
//!   - No kind:10002 → NoTargets, zero outbound frames.
//!   - gift-wrap + Auto target → refused (D10 gate).
//!   - gift-wrap + Explicit relay → allowed (fail-closed ≠ no-DM-publish).
//!   - BLOCKER #1 residual: persisted private rows dropped on resume and retry
//!     when their relay is public/non-explicit.

use std::sync::Arc;

use crate::kernel::Kernel;
use crate::publish::PublishStore;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

use super::{fake_signed, ok_payload, persist_pending_record, seed_kind10002, WRITE_R1, WRITE_R2};

// ─── T-publish-resolver-indexer: fail-closed for unroutable authors ──────────
//
// Pins the new `NoTargets` semantics: an author with no kind:10002 must not
// silently publish to arbitrary public relays. The engine surfaces `NoTargets`
// so the UI can show "no relay to publish to" rather than a silent failure.
// Mirrors T134's subscription-side `unroutable_authors` discipline.

#[test]
fn t_publish_resolver_unroutable_author_no_kind10002_produces_no_targets() {
    // An author with no kind:10002 in the store must produce ZERO outbound
    // frames and a `RecentFailure` row on the publish-status snapshot.
    // (Previously the old indexer-fallback would produce 2 frames destined
    // for arbitrary public relays; that path is removed per codex f81f735.)
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Intentionally do NOT seed kind:10002 for this author.
    let signed = fake_signed(
        "ab".repeat(32).as_str(),
        "cd".repeat(32).as_str(),
        1,
        "unroutable author publish test",
    );
    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert!(
        outbound.is_empty(),
        "author with no kind:10002 must produce zero outbound frames (NoTargets, fail-closed); \
         got {} frames targeting: {:?}",
        outbound.len(),
        outbound.iter().map(|m| &m.relay_url).collect::<Vec<_>>()
    );

    // The engine must surface the failure visibly — a `RecentFailure` row
    // on the snapshot (D6: errors never cross FFI silently).
    let snap = kernel.publish_status_snapshot();
    assert!(
        !snap.recent_errors.is_empty(),
        "unroutable publish must record a RecentFailure (D6 — no silent drop)"
    );
}

/// Workstream C structural chokepoint (BLOCKER #1). The publish engine entry
/// `run_publish_engine_at` is the single door EVERY signed publish funnels
/// through — so the D10 private-envelope gate enforced there closes the leak on
/// every path. A kind:1059 gift-wrap with `Auto` (even with the author's
/// kind:10002 write relays seeded, which WOULD route a public kind) must emit
/// ZERO outbound frames and set a D10 toast: the encrypted envelope never
/// reaches the public outbox.
#[test]
fn chokepoint_refuses_gift_wrap_with_auto_target() {
    let author = "33".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Seed write relays so the ONLY reason for zero frames is the D10 gate
    // (a public kind:1 with this seed routes to two relays — see bullet 1).
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("44".repeat(32).as_str(), &author, 1059, "encrypted-envelope");
    let outbound = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        None,
        1_000,
    );
    assert!(
        outbound.is_empty(),
        "gift-wrap + Auto must produce NO outbound frames (D10); got {outbound:?}"
    );
    let toast = kernel
        .last_error_toast_snapshot()
        .cloned()
        .expect("a D10 refusal must set an error toast");
    assert!(
        toast.contains("D10") && toast.contains("1059"),
        "toast must cite D10 + the kind; got: {toast}"
    );
}

/// The same chokepoint ALLOWS a gift-wrap pinned to an explicit non-empty
/// recipient-inbox relay set — fail-closed means "no Auto", not "no publish".
#[test]
fn chokepoint_allows_gift_wrap_with_explicit_relays() {
    let author = "55".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed = fake_signed("66".repeat(32).as_str(), &author, 1059, "encrypted-envelope");
    let outbound = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::explicit(
            vec![WRITE_R1.to_string()],
            crate::publish::PublishRouteClass::VerifiedPrivateInbox,
        ),
        None,
        1_000,
    );
    assert_eq!(
        outbound.len(),
        1,
        "gift-wrap with an explicit relay pin must route to exactly that relay"
    );
    assert_eq!(outbound[0].relay_url, WRITE_R1);
}

// ── BLOCKER #1 (residual) — fail-closed at the UNIVERSAL dispatch-emit site ──
//
// The entry gate (`run_publish_engine_at`) only covers INITIAL publish. The
// engine's `dispatch_due` loop is the single point ALL emit paths converge on
// — initial publish, resume-from-store on restart, and manual/availability
// retry — so the D10 fail-closed enforcement lives there. These tests prove a
// PERSISTED private row targeting a public/non-explicit relay is dropped on
// BOTH resume and retry, while a legitimately-Explicit private row still flows.

#[test]
fn resume_from_store_drops_persisted_gift_wrap_to_public_relay() {
    // TEST (a): a persisted kind:1059 row targeting a PUBLIC (AuthorWriteRelay)
    // relay must NOT emit on cold-restart replay — the universal dispatch gate
    // fails closed even though the row never went through the entry gate.
    use crate::publish::InMemoryPublishStore;
    let publish_store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let author = "a1".repeat(32);
    let signed = fake_signed("b1".repeat(32).as_str(), &author, 1059, "leaked-envelope");
    persist_pending_record(
        &publish_store,
        &signed,
        WRITE_R1,
        crate::publish::RelaySelectionReason::AuthorWriteRelay,
    );

    let mut kernel = Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&publish_store));
    let resumed = kernel.resume_publish_engine();
    assert!(
        resumed.is_empty(),
        "resume must NOT emit a private envelope to a non-explicit relay (D10); got {resumed:?}"
    );

    // SHOULD-FIX (no-debt): the refused row must be terminally finalized and
    // REMOVED from the durable store on this resume — not left Pending. If it
    // lingered, every restart-before-tick would re-refuse it forever.
    assert!(
        publish_store
            .load_pending()
            .expect("load_pending")
            .is_empty(),
        "a refused private row must be settled + deleted from the store, not left pending"
    );

    // And a SECOND resume (fresh kernel, same store) must have nothing to
    // re-attempt — proving the row was settled exactly once, not re-refused.
    let mut kernel_b =
        Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&publish_store));
    let resumed_again = kernel_b.resume_publish_engine();
    assert!(
        resumed_again.is_empty(),
        "second resume must find no lingering row to re-refuse; got {resumed_again:?}"
    );
}

#[test]
fn manual_retry_does_not_resurrect_refused_gift_wrap_to_public_relay() {
    // TEST (b): the user-driven retry path must never emit a private envelope to
    // a non-explicit relay. Resume both refuses the public-targeted kind:1059
    // row (D10 gate) AND finalizes it, so a subsequent manual retry has nothing
    // to resurrect — it emits nothing. (The retry DISPATCH gate itself is also
    // exercised positively in test (c)'s `_and_retry_` assertion.)
    use crate::publish::InMemoryPublishStore;
    let publish_store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let author = "a2".repeat(32);
    let signed = fake_signed("b2".repeat(32).as_str(), &author, 1059, "leaked-envelope");
    persist_pending_record(
        &publish_store,
        &signed,
        WRITE_R1,
        crate::publish::RelaySelectionReason::AuthorWriteRelay,
    );

    let mut kernel = Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&publish_store));
    // Resume refuses the relay (D10) and finalizes + deletes the row.
    let _ = kernel.resume_publish_engine();
    let retried = kernel.retry_publish_now(&signed.id);
    assert!(
        retried.is_empty(),
        "manual retry must NOT emit a private envelope to a non-explicit relay (D10); got {retried:?}"
    );
}

#[test]
fn resume_and_retry_allow_persisted_gift_wrap_to_explicit_relay() {
    // TEST (c): NO false positive. A persisted kind:1059 row whose relay was an
    // explicit pin (`RelaySelectionReason::Explicit`) DOES resume to its DM-inbox
    // relay — fail-closed means "no public/Auto", not "no private publish".
    use crate::publish::InMemoryPublishStore;
    let publish_store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let author = "a3".repeat(32);
    let signed = fake_signed("b3".repeat(32).as_str(), &author, 1059, "legit-dm-envelope");
    persist_pending_record(
        &publish_store,
        &signed,
        WRITE_R1,
        crate::publish::RelaySelectionReason::Explicit {
            route_class: crate::publish::PublishRouteClass::VerifiedPrivateInbox,
        },
    );

    let mut kernel = Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&publish_store));
    let resumed = kernel.resume_publish_engine();
    assert_eq!(
        resumed.len(),
        1,
        "an explicitly-pinned private row MUST resume to its DM-inbox relay (no false positive)"
    );
    assert_eq!(resumed[0].relay_url, WRITE_R1);
    assert!(resumed[0].text.contains("EVENT"));

    // NIT #2 — also exercise the manual-RETRY path for the sanctioned relay, so
    // the test name's "_and_retry_" is earned. Drive the explicit relay to a
    // retryable state (transient OK-false → scheduled backoff), then a
    // user-driven retry MUST re-emit the private envelope to the SAME explicit
    // DM-inbox relay (the emit gate allows it because the relay is Explicit).
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, false, "io: down"), 10);
    let retried = kernel.retry_publish_now(&signed.id);
    assert_eq!(
        retried.len(),
        1,
        "manual retry of an explicitly-pinned private row MUST re-emit to its DM-inbox relay"
    );
    assert_eq!(retried[0].relay_url, WRITE_R1);
    assert!(retried[0].text.contains("EVENT"));
}
