//! Universal-bug proof for the claim send-gate.
//!
//! # The bug (shared-kernel, NOT Android-specific)
//!
//! The actor computes the per-dispatch `relays_ready` flag at
//! `actor/mod.rs` as `all_relays_connected(&connected_relays)` and feeds it to
//! `kernel.claim_event(uri, consumer, relays_ready)` (`actor/dispatch.rs`).
//! `all_relays_connected` is `true` only when EVERY [`RelayRole`] lane
//! (`Content` AND `Indexer`) has a connected URL. If one bootstrap lane never
//! establishes a socket (on the Android emulator `purplepag.es` / the Indexer
//! lane never opens its WebSocket), `relays_ready` is permanently `false` and
//! every `claim_event` / `claim_profile` / `open_*` parks forever — no REQ is
//! ever emitted, not even to the nevent's own URI relay hint.
//!
//! # Why this is universal — there is NO iOS/TUI bypass
//!
//! iOS and the TUI smoke pass today **only because their environment connects
//! both bootstrap lanes**, so `all_relays_connected` reaches `true`. They run
//! the exact same shared kernel + actor gate. If their Indexer lane were down
//! they would park identically. These tests prove that by composing the **real**
//! actor gate function (`all_relays_connected`) with the **real** kernel
//! `claim_event` — the identical composition every platform executes — and
//! showing that "Content connected, Indexer offline" parks the claim.
//!
//! # RED → GREEN
//!
//! * [`content_up_indexer_down_currently_parks_claim`] — asserts the claim
//!   SENDS (registers a OneshotApi interest) when only the Content lane is
//!   connected. On current master this FAILS because the gate computes `false`
//!   and the claim parks. After Fix A (`relays_ready` uses
//!   `any_relay_connected`) it passes.
//! * [`all_lanes_connected_sends_claim_unchanged`] — the iOS/TUI happy path:
//!   both lanes connected → claim sends. Stays green before and after Fix A,
//!   proving the fix is behavior-preserving when all relays are healthy.

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::actor::relay_mgmt::{all_relays_connected, claim_send_gate};
    use crate::kernel::Kernel;
    use crate::nip19::{encode_nevent, NeventData};
    use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

    /// Build an `nostr:nevent…` URI carrying NIP-19 relay TLVs.
    fn nevent_uri_with_relays(event_id: &str, relays: &[&str]) -> String {
        let bech = encode_nevent(&NeventData {
            event_id: event_id.to_string(),
            relays: relays.iter().map(|r| (*r).to_string()).collect(),
            author: None,
            kind: Some(1),
        })
        .expect("encode_nevent");
        format!("nostr:{bech}")
    }

    fn hex64(prefix: &str) -> String {
        let mut s = prefix.to_string();
        while s.len() < 64 {
            s.push('0');
        }
        s.chars().take(64).collect()
    }

    /// The gate computation itself, isolated. `claim_send_gate` is the single
    /// function the actor calls at `mod.rs` to derive `relays_ready`.
    /// `Content` connected + `Indexer` absent: the historical `all`-lane gate
    /// (`all_relays_connected`) returns `false` (the bug trigger), while the
    /// production gate `claim_send_gate` (Fix A — `any`) returns `true`.
    #[test]
    fn gate_all_vs_any_with_one_lane_offline() {
        let mut connected = HashSet::new();
        connected.insert(RelayRole::Content);
        // Indexer is NOT connected (its bootstrap socket never opened).

        assert!(
            !all_relays_connected(&connected),
            "the historical all-lane gate is false when the Indexer lane is offline \
             — this was exactly the value the actor fed as `relays_ready`, which \
             parked every claim/open dispatch"
        );
        assert!(
            claim_send_gate(&connected),
            "claim_send_gate (Fix A) must be true the moment a single lane is \
             connected — the claim has a reachable socket to leave on"
        );
    }

    /// UNIVERSAL PROOF — RED before Fix A, GREEN after. Drives the identical
    /// composition every platform runs: `relays_ready = claim_send_gate(
    /// &connected_relays)` (the single production gate at `actor/mod.rs`) fed to
    /// `kernel.claim_event(uri, consumer, relays_ready)` (`actor/dispatch.rs`),
    /// with only the Content lane connected.
    ///
    /// Because the test calls the SAME `claim_send_gate` function production
    /// calls, it flips with a one-line body change in that function — not by the
    /// test inlining the new behavior. Pre-Fix-A (`claim_send_gate` body = `.all`)
    /// the gate computes `false`, the claim PARKS, and this fails (RED). Post-Fix-A
    /// (body = `.any`) the gate computes `true`, the claim SENDS, and this passes
    /// (GREEN). iOS/TUI run this exact gate; they pass today ONLY because their
    /// environment connects both bootstrap lanes — there is no platform bypass.
    #[test]
    fn content_up_indexer_down_currently_parks_claim() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

        // Live transport state as the actor would hold it: the Content lane's
        // socket opened, the Indexer lane's never did.
        let mut connected_relays = HashSet::new();
        connected_relays.insert(RelayRole::Content);

        // EXACT actor computation — the single production gate (actor/mod.rs).
        let relays_ready = claim_send_gate(&connected_relays);

        // An nevent that carries a working relay hint — the publisher's own
        // content relay. Even bootstrap-blind, this claim should resolve.
        let id = hex64("a1");
        let uri = nevent_uri_with_relays(&id, &["wss://relay.publisher.example"]);

        // EXACT actor dispatch (mirrors actor/dispatch.rs:509).
        let outbound = kernel.claim_event(uri, "view-universal".to_string(), relays_ready, false);

        assert!(
            outbound.is_empty(),
            "claim_event always returns Vec::new() — wire frames flow through the planner (D4)"
        );

        // THE PROOF: with one lane connected the claim must SEND, i.e. register
        // a OneshotApi interest (observable as `event_claim_requested`).
        assert!(
            kernel.event_claim_is_requested_for_test(&id),
            "UNIVERSAL BUG: with the Content lane connected the claim must register a \
             OneshotApi interest and send a REQ — but the shared actor gate \
             (all_relays_connected) computed relays_ready=false because the Indexer lane \
             is offline, so the claim PARKED with no REQ. iOS/TUI run this identical \
             composition; they pass today ONLY because their environment connects both \
             bootstrap lanes. There is no platform-specific bypass."
        );
        assert_eq!(
            kernel.pending_event_claims_len_for_test(),
            0,
            "the claim must NOT be parked in pending_event_claims when a relay is reachable"
        );
    }

    /// iOS/TUI happy path: both bootstrap lanes connected. The claim sends.
    /// This stays GREEN before and after Fix A — `any` is reached no later than
    /// `all`, so relaxing the gate is behavior-preserving when all relays are
    /// healthy.
    #[test]
    fn all_lanes_connected_sends_claim_unchanged() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

        let mut connected_relays = HashSet::new();
        connected_relays.insert(RelayRole::Content);
        connected_relays.insert(RelayRole::Indexer);

        // Sanity: with both lanes up, the historical `all` gate and the
        // production `claim_send_gate` agree — proving Fix A is behavior-
        // preserving on the all-lanes-healthy path iOS/TUI actually run.
        assert!(
            all_relays_connected(&connected_relays),
            "sanity: both lanes connected → the historical all-lane gate is true"
        );
        let relays_ready = claim_send_gate(&connected_relays);
        assert!(
            relays_ready,
            "both lanes connected → claim_send_gate is true (agrees with all-lane gate)"
        );

        let id = hex64("b2");
        let uri = nevent_uri_with_relays(&id, &["wss://relay.publisher.example"]);
        let _ = kernel.claim_event(uri, "view-happy".to_string(), relays_ready, false);

        assert!(
            kernel.event_claim_is_requested_for_test(&id),
            "with all lanes connected the claim sends — this is the path iOS/TUI exercise today"
        );
        assert_eq!(kernel.pending_event_claims_len_for_test(), 0);
    }
}
