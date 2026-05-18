//! T142 — WireFrame → OutboundMessage conversion bridge.
//!
//! Converts planner-generated [`WireFrame`]s into actor-layer
//! [`OutboundMessage`]s, attaching the relay lane discriminator
//! (`RelayRole`) required by the transport pool.

use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole};
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
pub(super) fn wire_frames_to_outbound(
    frames: Vec<WireFrame>,
    kernel: &Kernel,
) -> Vec<OutboundMessage> {
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
