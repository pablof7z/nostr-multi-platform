//! Relay / outbound command arms.
//!
//! Split out of `dispatch/mod.rs` to keep that file under the 500-LOC
//! ceiling. Behavior is identical — these are the fire-and-forget
//! outbound arms (`EnqueueOutbound`, `SetReconnectPreamble`,
//! `UnregisterPersistentSub`) plus the `RelayCommand` family dispatcher.

use nmp_network::role::RelayRole;

use crate::relay::{CanonicalRelayUrl, OutboundMessage};

use super::cmd_publish;
use super::ActorContext;
use crate::actor::RelayCommand;

/// `EnqueueOutbound` — D0-clean fire-and-forget outbound: route the frame
/// directly to `send_outbound` with the supplied role and URL. The sender
/// (e.g. a `RelayConnectedHook`) already holds no kernel reference and cannot
/// return `Vec<OutboundMessage>` directly; posting through this variant wakes
/// the actor (ADR-0050 §D3a) and delivers the frame on the actor thread
/// without any new mutex or blocking call.
pub(super) fn enqueue_outbound(
    role: RelayRole,
    relay_url: String,
    text: String,
) -> Option<Vec<OutboundMessage>> {
    Some(vec![OutboundMessage::new(role, relay_url, text)])
}

/// `SetReconnectPreamble` — REQ-before-EVENT fix (#2119): forward preamble to
/// pool. Stale handles silently ignored.
pub(super) fn set_reconnect_preamble(
    relay_url: String,
    frames: Vec<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    let canonical = CanonicalRelayUrl::parse_or_raw(&relay_url);
    if let Some(control) = ctx.relay_runtime.relay_controls.get(&canonical) {
        ctx.pool.set_reconnect_preamble(control.handle, frames);
    }
    Some(Vec::new())
}

/// `UnregisterPersistentSub` — cancel a persistent NIP-46 subscription. Removes
/// the sub_id from the kernel's persistent-sub registry so the relay worker no
/// longer prevents EOSE-triggered CLOSE. D0-clean (generic strings only).
pub(super) fn unregister_persistent_sub(
    relay_url: String,
    sub_id: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    ctx.kernel.unregister_persistent_sub(&relay_url, &sub_id);
    Some(Vec::new())
}

/// `RelayCommand` family dispatch.
pub(super) fn dispatch_relay(
    cmd: RelayCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        RelayCommand::AddRelay { url, role } => cmd_publish::add_relay(url, role, ctx),
        RelayCommand::RemoveRelay { url } => cmd_publish::remove_relay(url, ctx),
        RelayCommand::ReconnectRelays => cmd_publish::reconnect_relays_cmd(ctx),
        RelayCommand::SetRelayInfo {
            relay_url,
            doc_json,
        } => cmd_publish::set_relay_info(relay_url, doc_json, ctx),
    }
}
