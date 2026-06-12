//! Queuing claim sink — solves the RefCell re-entrancy hazard.
//!
//! The `RootIndexedFeed` engine fires its `ClaimSink` synchronously during
//! `on_kernel_event`. On the wasm32 path `on_kernel_event` is called from
//! within `KernelReducer::handle_relay_frame`, which holds a `borrow_mut`
//! on the `Rc<RefCell<KernelReducer>>`. If the sink tried to call
//! `reducer.borrow_mut().claim_event(…)` at that point it would panic with
//! "already mutably borrowed".
//!
//! Solution: the sink queues each `ClaimRequest` into a `VecDeque` protected
//! by an `Arc<Mutex>` (so the closure can be `Send + Sync` as `ClaimSink`
//! requires). After `handle_relay_frame` returns — and before the next call
//! borrows the reducer — `drain_pending_claims` processes every queued
//! request. The drain is wired into the runtime via
//! `WasmRuntime::install_post_tick_drain`.
//!
//! # Doctrine
//!
//! * **D6** — poisoned `Mutex` and encode failures are silent no-ops; the
//!   affected claim is dropped (best-effort hydration).
//! * **D8** — sink does O(1) work (lock + push); drain does O(n-pending)
//!   bounded-sized work.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use nmp_core::nip19::{encode_naddr, encode_nevent, NaddrData, NeventData};
use nmp_core::KernelReducer;
use nmp_feed::{ClaimRequest, ClaimSink};
use nmp_threading::pointer::ThreadPointer;

/// Shared queue of pending claim/release requests from the feed engine.
/// `Arc<Mutex<…>>` so the `ClaimSink` closure (which is `Send + Sync`) and
/// the drain closure (which runs on the wasm single-threaded event loop) can
/// share state without `Rc`.
pub type PendingClaimQueue = Arc<Mutex<VecDeque<ClaimRequest>>>;

/// Construct an empty pending-claim queue.
#[must_use]
pub fn new_pending_claim_queue() -> PendingClaimQueue {
    Arc::new(Mutex::new(VecDeque::new()))
}

/// Build a `ClaimSink` that pushes every claim/release request onto `queue`
/// instead of dispatching it immediately.
///
/// The returned sink captures a clone of `queue` (cheap `Arc` clone). It is
/// safe to call from within a `KernelReducer` borrow because it never borrows
/// the reducer itself.
#[must_use]
pub fn build_queuing_claim_sink(queue: PendingClaimQueue) -> ClaimSink {
    Arc::new(move |request: ClaimRequest| {
        if let Ok(mut guard) = queue.lock() {
            guard.push_back(request);
        }
        // Poisoned mutex: D6 silent fail. The claim is dropped; the engine
        // will re-emit it on the next matching event if the root is still
        // needed.
    })
}

/// Drain every pending `ClaimRequest` from `queue` and apply it to `reducer`.
///
/// Each `ClaimRequest::Claim { pointer, consumer_id, … }` is encoded to a
/// `nostr:` URI and forwarded to `KernelReducer::claim_event` with
/// `can_send = false, force = false`.  `can_send = false` means the claim
/// parks in `pending_view_requests`; the next `tick()` call or the next relay
/// connect event will emit the wire REQ.
///
/// Each `ClaimRequest::Release { pointer, consumer_id }` calls
/// `KernelReducer::release_event`.
///
/// # Safety
///
/// This function MUST be called AFTER any `KernelReducer::borrow_mut()` from
/// the current tick is released. The wasm32 tick loop guarantees this: the
/// `tick_once` borrow drops before `fan_out_outbound` runs, and
/// `drain_pending_claims` runs after `fan_out_outbound`.
pub fn drain_pending_claims(queue: &PendingClaimQueue, reducer: &std::rc::Rc<std::cell::RefCell<KernelReducer>>) {
    let requests: Vec<ClaimRequest> = {
        let Ok(mut guard) = queue.lock() else { return };
        guard.drain(..).collect()
    };
    for request in requests {
        match request {
            ClaimRequest::Claim { pointer, hints, consumer_id } => {
                let hint_relays: Vec<String> = hints.into_iter().map(|h| h.url).collect();
                if let Some(uri) = pointer_to_uri(&pointer, &hint_relays) {
                    let _ = reducer.borrow_mut().claim_event(uri, consumer_id, false, false);
                }
            }
            ClaimRequest::Release { pointer, consumer_id } => {
                if let Some(uri) = pointer_to_uri(&pointer, &[]) {
                    let _ = reducer.borrow_mut().release_event(&uri, &consumer_id);
                }
            }
        }
    }
}

/// Encode a [`ThreadPointer`] as a `nostr:`-prefixed NIP-19 URI.
///
/// Returns `None` on any encode failure (malformed coord, non-hex id).
/// Mirrors the private `pointer_to_uri` in `nmp-nip01/src/op_feed/wiring.rs`.
fn pointer_to_uri(pointer: &ThreadPointer, extra_relays: &[String]) -> Option<String> {
    match pointer {
        ThreadPointer::Event { id, relay, kind } => {
            let mut relays: Vec<String> = relay.iter().cloned().collect();
            for r in extra_relays {
                if !relays.contains(r) {
                    relays.push(r.clone());
                }
            }
            let data = NeventData {
                event_id: id.clone(),
                relays,
                author: None,
                kind: *kind,
            };
            encode_nevent(&data).ok().map(|b| format!("nostr:{b}"))
        }
        ThreadPointer::Address { coord, relay, .. } => {
            let mut parts = coord.splitn(3, ':');
            let kind = parts.next()?.parse::<u32>().ok()?;
            let pubkey = parts.next()?.to_string();
            let identifier = parts.next()?.to_string();
            let mut relays: Vec<String> = relay.iter().cloned().collect();
            for r in extra_relays {
                if !relays.contains(r) {
                    relays.push(r.clone());
                }
            }
            let data = NaddrData {
                identifier,
                pubkey,
                kind,
                relays,
            };
            encode_naddr(&data).ok().map(|b| format!("nostr:{b}"))
        }
        ThreadPointer::External { .. } => None,
    }
}
