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
//! 8. Registers an `on_change` callback that resets the engine on every
//!    follow-set perspective change.
//! 9. Registers the typed `nmp.feed.home` snapshot projection.
//! 10. Installs the post-tick drain hook into the runtime.
//!
//! The returned [`ChirpWebFeedSetup`] gives the caller handles to:
//!
//! * Notify the follow set on account change
//!   (`ChirpWebFeedSetup::notify_account_changed`).
//! * Query the feed engine directly for UI-driven snapshot pulls.
//!
//! # Engine reset on perspective change
//!
//! The engine holds roots and attributions admitted under the current
//! perspective: active account, follow set, and future mute/block policy. When
//! that perspective changes, stale rows must disappear immediately. The
//! `ActiveFollowSet::on_change` callback therefore resets the engine on
//! account switch, logout, and kind:3 replacement.
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

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::KernelEventObserver;
use nmp_core::TypedProjectionData;
use nmp_feed::FeedRequest;
use nmp_nip01::op_feed::{
    encode_op_feed_snapshot, register_op_feed, OpFeedEngine, OP_FEED_FILE_IDENTIFIER,
    OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION, OP_FEED_SNAPSHOT_KEY,
};
use nmp_nip02::ActiveFollowSet;
use nmp_wasm::WasmRuntime;

use crate::claim_queue::{build_queuing_claim_sink, drain_pending_claims, new_pending_claim_queue};

/// Content-parser seam implementation backed by nmp-content.
///
/// nmp-core can't depend on nmp-content (it would create a dependency cycle —
/// nmp-content depends on nmp-core), so the kernel holds a no-op
/// [`ContentParser`](nmp_core::substrate::ContentParser) by default. This crate
/// CAN depend on nmp-content, so it installs the real NIP-10/markdown tokenizer
/// into the kernel via `set_content_parser`. The kernel then carries a parsed
/// NFCT content tree in the `claimed_events` typed projection, which the web
/// content components decode and render — the `claim_event` twin of the native
/// content path.
struct NmpContentParser;

impl nmp_core::substrate::ContentParser for NmpContentParser {
    fn parse_to_nfct_bytes(&self, content: &str, tags: &[Vec<String>], kind: u32) -> Vec<u8> {
        // `RenderMode::Auto` sniffs the render mode from the event kind
        // (markdown for long-form 30023, inline for kind:1, …) — matching the
        // native tokenizer entry point.
        let tree =
            nmp_content::tokenize_with_kind(content, tags, nmp_content::RenderMode::Auto, kind);
        nmp_content::wire::typed_fb::encode_content_tree(&tree.to_wire())
    }
}

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
    /// Notify the follow set that the active account changed.
    ///
    /// A guard prevents failed/no-op signer installs from resetting the feed
    /// when the active pubkey did not actually change. Real account changes
    /// call `ActiveFollowSet::notify_account_changed`, whose `on_change`
    /// callback resets the current perspective window.
    pub fn notify_account_changed(&self) {
        let current = read_active(&self.active_account_slot);
        let Ok(mut last) = self.last_seen.lock() else {
            self.follow_set.notify_account_changed();
            return;
        };
        if *last == current {
            return;
        }
        *last = current;
        drop(last);

        self.follow_set.notify_account_changed();
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

    // 0. Install the production substrate cache/parser pairs. The wasm32 path
    //    has no native AppHost, so it calls the same wasm-safe construction
    //    helper used by `nmp_defaults::register_substrate`: one mailbox cache
    //    shared by the router and kind:10002 parser, one profile cache shared
    //    by the profile lookup and kind:0 parser, and one contacts cache shared
    //    by the contacts lookup and kind:3 parser.
    nmp_substrate_defaults::install_on_reducer(&mut reducer.borrow_mut());

    // 0b. Install the content-parser seam so the `claimed_events` projection
    //     carries a parsed NFCT content tree. nmp-core can't depend on
    //     nmp-content (layering), so the kernel holds a no-op parser by default;
    //     here (a layer that CAN depend on nmp-content) we install the real
    //     tokenizer. This lets a web host render the kernel-parsed content tree
    //     from a `claim_event` — the content components consume these bytes.
    reducer
        .borrow_mut()
        .set_content_parser(Arc::new(NmpContentParser));

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
    let viewer = reducer.borrow().active_account_pubkey().unwrap_or_default();
    let engine = register_op_feed(viewer, follow_set.predicate(), event_lookup, claim_sink);

    // 6. Register the engine as an event observer (kind:0/1/6 ingest).
    reducer
        .borrow()
        .register_event_observer(Arc::clone(&engine) as Arc<dyn KernelEventObserver>);

    // 7. Register the follow set as an event observer (kind:3 ingest).
    reducer
        .borrow()
        .register_event_observer(Arc::clone(&follow_set) as Arc<dyn KernelEventObserver>);

    // 8. Wire the perspective-change engine reset.
    //
    // `on_change` fires on active-account kind:3 replacement, account switch,
    // and logout. Each invalidates the visible rows admitted under the previous
    // follow-set perspective, so the engine clears immediately and the reactive
    // acquisition/cache path repopulates what still qualifies.
    let last_seen = Arc::new(Mutex::new(read_active(&active_account_slot)));
    let engine_for_cb = Arc::clone(&engine);
    follow_set.on_change(Box::new(move || {
        engine_for_cb.reset_for_perspective_change();
    }));

    // 9. Register the typed snapshot projection under "nmp.feed.home".
    let engine_for_projection = Arc::clone(&engine);
    reducer
        .borrow()
        .register_typed_snapshot_projection(OP_FEED_SNAPSHOT_KEY, move || {
            let snapshot = engine_for_projection.snapshot(&FeedRequest::default());
            Some(TypedProjectionData {
                key: OP_FEED_SNAPSHOT_KEY.to_string(),
                schema_id: OP_FEED_SCHEMA_ID.to_string(),
                schema_version: OP_FEED_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(OP_FEED_FILE_IDENTIFIER).into_owned(),
                payload: encode_op_feed_snapshot(&snapshot),
                ..Default::default()
            })
        });

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
