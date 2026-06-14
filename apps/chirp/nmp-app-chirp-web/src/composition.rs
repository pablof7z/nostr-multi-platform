//! Feed composition root for the Chirp web client.
//!
//! `setup_chirp_web_feeds` is the single entry point. It:
//!
//! 1. Extracts the kernel's [`ActiveAccountSlot`] from the reducer.
//! 2. Constructs an [`ActiveFollowSet`] seeded from the active account.
//! 3. Builds an `EventLookup` closure over the kernel's `EventStore` handle.
//! 4. Creates a [`PendingClaimQueue`] and a queuing [`ClaimSink`] that parks
//!    `ClaimRequest`s while the reducer is borrowed.
//! 5. Constructs the [`OpFeedEngine`] via `register_op_feed`.
//! 6. Registers the engine as a [`KernelEventObserver`] on the reducer.
//! 7. Registers the follow set as a `KernelEventObserver` for kind:3 ingest.
//! 8. Registers an `on_change` callback that resets the engine ONLY on an
//!    actual account switch (not on a kind:3 follow-set update). Uses the same
//!    self-detecting `last_seen` pattern as the native `register_op_feed_defaults`.
//! 9. Registers the typed `nmp.feed.home` snapshot projection.
//! 10. Installs the post-tick drain hook into the runtime.
//!
//! The returned [`ChirpWebFeedSetup`] gives the caller handles to:
//!
//! * Notify the follow set on account change
//!   (`ChirpWebFeedSetup::notify_account_changed`).
//! * Query the feed engine directly for UI-driven snapshot pulls.
//!
//! # Engine reset on identity change
//!
//! The engine holds roots and attributions built from the *prior* account's
//! events. After an account switch or logout the prior identity's roots MUST
//! be cleared via [`OpFeedEngine::reset_for_identity_change`]. The `on_change`
//! callback self-detects: it remembers the last-seen active pubkey (seeded at
//! registration time) and calls `reset_for_identity_change` only when the
//! pubkey actually changes — NOT on ordinary kind:3 follow-set updates.
//!
//! This mirrors the `last_seen` / `engine_for_cb.reset_for_identity_change()`
//! pattern in `nmp-defaults/src/op_feed_defaults.rs` §6.
//!
//! # Doctrine
//!
//! * **D0** — no protocol nouns leak into this crate's public API; the surface
//!   is [`WasmRuntime`] in, [`ChirpWebFeedSetup`] out.
//! * **D7** — composition is wired by closures; the engine asks, the drain
//!   decides.
//! * **D8** — no I/O or blocking in any registered closure.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use nmp_core::KernelEventObserver;
use nmp_core::slots::ActiveAccountSlot;
use nmp_nip01::op_feed::{
    encode_op_feed_snapshot, register_op_feed, OpFeedEngine, OP_FEED_FILE_IDENTIFIER,
    OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION, OP_FEED_SNAPSHOT_KEY,
};
use nmp_nip02::ActiveFollowSet;
use nmp_feed::FeedRequest;
use nmp_core::TypedProjectionData;
use nmp_wasm::WasmRuntime;

use crate::claim_queue::{
    build_queuing_claim_sink, drain_pending_claims, new_pending_claim_queue,
};

/// All handles the composition root hands back to the caller after wiring.
pub struct ChirpWebFeedSetup {
    /// The OP-feed engine. Use `.snapshot(…)` for direct UI reads.
    pub engine: Arc<OpFeedEngine>,
    /// The active-follow-set producer. Call `notify_account_changed()` when
    /// the active account switches or the user logs out.
    pub follow_set: Arc<ActiveFollowSet>,
    /// Last-seen active pubkey, used by `notify_account_changed` to detect
    /// whether the account actually changed (vs. a follow-set kind:3 update).
    last_seen: Arc<Mutex<Option<String>>>,
    /// Active-account slot, read by `notify_account_changed` to obtain the
    /// current pubkey after the host has written it.
    active_account_slot: ActiveAccountSlot,
}

impl ChirpWebFeedSetup {
    /// Notify the follow set that the active account changed (switch or
    /// logout). Rebuilds the set from the kernel slot, fires any registered
    /// `on_change` callbacks, and — **if the account pubkey actually changed**
    /// — resets the engine to discard the prior identity's roots and pending
    /// claims.
    ///
    /// Mirrors the `engine_for_cb.reset_for_identity_change()` path in
    /// `nmp-defaults/src/op_feed_defaults.rs` §6. The `last_seen` guard
    /// prevents a spurious reset on a kind:3 follow-set update (the `on_change`
    /// callback is fired on BOTH a kind:3 update and an account switch;
    /// self-detection distinguishes the two).
    pub fn notify_account_changed(&self) {
        // Notify the follow set first so its internal state is updated.
        self.follow_set.notify_account_changed();

        // Then check whether the active pubkey actually changed.
        let current = read_active(&self.active_account_slot);
        let Ok(mut last) = self.last_seen.lock() else {
            return;
        };
        if *last != current {
            *last = current;
            self.engine.reset_for_identity_change();
        }
    }
}

