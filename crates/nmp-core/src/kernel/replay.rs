//! T116 / G1 — reconnect-replay.
//!
//! Wires the actor's relay-reconnect event into the kernel's subscription
//! lifecycle. The actor reports the OS-level transition (`RelayEvent::Connected`
//! on a URL it has seen before); the kernel decides what to replay and when
//! (D7: kernel reports, actor decides — inverted here because the kernel is
//! the policy owner for subscription replay).
//!
//! See `docs/research/relay-lifecycle-and-pools.md` G1 for the gap that this
//! closes, and `docs/design/subscription-compilation/recompilation.md` §4.2
//! (A5 — `RelayReconnected`) for the semantics: pure replay of the current
//! plan's sub-shapes targeting this URL, with `since` bumped through the T129
//! watermark resolver so we don't re-download already-stored events.

use super::{json, Kernel, OutboundMessage, RelayRole, Value};
use crate::subs::WireFrame;

impl Kernel {
    /// T116 / G1 — replay every active wire-sub for `relay_url` after a true
    /// reconnect.
    ///
    /// The actor must call this only on `RelayEvent::Connected` for a URL the
    /// pool has seen before (i.e. NOT the first dial — see the
    /// `connected_urls` discriminator in `actor/dispatch.rs`). Returned
    /// `OutboundMessage`s are routed straight through `send_all_outbound` on
    /// the normal hot path; they target the freshly-reconnected socket via
    /// the URL-keyed transport pool (T105).
    ///
    /// Each returned REQ re-registers a `WireSub` row through
    /// [`Kernel::req_for_relay`] so the kernel's per-sub bookkeeping
    /// (`wire_subs` map; cleared by `relay_closed` at T133 eviction) is
    /// rebuilt at the same time. This is the path that lets EOSE/CLOSE
    /// arriving on the new socket correlate against the right sub-id.
    ///
    /// Watermark on replay: `SubscriptionLifecycle::handle_reconnect` clones
    /// each shape and applies `since = max(existing, watermark+1)` on the
    /// wire frame's filter so the relay does not re-emit events already
    /// resident in `EventStore`. The on-disk `current_plan` is left
    /// unchanged (the recompile path owns plan mutation).
    pub(crate) fn replay_on_reconnect(
        &mut self,
        role: RelayRole,
        relay_url: &str,
    ) -> Vec<OutboundMessage> {
        let frames = self.lifecycle.handle_reconnect(relay_url.to_string());
        if frames.is_empty() {
            return Vec::new();
        }
        self.log(format!(
            "replay {} subs onto reconnected {} (lane {})",
            frames.len(),
            relay_url,
            role.key()
        ));
        replay_frames_to_outbound(self, role, frames)
    }
}

/// Convert the lifecycle's replay frames into `OutboundMessage`s, re-recording
/// each REQ as a fresh `WireSub` row.
///
/// Unlike `auth_handlers::wire_frames_to_outbound` this helper does NOT gate on
/// `relay_url == role.url()` — `handle_reconnect` already scopes its output to
/// the target URL, and post-T105 most URLs are NOT the bootstrap host for
/// their role (the `AuthGate` helper's filter is a pre-T125 vestige that drops
/// every per-author/per-mailbox frame; reusing it would silently lose every
/// replay frame for non-bootstrap URLs).
///
/// REQ frames go through `req_for_relay` so the `wire_subs` registry is
/// repopulated post-T133 eviction; CLOSE frames flow through as-is (the
/// reconnect path normally produces only REQs, but the type allows both).
fn replay_frames_to_outbound(
    kernel: &mut Kernel,
    role: RelayRole,
    frames: Vec<WireFrame>,
) -> Vec<OutboundMessage> {
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        match frame {
            WireFrame::Req {
                relay_url,
                sub_id,
                filter_json,
                ..
            } => {
                // `req_for_relay` expects a `serde_json::Value` filter so it can
                // re-serialise inside `json!(["REQ", sub_id, filter])`. The
                // lifecycle's frame already carries a flat filter-object JSON
                // string — parse once and pass through. A malformed string
                // here would be an internal-invariant break (we wrote it
                // ourselves in `subs::wire::filter_json_for`), so we surface
                // by dropping the frame rather than panicking across FFI (D6).
                let Ok(filter_value) = serde_json::from_str::<Value>(&filter_json) else {
                    kernel.log(format!(
                        "replay: skipped malformed filter for sub {sub_id} on {relay_url}"
                    ));
                    continue;
                };
                let summary = format!("replay@{relay_url}");
                out.push(kernel.req_for_relay(role, relay_url, &sub_id, &summary, filter_value));
            }
            WireFrame::Close { relay_url, sub_id } => {
                out.push(OutboundMessage {
                    role,
                    relay_url,
                    text: json!(["CLOSE", sub_id]).to_string(),
                });
            }
        }
    }
    out
}
