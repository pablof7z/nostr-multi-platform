//! Post-restart live-receive recovery for Marmot group feeds.
//!
//! On every launch the in-memory `group_relays` cache starts empty and the
//! per-group kind:445 interests are absent — only the gift-wrap inbox interest
//! is re-pushed by `register_with_keys`. Groups joined in a prior session would
//! therefore never receive live kind:445 traffic until a create/join op ran
//! again this session. [`MarmotProjection::resubscribe_all_groups`] closes that
//! gap by replaying the persisted group set through the existing relay-cache
//! choke-point at registration time.

use mdk_core::prelude::group_types::GroupState;

use super::state::{hex_encode, InnerHandle, MarmotProjection};

impl MarmotProjection {
    /// Re-push kind:445 group-message interests for every group that MDK has
    /// persisted in the SQLite store.
    ///
    /// Called from `register_with_keys` right after the gift-wrap inbox push so
    /// that groups joined in a prior session receive live kind:445 traffic
    /// immediately after restart (the live-leg analogue of the store-leg
    /// cache-serve gap fixed in #1237).
    ///
    /// ## Design notes
    ///
    /// * Uses the EXISTING `cache_group_relays` choke-point, which both seeds
    ///   the in-memory `group_relays` HashMap and calls `subscribe_group_messages`
    ///   — no duplicate interest-push logic.
    /// * Interest ids are deterministic (`group_message_interest_id`); the kernel
    ///   de-dupes, so calling this after an in-session `create_group` /
    ///   `accept_welcome` that already pushed the interest is idempotent.
    /// * Empty relay set → skipped (matches the existing empty-guard in
    ///   `cache_group_relays`).
    /// * D8 compliant: one-shot, non-blocking. No timers, no polling.
    /// * D6: poisoned mutex / storage error → silent no-op (already degraded).
    pub fn resubscribe_all_groups(&self) {
        let Ok(mut guard) = self.inner.lock() else {
            return; // D6 — poisoned mutex silently no-ops.
        };
        let mut h = InnerHandle {
            inner: &mut guard,
            port: None,
        };

        // Enumerate all groups MDK has persisted in the SQLite store.
        let Ok(groups) = h.service().get_groups() else {
            return; // D6 — storage error silently no-ops.
        };

        for group in groups {
            // Security / correctness: only re-subscribe groups that are still
            // Active.  Inactive groups (left or removed) must not receive live
            // kind:445 traffic — resuming subscriptions for them would leak
            // metadata to relays and re-open delivery channels that the user
            // explicitly closed.
            if group.state != GroupState::Active {
                continue;
            }

            let group_id = &group.mls_group_id;

            // Read the persisted relay URLs for this group from MDK.
            let Ok(relays) = h.service().group_relays(group_id) else {
                continue; // per-group storage error: skip, not abort.
            };

            // Empty relay set → skip; matches the empty-guard in cache_group_relays.
            if relays.is_empty() {
                continue;
            }

            // Route through the existing choke-point: seeds the in-memory
            // group_relays cache AND pushes subscribe_group_messages interests.
            let gid_hex = hex_encode(group_id.as_slice());
            h.cache_group_relays(gid_hex, relays);
        }
    }
}
