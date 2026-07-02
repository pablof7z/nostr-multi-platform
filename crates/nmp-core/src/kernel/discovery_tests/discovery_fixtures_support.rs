//! Shared fixtures for the discovery-seam suite: fixed hex ids/pubkeys, a tag
//! builder, the planner-extension bootstrap-relay installer, the
//! drain-then-bridge helper that mirrors the production actor's
//! `drain_lifecycle_tick` → `register_planner_wire_frames` pipeline, and a
//! `WireFrame::Req` filter collector.

use crate::kernel::Kernel;
use crate::subs::WireFrame;

pub(super) const QUOTED_ID: &str =
    "1111111111111111111111111111111111111111111111111111111111111111";
pub(super) const MENTIONED_PK: &str =
    "2222222222222222222222222222222222222222222222222222222222222222";
pub(super) const KNOWN_ID: &str =
    "3333333333333333333333333333333333333333333333333333333333333333";

pub(super) const BOOTSTRAP_CONTENT: &str = "wss://bootstrap-content.test/";
pub(super) const BOOTSTRAP_INDEXER: &str = "wss://bootstrap-indexer.test/";

pub(super) fn tag(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Configure the lifecycle's planner-extension bootstrap relay lanes (PD-033-C
/// PR #365) so the planner has somewhere to land kernel-driven discovery
/// oneshots. Production wires these from `bootstrap_urls_for_role` in
/// `identity_state::set_configured_relays`; tests that construct
/// a bare `Kernel::new` install them directly.
///
/// Also clears the `cfg(test)` default indexer relay so
/// assertions can pin discovery REQs to BOOTSTRAP_CONTENT / BOOTSTRAP_INDEXER
/// rather than collapsing onto the default indexer fallback path.
pub(super) fn install_bootstrap_relays(kernel: &mut Kernel) {
    let lifecycle = kernel.lifecycle_mut();
    lifecycle.set_indexer_relays(vec![]);
    lifecycle.set_bootstrap_content_relays(vec![BOOTSTRAP_CONTENT.to_string()]);
    lifecycle.set_bootstrap_indexer_relays(vec![BOOTSTRAP_INDEXER.to_string()]);
}

/// Compile-and-register: run the planner's `drain_tick`, then push the
/// emitted frames through the kernel's `register_planner_wire_frames` bridge
/// so `oneshot_subs` is populated under the planner-assigned `sub_id`. This
/// mirrors the production actor's `drain_lifecycle_tick` →
/// `wire_frames_to_outbound` pipeline (`actor/mod.rs:1346-1357` +
/// `actor/outbound.rs`).
pub(super) fn drain_and_register(kernel: &mut Kernel) -> Vec<WireFrame> {
    let frames = kernel.drain_lifecycle_tick();
    kernel.register_wire_frames_for_test(&frames);
    frames
}

/// Collect every `WireFrame::Req` filter string emitted on this tick.
pub(super) fn planner_req_filters(frames: &[WireFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { filter_json, .. } => Some(filter_json.clone()),
            _ => None,
        })
        .collect()
}