/// Read the active account's hex pubkey from the slot, or `None` when no
/// account is signed in or the lock is poisoned (D6).
fn read_active(slot: &ActiveAccountSlot) -> Option<String> {
    match slot.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    }
}

/// Wire the OP-centric home feed into `runtime`.
///
/// Returns a [`ChirpWebFeedSetup`] the caller retains for account-change
/// notifications and direct snapshot queries.
///
/// Call this once after `WasmRuntime::new()`, before `Start`. The engine
/// will observe kernel events as soon as relays are connected.
#[must_use]
pub fn setup_chirp_web_feeds(runtime: &WasmRuntime) -> ChirpWebFeedSetup {
    let reducer = runtime.reducer_handle();

    // 1. Active-account slot (for follow-set seeding + account switch).
    let active_account_slot = reducer.borrow().active_account_handle();

    // 2. Follow set — seeded immediately from the slot.
    let follow_set = ActiveFollowSet::new(Arc::clone(&active_account_slot));

    // 3. Event lookup — captures Arc<dyn EventStore>, not the Rc<RefCell>.
    //    Uses `event_by_id_from_arc` to share the hex-decode body with
    //    the native seam (ADR §4-B, no duplication).
    let event_store = reducer.borrow().event_store_handle();
    let event_lookup: nmp_feed::EventLookup = Arc::new(move |id: &String| {
        nmp_core::slots::event_by_id_from_arc(&event_store, id.as_str())
    });

    // 4. Queuing claim sink — parks ClaimRequests while reducer is borrowed.
    let queue = new_pending_claim_queue();
    let claim_sink = build_queuing_claim_sink(Arc::clone(&queue));

    // 5. Build the OP-feed engine.
    let viewer = reducer
        .borrow()
        .active_account_pubkey()
        .unwrap_or_default();
    let engine = register_op_feed(
        viewer,
        follow_set.predicate(),
        event_lookup,
        claim_sink,
    );

    // 6. Register the engine as an event observer (kind:0/1/6 ingest).
    reducer
        .borrow()
        .register_event_observer(Arc::clone(&engine) as Arc<dyn KernelEventObserver>);

    // 7. Register the follow set as an event observer (kind:3 ingest).
    reducer
        .borrow()
        .register_event_observer(Arc::clone(&follow_set) as Arc<dyn KernelEventObserver>);

    // 8. Wire the identity-change engine reset.
    //
    // `on_change` fires on BOTH a kind:3 update and an account switch.
    // The predicate is live (it holds a clone of the follow-set's internal
    // `Arc<RwLock<…>>`), so a kind:3 update needs no engine action; only an
    // account switch requires a reset. The callback self-detects against the
    // slot, seeded with the registration-time active pubkey so the first
    // kind:3 fire is not a false positive. Mirrors the identical logic in
    // `nmp-defaults/src/op_feed_defaults.rs` §6.
    let last_seen = Arc::new(Mutex::new(read_active(&active_account_slot)));
    let engine_for_cb = Arc::clone(&engine);
    let slot_for_cb = Arc::clone(&active_account_slot);
    let last_for_cb = Arc::clone(&last_seen);
    follow_set.on_change(Box::new(move || {
        let current = read_active(&slot_for_cb);
        let Ok(mut last) = last_for_cb.lock() else {
            return;
        };
        if *last != current {
            *last = current;
            engine_for_cb.reset_for_identity_change();
        }
    }));

    // 9. Register the typed snapshot projection under "nmp.feed.home".
    let engine_for_projection = Arc::clone(&engine);
    reducer.borrow().register_typed_snapshot_projection(
        OP_FEED_SNAPSHOT_KEY,
        move || {
            let snapshot = engine_for_projection.snapshot(&FeedRequest::default());
            Some(TypedProjectionData {
                key: OP_FEED_SNAPSHOT_KEY.to_string(),
                schema_id: OP_FEED_SCHEMA_ID.to_string(),
                schema_version: OP_FEED_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(OP_FEED_FILE_IDENTIFIER).into_owned(),
                payload: encode_op_feed_snapshot(&snapshot),
                ..Default::default()
            })
        },
    );

    // 10. Install post-tick drain hook.
    let queue_for_drain = Arc::clone(&queue);
    let reducer_for_drain = Rc::clone(&reducer);
    runtime.install_post_tick_drain(Rc::new(move || {
        drain_pending_claims(&queue_for_drain, &reducer_for_drain);
    }));

    ChirpWebFeedSetup {
        engine,
        follow_set,
        last_seen,
        active_account_slot,
    }
}
