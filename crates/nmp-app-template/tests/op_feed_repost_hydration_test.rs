//! V-83 — `register_op_feed_defaults`'s `event_lookup` reads the REAL kernel
//! event store (replacing the prior `|_| None` no-op).
//!
//! These tests prove the production wiring: the engine's `event_lookup` now
//! resolves a parent/target/wrapper event the engine has not yet observed but
//! the kernel has cached, via `NmpApp::event_by_id` over the kernel's published
//! `EventStore`. The repost L-2/L-5 backward-hydration paths consult that
//! lookup; without a real read they degrade — L-5 loses repost provenance, L-2
//! buffers attribution against the kind:6 wrapper forever. The engine logic
//! itself is already covered by `nmp-nip01`'s synthetic-lookup harness; here we
//! exercise the *seam*: the slot the actor publishes into and the host reads
//! through.
//!
//! We seed a `MemEventStore` directly and publish it into the slot
//! `NmpApp::event_store_handle()` exposes — exactly what the actor does after
//! kernel construction, just synchronously and without spinning the relay pool.
//! (Split out of `op_feed_defaults_test.rs` to keep both files under the
//! 500-LOC ceiling.)

use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
use nmp_core::substrate::KernelEvent;
// `on_kernel_event` is a `KernelEventObserver` method (the engine impls it).
use nmp_core::KernelEventObserver as _;
// `AttributionPayload` brings `author_pubkey()` into scope for the L-2 assertion.
use nmp_feed::AttributionPayload as _;
use nmp_ffi::{nmp_app_free, nmp_app_new, NmpApp};

// Valid-looking 64-hex pubkeys (distinct), mirroring the rung-4/rung-5 idioms.
const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const CAROL: &str = "cccc000000000000000000000000000000000000000000000000000000000003";

// 64-hex event ids so the nevent encoder (32-byte TLV) accepts them.
const OP_ID: &str = "0000000000000000000000000000000000000000000000000000000000000abc";
const REPLY_ID: &str = "0000000000000000000000000000000000000000000000000000000000000de1";
// kind:6 repost wrapper id.
const REPOST_ID: &str = "0000000000000000000000000000000000000000000000000000000000000f06";
// A 128-char dummy Schnorr sig — `EventStore::insert` only checks
// `sig.len() == 128` structurally (we insert via `from_raw_unchecked`, which
// bypasses cryptographic verification; the real signature is irrelevant to the
// event-by-id read path V-83 exercises).
const DUMMY_SIG: &str = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
// `EventStore::insert`'s `is_structurally_valid` requires `sig.len() == 128`.
const _: () = assert!(DUMMY_SIG.len() == 128, "dummy sig must be 128 hex chars");

// ─── Event builders ──────────────────────────────────────────────────────────

fn op_event(id: &str, author: &str, created_at: u64, body: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: Vec::new(),
        content: body.to_string(),
    }
}

/// A NIP-10 reply whose single `e` reply-marker points at `parent_id` (L-2:
/// the parent is a kind:6 wrapper the engine must resolve). Mirrors the
/// `reply_to_parent` builder in `nmp-nip01`'s op-feed harness.
fn reply_to_parent(id: &str, author: &str, created_at: u64, parent_id: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: vec![vec![
            "e".to_string(),
            parent_id.to_string(),
            String::new(),
            "reply".to_string(),
        ]],
        content: "reply to a repost".to_string(),
    }
}

/// A kind:6 e-tag-only repost wrapper of `target`, as a store `RawEvent`.
fn repost_raw(id: &str, author: &str, created_at: u64, target: &str) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind: 6,
        tags: vec![vec!["e".to_string(), target.to_string()]],
        content: String::new(),
        sig: DUMMY_SIG.to_string(),
    }
}

/// The same kind:6 repost wrapper as a `KernelEvent` (what the engine observes
/// on the fan-out). Mirrors `repost_raw` field-for-field so the store read and
/// the observed event agree.
fn repost_kernel_event(id: &str, author: &str, created_at: u64, target: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: vec![vec!["e".to_string(), target.to_string()]],
        content: String::new(),
    }
}

