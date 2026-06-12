//! PR-3 feed-verb surface for [`super::KernelReducer`].
//!
//! Split from `kernel_reducer.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). The four methods here expose the M2 interest-registry
//! and identity surface to the wasm runtime so the browser host can open/close
//! generic feed subscriptions and hand off the viewer pubkey after NIP-07
//! sign-in.
//!
//! All four follow the same post-processing pattern as the relay-lifecycle
//! methods in `kernel_reducer.rs`: `drain_lifecycle_outbound` is called inline
//! (the wasm path has no idle actor loop — this is intentional) and
//! `partition_auth_paused` gates the result (AUTH-pause invariant).

use crate::relay::OutboundMessage;

impl super::KernelReducer {
    /// Attach a generic feed interest identified by `(filter_json, consumer_id,
    /// scope)`. On cold open (first owner) emits the batched-REQ frames
    /// immediately by draining the lifecycle outbound inline.
    ///
    /// `scope == 0` → `InterestScope::ActiveAccount` (re-routes on account
    /// switch). Any other value → `InterestScope::Global`.
    ///
    /// Malformed `filter_json` is silently dropped (D6: no panic, no
    /// `Result`). Duplicate opens (same owner already attached) are no-ops.
    pub fn open_interest(
        &mut self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
    ) -> Vec<OutboundMessage> {
        if let Some((identity, interest)) =
            crate::subs::interest_builder::build_interest_pair(filter_json, consumer_id, scope)
        {
            let _ = self.kernel.open_interest_sub(identity, interest);
        }
        let outbound = self.kernel.drain_lifecycle_outbound();
        self.kernel.partition_auth_paused(outbound)
    }

    /// Detach one owner from a generic feed interest. When the last owner
    /// leaves, enqueues a CLOSE diff and emits it inline.
    ///
    /// Malformed `filter_json` is silently dropped (D6). Closing an interest
    /// that is not open is a no-op.
    pub fn close_interest(
        &mut self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
    ) -> Vec<OutboundMessage> {
        if let Some((identity, _interest)) =
            crate::subs::interest_builder::build_interest_pair(filter_json, consumer_id, scope)
        {
            let _ = self.kernel.close_interest_sub(&identity);
        }
        let outbound = self.kernel.drain_lifecycle_outbound();
        self.kernel.partition_auth_paused(outbound)
    }

    /// Set the event kinds the contact-feed subscription should carry and
    /// re-register the active account's follow-feed interests under the new
    /// kind set. An empty `kinds` set deactivates the subscription (withdraws
    /// every follow-feed interest).
    ///
    /// Called by the host once at startup (before or after `set_active_account`)
    /// and again whenever the user changes their kind preferences.
    pub fn set_follow_feed_kinds(
        &mut self,
        kinds: std::collections::BTreeSet<u32>,
    ) -> Vec<OutboundMessage> {
        self.kernel.set_follow_feed_kinds(kinds);
        let outbound = self.kernel.drain_lifecycle_outbound();
        self.kernel.partition_auth_paused(outbound)
    }

    /// Install `pubkey_hex` as the active viewer account and fan out the
    /// resulting bootstrap interests and follow-feed reconciliation.
    ///
    /// This is the wasm analogue of `actor::commands::identity::switch_active`:
    /// it sets `active_account` (kernel projection + handle mutex), reconciles
    /// the M2 follow-feed (withdraw prior account's interests / install new
    /// account's follows), registers bootstrap interests for the new account
    /// (self-profile, NIP-65, kind:10050, contacts), and drains lifecycle
    /// outbound inline so any REQs that can be emitted against already-connected
    /// relays go out immediately.
    ///
    /// Idempotence gate: a redundant call with the same pubkey is a no-op
    /// and returns `Vec::new()`. This mirrors native `switch_active`'s
    /// early-return (`identity.active.as_deref() == Some(identity_id)`)
    /// and prevents a duplicate `SetSigner` from clearing the
    /// `pre_kind3_buffer` (V-59 rung 1 invariant).
    ///
    /// D6 — total: an empty or malformed pubkey is stored as-is (the kernel
    /// makes no validity assertion on `active_account`). No panic.
    pub fn set_active_account(&mut self, pubkey_hex: String) -> Vec<OutboundMessage> {
        // Same-account guard — must not clear pre_kind3_buffer on redundant call.
        if self.kernel.active_account_pubkey() == Some(pubkey_hex.as_str()) {
            return Vec::new();
        }
        self.kernel.set_active_account(pubkey_hex);
        self.kernel.reconcile_follow_feed_after_identity_change();
        let mut outbound = self.kernel.active_account_bootstrap_requests();
        // drain_lifecycle_outbound is called inline here (not in tick()) because
        // the wasm path has no idle actor loop — this is intentional.
        outbound.extend(self.kernel.drain_lifecycle_outbound());
        self.kernel.partition_auth_paused(outbound)
    }
}
