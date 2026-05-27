//! NIP-42 AUTH gate: predicates over the per-relay driver state plus the
//! `partition_auth_paused` outbound-batch splitter that withholds REQs from
//! AUTH-paused relays (deferring them into `deferred_outbound`) and drops
//! REQs targeting `Failed` relays (fail-closed per ADR-0019). Pairs with
//! `relay_lifecycle` (which resets drivers on reconnect) and the deferred-
//! queue drain in `pending_view_requests`.

use super::super::{Kernel, OutboundMessage, RelayRole};

impl Kernel {
    /// True when REQs to `role` must be withheld from the wire:
    /// `ChallengeReceived` / `Authenticating` (transient, deferred) or
    /// `Failed` (fail-closed, dropped — see [`Self::relay_auth_failed`]).
    /// `NotRequired` / `Authenticated` are pass-through. Per ADR-0019 a
    /// `Failed` relay never silently downgrades to unauthenticated reads.
    pub(crate) fn relay_auth_paused(&self, role: RelayRole) -> bool {
        let state = self
            .auth_drivers
            .get(&role)
            .map_or(crate::subs::RelayAuthState::NotRequired, |d| {
                d.state.clone()
            });
        matches!(
            state,
            crate::subs::RelayAuthState::ChallengeReceived
                | crate::subs::RelayAuthState::Authenticating
                | crate::subs::RelayAuthState::Failed
        )
    }

    /// True when `role`'s NIP-42 handshake is `Failed`. REQs to a failed
    /// relay are dropped, not deferred (the shared ring has no per-relay
    /// segregation). Recovery is reconnect-only. Rationale: ADR-0019.
    pub(crate) fn relay_auth_failed(&self, role: RelayRole) -> bool {
        matches!(
            self.auth_drivers.get(&role).map(|d| d.state.clone()),
            Some(crate::subs::RelayAuthState::Failed)
        )
    }

    /// Purge deferred REQ messages targeting `role` — called on the
    /// transition into `Failed` so withheld REQs cannot leak when the ring
    /// next drains. CLOSEs and other relays' messages are retained (ADR-0019).
    pub(crate) fn purge_deferred_reqs_for(&mut self, role: RelayRole) {
        self.deferred_outbound
            .retain(|msg| !(msg.role == role && msg.text.starts_with("[\"REQ\"")));
    }

    /// Partition an outbound batch: REQ frames targeting an AUTH-paused relay
    /// are removed from the batch and parked in the deferred queue (drained
    /// on `Authenticated` via `pending_view_requests`). Non-REQ frames and
    /// REQs to live relays pass through unchanged. This is the M5+M2+M8
    /// wiring seam replacing the hand-rolled "send + cross-fingers" path for
    /// AUTH-required relays — `AuthGate` semantics modelled inline so the
    /// kernel doesn't need to hold a separate per-relay buffer.
    ///
    /// **D8 invariant:** unlike the generic `defer_outbound` path (which
    /// bumps `changed_since_emit` because connection-drop replay is itself
    /// a diagnostic event worth surfacing), AUTH-pause re-defers do NOT
    /// bump the emit flag. AUTH-state is already pure-diagnostic per the
    /// `update_relay_auth_status` contract; re-defer on every tick (the
    /// `pending_view_requests` drain → still-paused re-defer loop) would
    /// otherwise wake the actor every tick.
    pub(crate) fn partition_auth_paused(
        &mut self,
        outbound: Vec<OutboundMessage>,
    ) -> Vec<OutboundMessage> {
        let mut passthrough = Vec::with_capacity(outbound.len());
        for msg in outbound {
            let is_req = msg.text.starts_with("[\"REQ\"");
            if is_req && self.relay_auth_failed(msg.role) {
                // Fail-closed: an AUTH-required relay that rejected/refused
                // AUTH gets its gated REQs DROPPED, never deferred — no
                // silent unauthenticated downgrade (T76 / ADR-0019).
                self.log(format!(
                    "REQ@{} dropped — relay AUTH failed (fail-closed)",
                    msg.role.key()
                ));
            } else if is_req && self.relay_auth_paused(msg.role) {
                self.log(format!("REQ@{} held — relay AUTH-paused", msg.role.key()));
                self.defer_outbound_silent(msg);
            } else {
                passthrough.push(msg);
            }
        }
        passthrough
    }

    /// Diagnostic-quiet variant of `defer_outbound` — same bounded-queue
    /// discipline (64 slots) but does NOT set `changed_since_emit`. Used by
    /// `partition_auth_paused` so the actor doesn't false-wakeup-emit on
    /// every tick that re-defers an AUTH-paused REQ.
    fn defer_outbound_silent(&mut self, message: OutboundMessage) {
        self.deferred_outbound.push_back(message);
        while self.deferred_outbound.len() > 64 {
            self.deferred_outbound.pop_front();
        }
    }
}
