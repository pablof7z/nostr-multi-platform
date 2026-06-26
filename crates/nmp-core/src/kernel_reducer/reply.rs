//! Reply write-path surface for [`super::KernelReducer`].
//!
//! Split from `kernel_reducer.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). These two methods form the PR-5 reply write path:
//! `publish_signed_event` feeds a caller-signed event through the publish
//! engine and fans out the resulting outbound frames; `build_reply_tags`
//! resolves NIP-10 marked-form reply tags from the kernel store before the
//! async sign boundary so no `RefCell` borrow lives across an await point.

use crate::relay::OutboundMessage;
use nmp_signer_iface::SignedEvent;

impl super::KernelReducer {
    /// V-01 Stage 3c — public publish-from-signed-event surface for non-actor
    /// consumers (today: the wasm32 `WasmRuntime` write path after the
    /// `Nip07Signer::sign()` Promise resolves; tomorrow: any in-process Rust
    /// caller that signs out-of-band and wants to feed the result through the
    /// kernel's publish engine).
    ///
    /// Internally delegates to `Kernel::publish_signed_with_correlation` —
    /// byte-for-byte the same entrypoint `actor::commands::publish::publish_unsigned_event`
    /// reaches after `sign_active_nonblocking` resolves on the dispatched
    /// path. The returned `Vec<OutboundMessage>` is the engine's per-(outbox-
    /// relay) `EVENT` frame set, already AUTH-pause-partitioned through
    /// `partition_auth_paused` for symmetry with the `handle_relay_*` surface
    /// above.
    ///
    /// `p_tags` mirrors the legacy parameter on `Kernel::publish_signed` —
    /// callers that have no extra `#p` tags pass an empty slice. The engine
    /// recomputes `#p` tags from `signed.unsigned.tags` itself, so this slice
    /// is informational only (kept on the surface so a future caller that
    /// needs additional outbox routing tags has a place to inject them).
    ///
    /// `correlation_id` is the host-visible action id the publish should
    /// report in the `action_results` projection on terminal verdicts (per-
    /// relay OK / failed). Pass `Some(id)` when the publish is a dispatched
    /// action whose host caller is awaiting a terminal under `id` (the wasm
    /// runtime's `dispatch_app_action_async` Promise path); pass `None` for
    /// non-dispatch callers (the engine then reports the event id as the
    /// terminal key, matching every existing non-dispatched native publish).
    ///
    /// Without correlation threading the wasm host receives a publish-engine
    /// terminal keyed on an event id it never saw — defeating partial-success
    /// UX (e.g. "2/3 relays accepted"). Pinning the contract here keeps the
    /// wasm path byte-for-byte aligned with the native generic publish dispatch.
    ///
    /// Doctrine (D0/D6): the surface is substrate-typed (`SignedEvent`,
    /// `OutboundMessage`); failure is encoded as an empty outbound vec plus a
    /// kernel-side toast / `RecentFailure` row (no `Result` across this
    /// boundary, matching every other `KernelReducer` method).
    pub fn publish_signed_event(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
        correlation_id: Option<String>,
    ) -> Vec<OutboundMessage> {
        // Route through the shared Kernel::publish_externally_signed helper
        // (#2045 PR-A): target-validate → verify-sig → D10 routing gate →
        // publish. Callers always pass `p_tags = &[]`; the engine re-extracts
        // `#p` tags from `signed.unsigned.tags` internally.
        //
        // Uses PublishTarget::Auto because this path (the post-sign wasm reply
        // surface) always resolves via the NIP-65 outbox; callers that need an
        // explicit pin use `publish_pre_signed` with `PublishTarget::Explicit`.
        let raw = crate::store::RawEvent {
            id: signed.id.clone(),
            sig: signed.sig.clone(),
            pubkey: signed.unsigned.pubkey.clone(),
            kind: signed.unsigned.kind,
            tags: signed.unsigned.tags.clone(),
            content: signed.unsigned.content.clone(),
            created_at: signed.unsigned.created_at,
        };
        let _ = p_tags; // callers always pass &[]; p-routing comes from event tags
        let outbound =
            self.kernel
                .publish_externally_signed(raw, crate::publish::PublishTarget::Auto, correlation_id);
        self.kernel.partition_auth_paused(outbound)
    }

    /// Build NIP-10 marked-form reply tags for a reply to `reply_to_id` (hex).
    ///
    /// Delegates to [`crate::tags::reply_tags`] after looking up the event
    /// from the kernel store and parsing its NIP-10 refs. Returns `None` on
    /// invalid hex, missing event, or non-kind-1 parent (fail-closed; matches
    /// native `NoteRecord`-only domain). Callers: use `reply_target_unknown:`
    /// reason. Takes `&self` — the borrow drops before any async boundary
    /// (wasm `RefCell` borrow discipline).
    #[must_use]
    pub fn build_reply_tags(&self, reply_to_id: &str) -> Option<Vec<Vec<String>>> {
        use crate::kernel::hex_to_pubkey_bytes;
        use crate::tags::{parse_nip10, reply_tags};

        let id_bytes = hex_to_pubkey_bytes(reply_to_id)?;
        let store = self.kernel.event_store_handle();
        let stored = store.get_by_id(&id_bytes).ok()??;
        if stored.raw.kind != 1 {
            return None;
        }
        let refs = parse_nip10(&stored.raw.tags);
        Some(reply_tags(reply_to_id, &stored.raw.pubkey, &refs, None))
    }
}
