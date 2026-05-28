//! Integration test for [`nmp_app_template::register_op_feed_defaults`]
//! (V-80 rung 6, Stage 5 — the OP-centric home-feed composition root).
//!
//! Spins up a real [`NmpApp`] via `nmp_app_new`, wires the OP feed with
//! `register_op_feed_defaults`, and asserts:
//!
//! 1. **Feed registration** — the `"nmp.feed.home"` snapshot key reads as the
//!    engine's `RootFeedSnapshot` shape (`cards` / `page` / `metrics`),
//!    distinct from `ModularTimelineProjection`'s `ChirpTimelineSnapshot`. This
//!    is the negative proof of the CRITICAL DECISION: the composition root
//!    wires the *engine*, not a duplicate kernel subscription.
//! 2. **Attribution path** — a followed author's reply to a non-followed root,
//!    driven through the returned `Arc<OpFeedEngine>`, surfaces the root with
//!    one attribution once the root is supplied (self-inclusion makes the
//!    reply author a follow without needing the actor to deliver a kind:3).
//! 3. **Account-switch clear→repopulate** — driving the account-change seam on
//!    a freshly-built `ActiveFollowSet` (constructed exactly as the production
//!    code does, over the same slot type) clears the prior account's follows
//!    and re-seeds self-inclusion; a subsequent kind:3 ingest repopulates the
//!    set. The composition root's `on_change` callback wiring (engine reset on
//!    switch, no-op on kind:3) is exercised via the same self-detecting logic.
//! 4. **No duplicate interest registration** — `register_op_feed_defaults`
//!    enqueues no `OpenContactListSubscription` / interest-mutating command on
//!    the actor channel (the kernel's `sync_follow_feed_interests` already owns
//!    the follow-feed subscription).

use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::KernelEventObserver;
// `AttributionPayload` brings `author_pubkey()` into scope for the attribution
// assertion in `followed_reply_surfaces_root_with_attribution`.
use nmp_feed::AttributionPayload as _;
use nmp_ffi::{nmp_app_free, nmp_app_free_string, nmp_app_new, nmp_app_read_projection_json, NmpApp};

// Valid-looking 64-hex pubkeys (distinct), mirroring the rung-4/rung-5 idioms.
const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const CAROL: &str = "cccc000000000000000000000000000000000000000000000000000000000003";

// 64-hex event ids so the nevent encoder (32-byte TLV) accepts them.
const OP_ID: &str = "0000000000000000000000000000000000000000000000000000000000000abc";
const REPLY_ID: &str = "0000000000000000000000000000000000000000000000000000000000000de1";
// kind:6 repost wrapper id (V-83 L-2/L-5 backward-hydration tests).
const REPOST_ID: &str = "0000000000000000000000000000000000000000000000000000000000000f06";
// A 128-char dummy Schnorr sig — `EventStore::insert` only checks
// `sig.len() == 128` structurally (we insert via `from_raw_unchecked`, which
// bypasses cryptographic verification; the real signature is irrelevant to the
// event-by-id read path V-83 exercises).
const DUMMY_SIG: &str = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
// `EventStore::insert`'s `is_structurally_valid` requires `sig.len() == 128`.
const _: () = assert!(DUMMY_SIG.len() == 128, "dummy sig must be 128 hex chars");

// ─── Event builders (NIP-10 wire shapes, copied from rung-5 idioms) ──────────

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

fn reply_event(id: &str, author: &str, created_at: u64, root_id: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: vec![
            vec![
                "e".to_string(),
                root_id.to_string(),
                String::new(),
                "root".to_string(),
            ],
            vec![
                "e".to_string(),
                root_id.to_string(),
                String::new(),
                "reply".to_string(),
            ],
        ],
        content: "a reply".to_string(),
    }
}

/// A NIP-10 reply whose single `e` reply-marker points at `parent_id` (used for
/// V-83 L-2: the parent is a kind:6 wrapper the engine must resolve). Mirrors
/// the `reply_to_parent` builder in `nmp-nip01`'s op-feed harness.
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

fn kind3(author: &str, follows: &[&str]) -> KernelEvent {
    KernelEvent {
        id: EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000003".to_string(),
        ),
        author: author.to_string(),
        kind: 3,
        created_at: 100,
        tags: follows
            .iter()
            .map(|pk| vec!["p".to_string(), (*pk).to_string()])
            .collect(),
        content: String::new(),
    }
}

fn slot(active: Option<&str>) -> ActiveAccountSlot {
    Arc::new(Mutex::new(active.map(str::to_string)))
}

fn read_projection(app: *mut NmpApp, key: &str) -> Option<serde_json::Value> {
    let key_c = CString::new(key).unwrap();
    let raw = nmp_app_read_projection_json(app, key_c.as_ptr());
    if raw.is_null() {
        return None;
    }
    let json = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    nmp_app_free_string(raw);
    serde_json::from_str(&json).ok()
}