fn slot(active: Option<&str>) -> ActiveAccountSlot {
    Arc::new(Mutex::new(active.map(str::to_string)))
}

/// Build a `MemEventStore` seeded with `events` and publish it into the app's
/// event-store slot — the synchronous, in-test equivalent of the actor's
/// publish-back after kernel construction.
fn publish_store_with(app: *mut NmpApp, events: &[RawEvent]) {
    let store = Arc::new(MemEventStore::new());
    for raw in events {
        let outcome = store
            .insert(
                VerifiedEvent::from_raw_unchecked(raw.clone()),
                &"wss://test/".to_string(),
                1_000_000,
            )
            .expect("seed store insert");
        assert!(
            matches!(
                outcome,
                nmp_core::store::InsertOutcome::Inserted { .. }
                    | nmp_core::store::InsertOutcome::Replaced { .. }
            ),
            "seed event {} must land in the store, got {outcome:?}",
            raw.id
        );
    }
    // SAFETY: `app` is a valid non-null pointer for the duration of the test.
    let handle = unsafe { &*app }.event_store_handle();
    *handle.lock().expect("event-store slot lock") = Some(store);
}

#[test]
fn repost_l5_backward_hydration_resolves_wrapper_via_real_event_lookup() {
    // L-5: an e-tag-only kind:6 repost (CAROL reposts BOB's OP) is observed
    // first — a placeholder card keyed by the target id, repost provenance
    // recorded against the wrapper id. The target kind:1 arrives LATER; the
    // engine re-fetches the WRAPPER via `event_lookup(wrapper_event_id)` and
    // rebuilds the card from the `(wrapper, target)` pair so the "reposted by"
    // banner survives. With the prior `|_| None` no-op the rebuild would use
    // `(target, None)` and LOSE the provenance — that is the regression these
    // assertions guard.
    let app = nmp_app_new();
    assert!(!app.is_null());

    // The wrapper is in the kernel store (the engine has observed it via the
    // fan-out below; the store is the canonical copy `event_lookup` reads).
    publish_store_with(app, &[repost_raw(REPOST_ID, CAROL, 20, OP_ID)]);

    // ALICE is the active account (self-included → a follow); CAROL's repost is
    // attributed regardless of follow because reposts surface the target root.
    // SAFETY: valid non-null pointer.
    let engine = nmp_app_template::register_op_feed_defaults(
        unsafe { &*app },
        ALICE.to_string(),
        slot(Some(ALICE)),
    )
    .engine;

    // 1. The repost wrapper arrives (fan-out). Placeholder keyed by the target,
    //    provenance recorded, target not local yet → claim emitted.
    engine.on_kernel_event(&repost_kernel_event(REPOST_ID, CAROL, 20, OP_ID));
    let before = engine.snapshot(&nmp_feed::FeedRequest::default());
    assert_eq!(before.cards.len(), 1, "placeholder card keyed by target");
    assert_eq!(before.cards[0].card.id, OP_ID);
    assert!(
        before.cards[0].card.content.is_empty(),
        "placeholder body before the target arrives"
    );

    // 2. The target kind:1 arrives. `ingest_root` calls `event_lookup(REPOST_ID)`
    //    → the real store read resolves the wrapper → card rebuilt from the pair.
    let target = op_event(OP_ID, BOB, 9, "the real body");
    engine.on_kernel_event(&target);

    let after = engine.snapshot(&nmp_feed::FeedRequest::default());
    assert_eq!(after.cards.len(), 1, "target surfaces once");
    let card = &after.cards[0];
    assert_eq!(card.card.id, OP_ID);
    assert_eq!(card.card.content, "the real body", "body hydrated (L-5)");
    // THE V-83 DISCRIMINATOR: provenance preserved only because `event_lookup`
    // resolved the wrapper. The prior no-op would leave `reposted_by == None`.
    let reposted = card
        .card
        .reposted_by
        .as_ref()
        .expect("repost provenance preserved via the real event_lookup (L-5)");
    assert_eq!(
        reposted.author_pubkey, CAROL,
        "the rebuilt card carries the reposter from the resolved wrapper"
    );

    nmp_app_free(app);
}

