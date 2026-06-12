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
//! 7. Registers the engine as a `KernelEventObserver` for the follow-set too.
//! 8. Registers the typed `nmp.feed.home` snapshot projection.
//! 9. Installs the post-tick drain hook into the runtime.
//!
//! The returned [`ChirpWebFeedSetup`] gives the caller handles to:
//!
//! * Notify the follow set on account change
//!   (`ChirpWebFeedSetup::notify_account_changed`).
//! * Query the feed engine directly for UI-driven snapshot pulls.
//!
//! # Doctrine
//!
//! * **D0** — no protocol nouns leak into this crate's public API; the surface
//!   is [`WasmRuntime`] in, [`ChirpWebFeedSetup`] out.
//! * **D7** — composition is wired by closures; the engine asks, the drain
//!   decides.
//! * **D8** — no I/O or blocking in any registered closure.

use std::rc::Rc;
use std::sync::Arc;

use nmp_core::KernelEventObserver;
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
}

impl ChirpWebFeedSetup {
    /// Convenience wrapper: notify the follow set that the active account
    /// changed (switch or logout). Rebuilds the set from the kernel slot and
    /// fires any registered `on_change` callbacks.
    pub fn notify_account_changed(&self) {
        self.follow_set.notify_account_changed();
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
    let follow_set = ActiveFollowSet::new(active_account_slot);

    // 3. Event lookup — captures Arc<dyn EventStore>, not the Rc<RefCell>.
    let event_store = reducer.borrow().event_store_handle();
    let event_lookup: nmp_feed::EventLookup = Arc::new(move |id: &String| {
        // hex_to_id_bytes + store.get_by_id — same body as event_by_id() but
        // using the pre-captured store Arc so the closure is Send+Sync.
        let id: &str = id.as_str();
        let key = {
            if id.len() != 64 {
                return None;
            }
            let mut out = [0u8; 32];
            for (i, chunk) in id.as_bytes().chunks(2).enumerate() {
                let hi = (chunk[0] as char).to_digit(16)? as u8;
                let lo = (chunk[1] as char).to_digit(16)? as u8;
                out[i] = (hi << 4) | lo;
            }
            out
        };
        let stored = event_store.get_by_id(&key).ok()??;
        let raw = &stored.raw;
        Some(nmp_core::substrate::KernelEvent {
            id: raw.id.clone(),
            author: raw.pubkey.clone(),
            kind: raw.kind,
            created_at: raw.created_at,
            tags: raw.tags.clone(),
            content: raw.content.clone(),
        })
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

    // 8. Register the typed snapshot projection under "nmp.feed.home".
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
            })
        },
    );

    // 9. Install post-tick drain hook.
    let queue_for_drain = Arc::clone(&queue);
    let reducer_for_drain = Rc::clone(&reducer);
    runtime.install_post_tick_drain(Rc::new(move || {
        drain_pending_claims(&queue_for_drain, &reducer_for_drain);
    }));

    ChirpWebFeedSetup { engine, follow_set }
}
