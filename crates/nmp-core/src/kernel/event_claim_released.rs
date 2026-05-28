//! V-59 rung 1 (#4) — `event_claim_released` ring projection + observer signal.
//!
//! When an event claim resolves to EOSE-without-match (the relay set returned
//! no event for the claimed `primary_id`), the kernel:
//!
//! 1. clears the claim's `event_claims` + `event_claim_requested` state so a
//!    later re-claim can re-fetch (the claim is no longer "in flight"), and
//! 2. pushes the `primary_id` into the bounded `event_claim_released` ring and
//!    notifies any registered [`EventClaimReleasedObserver`].
//!
//! The ring is a read projection later rungs consume: the OP-centric feed
//! engine registers an observer to learn "this claimed thread root could not
//! be hydrated" and drop the corresponding pending attribution rather than
//! parking it forever.
//!
//! ## Observer shape
//!
//! In-process Rust-only for now — there is no FFI consumer in this PR. The
//! registration mirrors the *spirit* of `actor/commands/raw_event_observer.rs`
//! (register / notify), but deliberately omits the C-ABI fan-out channel,
//! background drain thread, and kind filter: a no-consumer signal does not
//! warrant that machinery (Article VII — no future-proofing). When an FFI
//! consumer materializes, add the C-ABI channel following the
//! `raw_event_observer.rs` precedent.
//!
//! Doctrine:
//! - **D0** — substrate-generic. `event_claim_released` carries raw event-id /
//!   coordinate strings; no NIP / protocol noun in the API.
//! - **D6** — observer callbacks are best-effort; a panicking observer is
//!   isolated with `catch_unwind` so it cannot unwind the actor thread.
//! - **D8** — `notify` is O(observers); the no-observer fast path is a single
//!   `is_empty` check.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use super::Kernel;

/// In-process observer notified when a claimed `primary_id` is released
/// because its claim resolved to EOSE-without-match. Implementations must be
/// cheap and must not block — the callback fires on the actor thread between
/// relay frames.
pub trait EventClaimReleasedObserver: Send + Sync {
    /// Called once per `primary_id` pushed into the `event_claim_released`
    /// ring. `primary_id` is the raw hex event id (nevent/note URIs) or the
    /// `kind:pubkey:d_tag` coordinate string (naddr URIs).
    fn on_event_claim_released(&self, primary_id: &str);
}

impl Kernel {
    /// Register an in-process observer for `event_claim_released` ring pushes.
    /// Idempotent at the slot level only in the sense that the same `Arc`
    /// added twice fires twice — callers own dedup if they need it (mirrors
    /// the additive `register_*_observer` contract elsewhere).
    pub fn register_event_claim_released_observer(
        &mut self,
        observer: Arc<dyn EventClaimReleasedObserver>,
    ) {
        self.event_claim_released_observers.push(observer);
    }

    /// Public read projection: the released-claim ids in arrival order
    /// (oldest first). Returns an owned snapshot of raw id / coordinate
    /// strings — display composition is a higher-layer concern.
    #[must_use]
    pub fn event_claim_released(&self) -> Vec<String> {
        self.event_claim_released.iter().cloned().collect()
    }

    /// Push `primary_id` into the released-claim ring and fan it out to every
    /// registered observer. Best-effort per observer (D6: a panicking
    /// observer is isolated and does not stop its siblings nor unwind the
    /// actor thread).
    pub(in crate::kernel) fn record_event_claim_released(&mut self, primary_id: &str) {
        self.event_claim_released.push(primary_id.to_string());
        if self.event_claim_released_observers.is_empty() {
            return;
        }
        // Snapshot the observer list so a callback that re-registers cannot
        // invalidate the iteration.
        let observers: Vec<Arc<dyn EventClaimReleasedObserver>> =
            self.event_claim_released_observers.clone();
        for observer in &observers {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                observer.on_event_claim_released(primary_id);
            }));
        }
    }
}
