//! Relay / outbound command arms.
//!
//! Split out of `dispatch/mod.rs` to keep that file under the 500-LOC
//! ceiling. Behavior is identical — these are the fire-and-forget
//! outbound arms (`EnqueueOutbound`, `SetReconnectPreamble`,
//! `UnregisterPersistentSub`) plus the `RelayCommand` family dispatcher.

use nmp_network::role::RelayRole;

use crate::actor::commands;
use crate::actor::relay_mgmt::{ensure_relay_worker, shutdown_relay_worker};
use crate::actor::relay_reconnect::reconnect_relays;
use crate::relay::{CanonicalRelayUrl, OutboundMessage};

use super::helpers::maybe_publish_relay_list_after_edit;
use super::ActorContext;
use crate::actor::RelayCommand;

/// `EnqueueOutbound` — D0-clean fire-and-forget outbound: route the frame
/// directly to `send_outbound` with the supplied role and URL. The sender
/// (e.g. a `RelayConnectedHook`) already holds no kernel reference and cannot
/// return `Vec<OutboundMessage>` directly; posting through this variant wakes
/// the actor (ADR-0072 §D3a) and delivers the frame on the actor thread
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
        RelayCommand::AddRelay { url, role } => add_relay(url, role, ctx),
        RelayCommand::RemoveRelay { url } => remove_relay(url, ctx),
        RelayCommand::ReconnectRelays => reconnect_relays_cmd(ctx),
        RelayCommand::SetRelayInfo {
            relay_url,
            doc_json,
        } => set_relay_info(relay_url, doc_json, ctx),
    }
}

fn add_relay(
    url: String,
    role: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // Snapshot before mutation so pure no-op edits do not re-publish kind:10002.
    let projection_before = ctx.kernel.configured_relays_snapshot().to_vec();
    let mut outbound = Vec::new();
    if let Some(canonical_url) = commands::add_relay(ctx.kernel, &url, &role) {
        ensure_relay_worker(
            ctx.relay_runtime,
            ctx.pool,
            ctx.kernel,
            RelayRole::Content,
            canonical_url,
        );
        outbound.extend(maybe_publish_relay_list_after_edit(
            ctx.identity,
            ctx.kernel,
            &projection_before,
            ctx.parked_ops,
        ));
    }
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

fn remove_relay(url: String, ctx: &mut ActorContext<'_>) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // Shutdown and kernel mutation both canonicalize URLs; idempotent for absent URLs.
    let projection_before = ctx.kernel.configured_relays_snapshot().to_vec();
    shutdown_relay_worker(ctx.relay_runtime, ctx.pool, &url);
    commands::remove_relay(ctx.kernel, &url);
    let outbound = maybe_publish_relay_list_after_edit(
        ctx.identity,
        ctx.kernel,
        &projection_before,
        ctx.parked_ops,
    );
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

fn reconnect_relays_cmd(ctx: &mut ActorContext<'_>) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // Fail closed before `Start`: nothing has consented to re-dial yet.
    if *ctx.running {
        reconnect_relays(ctx.relay_runtime, ctx.pool, ctx.kernel);
        maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    }
    Some(Vec::new())
}

fn set_relay_info(
    relay_url: String,
    doc_json: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    if let Some(doc) = crate::substrate::RelayInfoDoc::from_json(&doc_json) {
        ctx.kernel
            .set_relay_info_at(&relay_url, doc, ctx.dispatch_now);
        maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    }
    Some(Vec::new())
}
