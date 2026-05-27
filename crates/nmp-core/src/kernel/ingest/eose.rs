//! EOSE-frame handling extracted from `ingest/mod.rs`.
//!
//! `handle_eose` is the single entry point, called from `handle_text` whenever
//! the relay sends an `["EOSE", sub_id]` frame. It owns:
//!
//! - Counter / transport trace updates
//! - Wire-sub lifecycle: keep-live decision, thread-view inflight flags
//! - Discovery-oneshot completion (T82/T104 typed routing)
//! - W3 claim-expansion EoseNoMatch scoring hook (dormant until W5)
//! - CLOSE emission + wire-sub eviction for non-persistent subs (T105/T133)
//!
//! # Extraction rationale
//!
//! `ingest/mod.rs` was over the 500-LOC hard cap (AGENTS.md). Moving the
//! ~75-LOC EOSE arm here brings `mod.rs` back under the cap.

use super::super::{json, CanonicalRelayUrl, Instant, Kernel, OutboundMessage, RelayRole};

impl Kernel {
    /// Process an `["EOSE", sub_id]` relay frame.
    ///
    /// Returns any outbound [`OutboundMessage`]s produced (typically one
    /// `CLOSE` for non-persistent subscriptions).
    ///
    /// Called exclusively from [`Kernel::handle_text`] in `ingest/mod.rs`.
    pub(super) fn handle_eose(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        wire_key_url: &CanonicalRelayUrl,
        sub_id: &str,
    ) -> Vec<OutboundMessage> {
        let mut outbound = Vec::new();

        {
            let relay = self.relay_mut(role);
            relay.counters.eose_rx = relay.counters.eose_rx.saturating_add(1);
        }
        self.record_transport_eose(role, relay_url);

        // T105: the follow-feed (seed-timeline) is now per-relay
        // (`seed-timeline-<short-hash>`). Both the legacy id and its
        // per-relay variants stay live after EOSE. Persistent subs
        // (NWC kind:23195 listener, …) registered via
        // `register_persistent_sub` also survive EOSE.
        let keep_live = sub_id == "seed-timeline"
            || sub_id.starts_with("seed-timeline-")
            || sub_id.starts_with("diag-firehose-")
            || self.is_persistent_sub(wire_key_url, sub_id);
        let wire_key = (wire_key_url.clone(), sub_id.to_string());
        if let Some(sub) = self.wire.subs.get_mut(&wire_key) {
            sub.eose_at = Some(Instant::now());
            if keep_live {
                sub.state = "live".to_string();
            } else {
                // T133: mark closed for the brief window before
                // eviction below; ingest path readers (e.g. EVENT for
                // an already-EOSE'd sub) will see the row absent.
                sub.state = "closed".to_string();
            }
        }
        if sub_id.starts_with("thread-ids-") {
            self.thread_view.ids_inflight = false;
        }
        if sub_id.starts_with("thread-replies-") {
            self.thread_view.replies_inflight = false;
        }
        // T82/T104: a discovery oneshot's first stored set has landed
        // (OneShot lifecycle == "EOSE closes"). Complete + release the
        // token; the generic CLOSE below tears down the wire sub.
        // Dispatch is on the typed OneshotKind stored in oneshot_subs
        // (not a string-prefix scan — T104 typed routing).
        if self.is_discovery_oneshot(sub_id) {
            self.complete_unknown_oneshot(sub_id);
        }

        // W3 — claim-expansion EoseNoMatch hook.
        //
        // Only records EoseNoMatch when no accepted matching EVENT was seen
        // for this `(sub_id, relay_url)` in the current window
        // (`record_claim_expansion_eose_no_match` checks the match-seen seam).
        // Dormant until W5 populates `claim_sub_index`.
        //
        // D0: keys are (author, relay_url) — no protocol noun.
        // D4: `&mut self` — sole writer.
        // D8: called from an already-edge-triggered frame-ingest seam.
        self.record_claim_expansion_eose_no_match(sub_id, relay_url);

        if !keep_live {
            // T105: CLOSE must travel back to the same socket the REQ
            // went out on — the transport pool is URL-keyed, so a
            // role-only close would target the bootstrap socket and
            // leave the resolved sub open. Pull the recorded URL from
            // the WireSub set on req_for_relay; fall back to the
            // delivering relay's URL when the sub_id is unknown.
            // #170: the CLOSE travels back on the SAME socket the
            // EOSE arrived on (relay_url) — the wire_subs key is now
            // relay-scoped so the row, if any, is this relay's row,
            // not a sibling's. Fall back to the delivering URL.
            let close_url = self
                .wire
                .subs
                .get(&wire_key)
                .map_or_else(|| relay_url.to_string(), |sub| sub.relay_url.to_string());
            outbound.push(OutboundMessage {
                role,
                relay_url: close_url,
                text: json!(["CLOSE", sub_id]).to_string(),
            });
            // T133: evict the row now that the CLOSE outbound is
            // queued. The closed state is logically terminal for any
            // sub that is not the live follow-feed / firehose; keeping
            // the row was a diagnostic-only courtesy that grew the
            // table unboundedly across long sessions (every
            // profile-claim, thread-ids, thread-replies, and discovery
            // oneshot completes via this EOSE→CLOSE path).
            self.wire.subs.remove(&wire_key);
        }
        self.changed_since_emit = true;
        self.log(format!("EOSE {sub_id}"));

        outbound
    }
}