#[test]
fn repost_l2_reply_to_kind6_wrapper_rekeys_via_real_event_lookup() {
    // L-2: a reply targets a kind:6 repost wrapper that the kernel already holds.
    // `ingest_reply` calls `event_lookup(wrapper_id)`; the real store read
    // resolves the wrapper, the engine sees it `supersedes` a different target,
    // and re-keys the attribution onto that TARGET (not the wrapper). With the
    // prior no-op the wrapper would never resolve, so the attribution would
    // buffer against the kind:6 wrapper id — which never becomes a root — and
    // the OP would surface with ZERO attribution.
    let app = nmp_app_new();
    assert!(!app.is_null());

    // Both the wrapper AND the target OP are in the kernel store, so the re-keyed
    // attribution attaches immediately.
    publish_store_with(
        app,
        &[
            repost_raw(REPOST_ID, CAROL, 19, OP_ID),
            RawEvent {
                id: OP_ID.to_string(),
                pubkey: BOB.to_string(),
                created_at: 9,
                kind: 1,
                tags: Vec::new(),
                content: "Bob's original".to_string(),
                sig: DUMMY_SIG.to_string(),
            },
        ],
    );

    // SAFETY: valid non-null pointer.
    let engine = nmp_app_template::register_op_feed_defaults(
        unsafe { &*app },
        ALICE.to_string(),
        slot(Some(ALICE)),
    )
    .engine;

    // The target OP is observed (so it is a live root) and the wrapper too.
    engine.on_kernel_event(&op_event(OP_ID, BOB, 9, "Bob's original"));
    engine.on_kernel_event(&repost_kernel_event(REPOST_ID, CAROL, 19, OP_ID));

    // ALICE (self-included follow) replies to the WRAPPER id, not the OP id.
    engine.on_kernel_event(&reply_to_parent(REPLY_ID, ALICE, 21, REPOST_ID));

    let snap = engine.snapshot(&nmp_feed::FeedRequest::default());
    let target_card = snap
        .cards
        .iter()
        .find(|c| c.card.id == OP_ID)
        .expect("target card present");
    assert_eq!(
        target_card.attribution.len(),
        1,
        "attribution re-keyed onto the wrapped target via the real event_lookup (L-2)"
    );
    assert_eq!(target_card.attribution[0].author_pubkey(), ALICE);

    nmp_app_free(app);
}

#[test]
fn event_lookup_is_correctness_preserving_before_store_published() {
    // Cold-start guard: before the actor publishes the store (the slot is empty,
    // exactly the pre-`nmp_app_start` state), `event_by_id` returns `None` — the
    // same behaviour as the prior no-op closure. The engine must still function:
    // the L-5 placeholder shows, and the wrapper-less rebuild simply omits
    // provenance until a real read is available. This locks in the
    // "correctness-preserving" contract the BACKLOG V-83 entry highlighted.
    let app = nmp_app_new();
    assert!(!app.is_null());

    // Slot deliberately NOT published.
    // SAFETY: valid non-null pointer.
    let app_ref = unsafe { &*app };
    assert!(
        app_ref.event_by_id(OP_ID).is_none(),
        "empty slot → None (matches the prior no-op)"
    );

    let engine =
        nmp_app_template::register_op_feed_defaults(app_ref, ALICE.to_string(), slot(Some(ALICE)))
            .engine;

    // Drive the L-5 sequence with no published store: placeholder, then hydrate
    // body — but provenance is absent (no wrapper read possible), and crucially
    // nothing panics.
    engine.on_kernel_event(&repost_kernel_event(REPOST_ID, CAROL, 20, OP_ID));
    engine.on_kernel_event(&op_event(OP_ID, BOB, 9, "the real body"));
    let snap = engine.snapshot(&nmp_feed::FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card.id, OP_ID);
    assert_eq!(
        snap.cards[0].card.content, "the real body",
        "body still hydrates from the observed target even without a store read"
    );
    assert!(
        snap.cards[0].card.reposted_by.is_none(),
        "no store read → no wrapper resolution → provenance omitted (degrades, never panics)"
    );

    nmp_app_free(app);
}
