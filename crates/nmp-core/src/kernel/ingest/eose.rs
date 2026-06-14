//! EOSE relay-frame handling, split out of `ingest/mod.rs` (LOC cap).
//!
//! The `handle_text` dispatch routes the `["EOSE", sub_id]` frame here. This is
//! the keep-live decision (follow-feed / firehose / persistent subs survive
//! EOSE; everything else is CLOSEd and evicted), the F-TTL freshness stamp, and
//! the K3 Stage D1 coverage-ledger write (ADR-0056 §3).

use serde_json::json;

use super::super::{CanonicalRelayUrl, Instant, Kernel, OutboundMessage, RelayRole};

impl Kernel {
    /// Handle an `EOSE` frame for `sub_id` delivered on `relay_url`.
    ///
    /// `wire_key_url` is the canonicalised delivering URL (the `wire_subs` /
    /// `persistent_subs` key half). Appends any resulting CLOSE frame to
    /// `outbound`.
    pub(super) fn handle_eose(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        sub_id: &str,
        wire_key_url: &CanonicalRelayUrl,
        outbound: &mut Vec<OutboundMessage>,
    ) {
        {
            let relay = self.relay_mut(role);
            relay.counters.eose_rx = relay.counters.eose_rx.saturating_add(1);
        }
        self.record_transport_eose(role, relay_url);
        // T105: the follow-feed (seed-timeline) is now per-relay
        // (`seed-timeline-<short-hash>`). Both the legacy id and its per-relay
        // variants stay live after EOSE. Persistent subs (NWC kind:23195
        // listener, …) registered via `register_persistent_sub` also survive.
        let keep_live = sub_id == "seed-timeline"
            || sub_id.starts_with("seed-timeline-")
            || sub_id.starts_with("diag-firehose-")
            || self.is_persistent_sub(wire_key_url, sub_id);
        let wire_key = (wire_key_url.clone(), sub_id.to_string());
        // `Some(since_floor)` iff the row existed (K3 Stage D1 reads it).
        let eose_row_floor: Option<Option<u64>> = self.wire.subs.get_mut(&wire_key).map(|sub| {
            sub.eose_at = Some(Instant::now());
            // T133: mark closed for the brief window before eviction below;
            // ingest readers (EVENT for an already-EOSE'd sub) see the row
            // absent. Keep-live stays "live".
            sub.state = if keep_live { "live" } else { "closed" }.to_string();
            sub.since_floor
        });
        // K3 Stage D1 (ADR-0056 §3) — record completed coverage at EOSE.
        if let Some(since_floor) = eose_row_floor {
            self.record_eose_coverage(sub_id, relay_url, since_floor, self.now_secs());
        }
        // V-112 (ADR-0042): thread-ids-/thread-replies- inflight-flag updates
        // deleted; thread_view state no longer exists in the kernel.
        // T82/T104: a discovery oneshot's first stored set has landed (OneShot
        // lifecycle == "EOSE closes"). Complete + release the token; the generic
        // CLOSE below tears down the wire sub. Dispatch is on the typed
        // OneshotKind stored in oneshot_subs (not a string-prefix scan).
        if self.is_discovery_oneshot(sub_id) {
            self.complete_unknown_oneshot(sub_id);
        }
        self.record_claim_expansion_eose_no_match(sub_id, relay_url);

        // F-TTL — handle EOSE for in-flight re-verification REQs. On EOSE the
        // relay has delivered everything it has for the reverify filter, so the
        // cached replaceable identity is now confirmed fresh: stamp each tracked
        // key's `check_again_after` forward by its per-kind TTL. This clears the
        // in-flight tracking and gates the next claim (claim_replaceable sees a
        // future timestamp and skips the REQ until the TTL elapses).
        // D9 clock seam: `now_ms()` reads the injected `Clock`.
        if let Some(keys) = self.reverify_subs.remove(sub_id) {
            let now = self.now_ms();
            for key in keys {
                let ttl_ms = self.replaceable_ttl.ttl_for_kind(key.kind()).as_millis() as u64;
                self.store.set_check_again_after(key, now + ttl_ms);
            }
        }

        if !keep_live {
            // T105/#170: CLOSE must travel back on the SAME socket the EOSE
            // arrived on — the transport pool is URL-keyed, so a role-only close
            // would target the bootstrap socket and leave the resolved sub open.
            // Pull the recorded URL from the relay-scoped WireSub row; fall back
            // to the delivering URL when the sub_id is unknown.
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
            // T133: evict the row now that the CLOSE outbound is queued. The
            // closed state is logically terminal for any sub that is not the live
            // follow-feed / firehose; keeping the row was a diagnostic-only
            // courtesy that grew the table unboundedly across long sessions
            // (every profile-claim, thread-ids, thread-replies, and discovery
            // oneshot completes via this EOSE→CLOSE path).
            self.wire.subs.remove(&wire_key);
        }
        self.changed_since_emit = true;
        self.log(format!("EOSE {sub_id}"));
    }
}
