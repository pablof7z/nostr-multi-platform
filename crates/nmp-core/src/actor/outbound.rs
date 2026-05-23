//! T142 — `WireFrame` → `OutboundMessage` conversion bridge.
//!
//! Converts planner-generated [`WireFrame`]s into actor-layer
//! [`OutboundMessage`]s, attaching the relay lane discriminator
//! (`RelayRole`) required by the transport pool.

use crate::kernel::Kernel;
use crate::relay::{canonical_relay_url, OutboundMessage, RelayRole};
use crate::subs::WireFrame;

/// Convert planner `WireFrame`s to actor `OutboundMessage`s for the relay pool.
///
/// Each `WireFrame` carries a resolved `relay_url` and a JSON-encoded REQ or
/// CLOSE frame. `OutboundMessage` additionally requires a `RelayRole` for the
/// transport-lane + diagnostics discriminator. This function looks up the role
/// from the kernel's relay-URL index (bootstrap-URL matching); unrecognized
/// URLs fall back to `RelayRole::Content`, which safely accepts content-fetch
/// REQs (spec §3.2 Option A).
///
/// Called only when `drain_lifecycle_tick()` returns a non-empty frame list —
/// the common empty-inbox case returns `Vec::new()` before reaching this path.
///
/// T140: this is the single point where planner frames cross into the
/// transport layer, so it also registers each frame into the kernel's wire-sub
/// / persistent-sub bookkeeping (`register_planner_wire_frames`) — the EOSE
/// keep-live predicate then keeps `Tailing` follow-feed subs open at parity
/// with the retired M1 `seed-timeline-*` path.
///
/// T-relay-url-normalize: each `WireFrame::relay_url` originates from a
/// `CompiledPlan::per_relay` key, which is in turn an NIP-65 mailbox URL
/// published verbatim in a `kind:10002` event — NOT guaranteed canonical
/// (trailing slash, uppercase scheme). The URL is canonicalized here, once,
/// before both the role lookup and the `OutboundMessage` stamp so the whole
/// actor-layer path agrees on a single key:
///   - `role_for_relay_url` does an exact `row.url == url` compare; a raw
///     non-canonical URL would miss the matching `relay_edit_rows` entry and
///     silently fall through to `RelayRole::Content`, mis-charging an indexer
///     relay to the Content diagnostic lane.
///   - `register_planner_wire_frames` already canonicalizes its own bookkeeping
///     key, and `send_outbound` canonicalizes the pool key — emitting the
///     canonical form here keeps `OutboundMessage.relay_url` consistent with
///     both rather than relying on a downstream re-canonicalization.
pub(super) fn wire_frames_to_outbound(
    frames: Vec<WireFrame>,
    kernel: &mut Kernel,
) -> Vec<OutboundMessage> {
    kernel.register_planner_wire_frames(&frames);
    frames
        .into_iter()
        .map(|f| {
            let (relay_url, text) = match f {
                WireFrame::Req {
                    relay_url,
                    sub_id,
                    filter_json,
                    ..
                } => {
                    let text = format!(r#"["REQ","{sub_id}",{filter_json}]"#);
                    (relay_url, text)
                }
                WireFrame::Close { relay_url, sub_id } => {
                    let text = format!(r#"["CLOSE","{sub_id}"]"#);
                    (relay_url, text)
                }
            };
            // Canonicalize once. A URL that does not parse as ws/wss falls
            // back to the raw string (no panic) — the same fail-open contract
            // `send_outbound` and `register_planner_wire_frames` use.
            let relay_url = canonical_relay_url(&relay_url).unwrap_or(relay_url);
            let role = kernel
                .role_for_relay_url(&relay_url)
                .unwrap_or(RelayRole::Content);
            OutboundMessage {
                role,
                relay_url,
                text,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::wire_frames_to_outbound;
    use crate::kernel::Kernel;
    use crate::planner::{InterestId, InterestLifecycle};
    use crate::subs::WireFrame;

    /// T-relay-url-normalize regression — a `WireFrame` carrying a non-canonical
    /// `relay_url` (uppercase scheme + empty-path trailing slash, exactly what
    /// an author can publish verbatim in a `kind:10002` event) must produce an
    /// `OutboundMessage` whose `relay_url` is the canonical form. Without the
    /// canonicalization the raw URL would miss `role_for_relay_url`'s exact
    /// string match and mis-charge the diagnostic lane.
    #[test]
    fn non_canonical_wire_frame_url_is_canonicalized_on_outbound() {
        let mut kernel = Kernel::new(50);
        let frames = vec![
            WireFrame::Req {
                relay_url: "WSS://R.Ex/".to_string(),
                sub_id: "sub-1".to_string(),
                filter_json: r#"{"kinds":[1]}"#.to_string(),
                interest_id: InterestId(1),
                lifecycle: InterestLifecycle::OneShot,
            },
            WireFrame::Close {
                relay_url: "WSS://R.Ex/".to_string(),
                sub_id: "sub-1".to_string(),
            },
        ];

        let outbound = wire_frames_to_outbound(frames, &mut kernel);
        assert_eq!(outbound.len(), 2);
        for msg in &outbound {
            assert_eq!(
                msg.relay_url, "wss://r.ex",
                "OutboundMessage.relay_url must be canonicalized (scheme \
                 lowercased, empty-path trailing slash stripped)"
            );
        }
    }

    /// A URL that cannot be canonicalized (bad scheme) is passed through
    /// verbatim — the fail-open contract shared with `send_outbound`. The frame
    /// must still be emitted, never dropped.
    #[test]
    fn uncanonicalizable_wire_frame_url_passes_through_verbatim() {
        let mut kernel = Kernel::new(50);
        let frames = vec![WireFrame::Close {
            relay_url: "http://not-a-relay".to_string(),
            sub_id: "sub-x".to_string(),
        }];

        let outbound = wire_frames_to_outbound(frames, &mut kernel);
        assert_eq!(outbound.len(), 1, "frame must not be dropped");
        assert_eq!(outbound[0].relay_url, "http://not-a-relay");
    }

    #[test]
    fn planner_wire_sub_diagnostics_show_exact_filter_json() {
        let mut kernel = Kernel::new(50);
        let filter_json = r##"{"authors":["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],"kinds":[1,6],"#t":["nostr"],"limit":20}"##;
        let frames = vec![WireFrame::Req {
            relay_url: "wss://relay.example/".to_string(),
            sub_id: "sub-filter-json".to_string(),
            filter_json: filter_json.to_string(),
            interest_id: InterestId(1),
            lifecycle: InterestLifecycle::Tailing,
        }];

        let _ = wire_frames_to_outbound(frames, &mut kernel);
        let update = kernel.make_update(true);
        let payload: serde_json::Value = serde_json::from_str(&update).expect("kernel update JSON");
        let sub = payload["wire_subscriptions"]
            .as_array()
            .expect("wireSubscriptions array")
            .iter()
            .find(|row| row["wire_id"] == "sub-filter-json")
            .expect("registered wire subscription");

        assert_eq!(
            sub["filter_summary"].as_str(),
            Some(filter_json),
            "subscription diagnostics must expose the exact REQ filter JSON"
        );
    }
}