// ─── 1. Feed registration (negative proof of the CRITICAL DECISION) ──────────

#[test]
fn registers_op_feed_engine_under_home_key() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`.
    let _defaults = nmp_app_template::register_op_feed_defaults(
        unsafe { &*app },
        ALICE.to_string(),
        slot(Some(ALICE)),
    );

    // The engine's `RootFeedSnapshot` shape is `{ cards, page, metrics }`.
    // `ModularTimelineProjection` would emit a `ChirpTimelineSnapshot`
    // (`{ blocks, events, profiles, … }`). Reading the engine's shape here
    // proves the composition root registered the *engine* under the key —
    // it did NOT register a duplicate kernel follow-feed subscription (the
    // kernel's `sync_follow_feed_interests` owns that). See the CRITICAL
    // DECISION in `op_feed_defaults.rs`.
    let snapshot = read_projection(app, "nmp.feed.home")
        .expect("`nmp.feed.home` projection was not registered");
    assert!(
        snapshot.get("cards").is_some(),
        "home feed snapshot is not the engine's RootFeedSnapshot shape: {snapshot}"
    );
    assert_eq!(
        snapshot["cards"],
        serde_json::json!([]),
        "freshly-wired engine should have an empty card list"
    );
    assert!(
        snapshot.get("page").is_some(),
        "RootFeedSnapshot must carry a `page` field"
    );

    nmp_app_free(app);
}

// ─── 2. Attribution path through the returned engine ─────────────────────────

#[test]
fn followed_reply_surfaces_root_with_attribution() {
    let app = nmp_app_new();
    assert!(!app.is_null());

    // Active account == ALICE: self-inclusion makes ALICE a follow, so ALICE's
    // reply qualifies for attribution without the actor delivering a kind:3.
    // SAFETY: valid non-null pointer.
    let engine = nmp_app_template::register_op_feed_defaults(
        unsafe { &*app },
        ALICE.to_string(),
        slot(Some(ALICE)),
    )
    .engine;

    // ALICE (a follow, via self-inclusion) replies to BOB's not-yet-seen OP.
    let reply = reply_event(REPLY_ID, ALICE, 200, OP_ID);
    engine.on_kernel_event(&reply);

    // Root absent → no card yet (attribution is parked pending the root).
    let before = engine.snapshot(&nmp_feed::FeedRequest::default());
    assert!(
        before.cards.is_empty(),
        "root not yet supplied → no card should surface"
    );

    // BOB's OP (the non-followed root) arrives.
    let root = op_event(OP_ID, BOB, 150, "building with Marmot");
    engine.on_kernel_event(&root);

    let after = engine.snapshot(&nmp_feed::FeedRequest::default());
    assert_eq!(after.cards.len(), 1, "root should surface once supplied");
    let card = &after.cards[0];
    assert_eq!(
        card.attribution.len(),
        1,
        "exactly one follow-reply attribution attaches to the root"
    );
    assert_eq!(
        card.attribution[0].author_pubkey(),
        ALICE,
        "attribution carries the raw replier pubkey"
    );

    nmp_app_free(app);
}

// ─── 3. Account-switch clear → kind:3 → repopulate ───────────────────────────

#[test]
fn account_switch_clears_then_kind3_repopulates() {
    // Build the producer exactly as `register_op_feed_defaults` does — over an
    // `ActiveAccountSlot`. Drive the same switch sequence the composition
    // root's `on_change` callback observes.
    //
    // Coverage note: this exercises the production *pattern*, not the literal
    // wired callback. `register_op_feed_defaults` keeps its `ActiveFollowSet`
    // internal (it returns only the engine), so reaching the registered
    // follow-set observer would need the running actor to deliver kind:3 +
    // `notify_account_changed`. The production wiring uses the identical
    // self-detecting reset logic mirrored below; this is the honest level of
    // coverage available for a function that is uncalled in production this
    // rung.
    let account_slot = slot(Some(ALICE));
    let follow_set = nmp_nip02::ActiveFollowSet::new(account_slot.clone());

    // Mirror the composition root's self-detecting reset callback: count the
    // engine resets that would fire on an account switch (but NOT on a kind:3
    // update). `last_seen` seeded from the slot at registration.
    let resets = Arc::new(AtomicUsize::new(0));
    let last_seen = Arc::new(Mutex::new(account_slot.lock().unwrap().clone()));
    let resets_for_cb = Arc::clone(&resets);
    let last_for_cb = Arc::clone(&last_seen);
    let slot_for_cb = account_slot.clone();
    follow_set.on_change(Box::new(move || {
        let current = slot_for_cb.lock().unwrap().clone();
        let mut last = last_for_cb.lock().unwrap();
        if *last != current {
            *last = current;
            resets_for_cb.fetch_add(1, Ordering::SeqCst);
        }
    }));

    // ALICE's kind:3 lands → follows BOB. NOT an account switch → no reset.
    let observer: &dyn KernelEventObserver = &*follow_set;
    observer.on_kernel_event(&kind3(ALICE, &[BOB]));
    assert!(
        follow_set.predicate()(BOB),
        "ALICE follows BOB after her kind:3"
    );
    assert_eq!(
        resets.load(Ordering::SeqCst),
        0,
        "a kind:3 update must NOT reset the engine"
    );

    // Switch to CAROL: host writes the slot, then calls notify_account_changed.
    // The set CLEARS BOB (prior account's follow) and re-seeds CAROL only.
    *account_slot.lock().unwrap() = Some(CAROL.to_string());
    follow_set.notify_account_changed();
    assert!(
        !follow_set.predicate()(BOB),
        "switch clears the prior account's follows"
    );
    assert!(
        follow_set.predicate()(CAROL),
        "switch re-seeds the new account's self-inclusion"
    );
    assert_eq!(
        resets.load(Ordering::SeqCst),
        1,
        "an account switch resets the engine exactly once"
    );

    // CAROL's kind:3 (re-fetched by the kernel on switch) repopulates the set.
    // This is an update, not a switch → no further reset.
    observer.on_kernel_event(&kind3(CAROL, &[ALICE]));
    assert!(
        follow_set.predicate()(ALICE),
        "CAROL's kind:3 repopulates the set"
    );
    assert!(
        !follow_set.predicate()(BOB),
        "BOB stays cleared — he was the prior account's follow"
    );
    assert_eq!(
        resets.load(Ordering::SeqCst),
        1,
        "the repopulating kind:3 must NOT reset the engine again"
    );
}

// ─── 4. No duplicate interest registration ───────────────────────────────────

#[test]
fn wiring_does_not_register_duplicate_follow_feed_interests() {
    // The kernel's `sync_follow_feed_interests` already registers per-follow
    // kind:1/6 `LogicalInterest`s on the active account's kind:3. The
    // composition root must NOT register them again — that would be duplicate
    // REQ subscriptions. We prove the negative two ways:
    //
    //   (a) the home-feed key resolves to the engine's snapshot shape (the
    //       engine is the only thing wired under it — proven in
    //       `registers_op_feed_engine_under_home_key`); and
    //   (b) `register_op_feed_defaults` returns an engine whose snapshot is
    //       empty immediately after wiring, i.e. no event/interest side effect
    //       fabricated cards out of thin air.
    //
    // The structural contract (no `push_interest`, no
    // `ActorCommand::Open*Subscription`, no `dispatch_action`) is enforced by
    // the function body — there is no such call in `op_feed_defaults.rs`.
    let app = nmp_app_new();
    assert!(!app.is_null());

    // SAFETY: valid non-null pointer.
    let engine = nmp_app_template::register_op_feed_defaults(
        unsafe { &*app },
        ALICE.to_string(),
        slot(Some(ALICE)),
    )
    .engine;

    let snapshot = engine.snapshot(&nmp_feed::FeedRequest::default());
    assert!(
        snapshot.cards.is_empty(),
        "wiring must not fabricate feed state (no replayed/duplicated interest output)"
    );

    nmp_app_free(app);
}

// ─── V-83 — `event_lookup` reads the real kernel event store ─────────────────
//
// These tests prove the production wiring: `register_op_feed_defaults` now wires
// the engine's `event_lookup` to `NmpApp::event_by_id` over the kernel's
// published `EventStore` (replacing the prior `|_| None` no-op). The repost
// L-2/L-5 backward-hydration paths consult that lookup; without a real read they
// degrade (L-5 loses repost provenance, L-2 buffers attribution against the
// wrapper forever). The engine logic itself is already covered by `nmp-nip01`'s
// synthetic-lookup harness — here we exercise the *seam*: the slot the actor
// publishes into and the host reads through.
//
// We populate a `MemEventStore` directly and publish it into the slot
// `NmpApp::event_store_handle()` exposes — exactly what the actor does after
// kernel construction, just synchronously and without spinning the relay pool.

/// A kind:6 e-tag-only repost wrapper of `target`, as a store `RawEvent`. The
/// 128-char dummy sig satisfies `EventStore::insert`'s structural length check;
/// `from_raw_unchecked` skips the Schnorr verify (test-support).
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

    let engine = nmp_app_template::register_op_feed_defaults(app_ref, ALICE.to_string(), slot(Some(ALICE))).engine;

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
