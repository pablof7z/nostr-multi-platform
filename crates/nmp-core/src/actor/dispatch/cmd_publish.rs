//! Publish, follow, relay-mutation, and action-record dispatch arms.
//!
//! Covers: `PublishRawEvent`, `PublishProfile`, `PublishUnsignedEvent`,
//! `PublishUnsignedEventToRelays`, `PublishSignedEvent`, `RetryPublish`,
//! `CancelPublish`, `Follow`, `Unfollow`, `FollowMany`,
//! `AddRelay`, `RemoveRelay`, `ReconnectRelays`,
//! `RecordActionFailure`, `RecordActionSuccess`, `AckActionStage`,
//! `SetRelayInfo`.
//!
//! Extracted from `dispatch.rs` to keep `mod.rs` under the LOC ceiling.
//! No behaviour change — all logic is verbatim from the original file.
//!
//! ADR-0065 — the `dispatch_publish` / `dispatch_contacts` /
//! `dispatch_relay` / `dispatch_action_ledger` functions below match the
//! `PublishCommand` / `ContactsCommand` / `RelayCommand` /
//! `ActionLedgerCommand` sub-enums and route each verb to its existing
//! handler.

use crate::actor::commands;
use crate::actor::relay_mgmt::{ensure_relay_worker, shutdown_relay_worker};
use crate::actor::relay_reconnect::reconnect_relays;
use crate::relay::OutboundMessage;

use super::helpers::maybe_publish_relay_list_after_edit;
use super::ActorContext;

/// Dispatch `ActorCommand::PublishRawEvent`.
pub(super) fn publish_raw_event(
    kind: u32,
    tags: Vec<Vec<String>>,
    content: String,
    target: crate::publish::PublishTarget,
    signer_pubkey: Option<String>,
    correlation_id: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // D7: kernel owns the wall clock. Unlike `PublishUnsignedEvent`
    // below — whose callers (NIP-crate executors) set the sentinel
    // `created_at: 0` and rely on the dispatch arm to stamp — this
    // arm builds the `UnsignedEvent` itself, so we stamp inline
    // from `kernel.now_secs()` directly. Same effect, no sentinel
    // round-trip required. The FixedClock test hook plugs into
    // `kernel.now_secs()`, so end-to-end behaviour is preserved.
    //
    // `pubkey` is intentionally left empty: both
    // `publish_unsigned_event` and `publish_unsigned_event_to_relays`
    // ignore the caller's `unsigned.pubkey` and write the active
    // identity's pubkey onto the SignedEvent at sign time. Setting
    // it here would be dead work.
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind,
        tags,
        content,
        created_at: ctx.kernel.now_secs(),
    };
    if let Some(ref cid) = correlation_id {
        ctx.kernel.record_action_stage(
            cid,
            crate::kernel::action_stages::ActionStage::Requested,
            None,
        );
    }
    // Route on `target`: `Auto` resolves via NIP-65 outbox (D3);
    // `Explicit { relays }` pins to exactly those relays. Both
    // helpers handle local-keys (sync sign) and bunker (parked
    // ParkedOp Publish sink) paths internally — `PublishRaw` inherits the
    // same identity-kind support as `PublishProfile`.
    let outbound = match target {
        crate::publish::PublishTarget::Auto => commands::publish_unsigned_event(
            ctx.identity,
            ctx.kernel,
            unsigned,
            correlation_id,
            // Honour the `PublishRaw` signer selector: `None` signs with
            // the active account; `Some(pubkey)` signs with that
            // registered app-managed signer slot.
            signer_pubkey,
            ctx.parked_ops,
        ),
        crate::publish::PublishTarget::Explicit { relays } => {
            commands::publish_unsigned_event_to_relays(
                ctx.identity,
                ctx.kernel,
                unsigned,
                relays,
                correlation_id,
                // Honour the `PublishRaw` signer selector: `None` signs
                // with the active account; `Some(pubkey)` signs with that
                // registered app-managed signer slot.
                signer_pubkey,
                ctx.parked_ops,
            )
        }
    };
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::PublishProfile`.
pub(super) fn publish_profile(
    fields: serde_json::Map<String, serde_json::Value>,
    correlation_id: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    if let Some(ref cid) = correlation_id {
        ctx.kernel.record_action_stage(
            cid,
            crate::kernel::action_stages::ActionStage::Requested,
            None,
        );
    }
    let outbound = commands::publish_profile(
        ctx.identity,
        ctx.kernel,
        fields,
        correlation_id,
        ctx.parked_ops,
    );
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::PublishUnsignedEvent`.
pub(super) fn publish_unsigned_event(
    mut unsigned: nmp_signer_iface::UnsignedEvent,
    correlation_id: Option<String>,
    signer_pubkey: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::emit_now;
    // D7: apply the same created_at=0 sentinel as PublishUnsignedEventToRelays.
    // A host that builds an UnsignedEvent without setting created_at gets
    // the kernel clock rather than epoch time.
    if unsigned.created_at == 0 {
        unsigned.created_at = ctx.kernel.now_secs();
    }
    if let Some(ref cid) = correlation_id {
        ctx.kernel.record_action_stage(
            cid,
            crate::kernel::action_stages::ActionStage::Requested,
            None,
        );
    }
    let outbound = commands::publish_unsigned_event(
        ctx.identity,
        ctx.kernel,
        unsigned,
        correlation_id,
        signer_pubkey,
        ctx.parked_ops,
    );
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::PublishUnsignedEventToRelays`.
pub(super) fn publish_unsigned_event_to_relays(
    mut event: nmp_signer_iface::UnsignedEvent,
    relays: Vec<String>,
    correlation_id: Option<String>,
    signer_pubkey: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::emit_now;
    // D7: kernel owns the wall clock. Executors in NIP crates set
    // created_at = 0 as a sentinel; we re-stamp here so they never
    // call SystemTime::now() and the FixedClock test hook stays
    // effective end-to-end.
    if event.created_at == 0 {
        event.created_at = ctx.kernel.now_secs();
    }
    if let Some(ref cid) = correlation_id {
        ctx.kernel.record_action_stage(
            cid,
            crate::kernel::action_stages::ActionStage::Requested,
            None,
        );
    }
    let outbound = commands::publish_unsigned_event_to_relays(
        ctx.identity,
        ctx.kernel,
        event,
        relays,
        correlation_id,
        signer_pubkey,
        ctx.parked_ops,
    );
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::PublishSignedEvent`.
pub(super) fn publish_signed_event(
    raw: crate::store::RawEvent,
    target: crate::publish::PublishTarget,
    correlation_id: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::emit_now;
    if let Some(ref cid) = correlation_id {
        ctx.kernel.record_action_stage(
            cid,
            crate::kernel::action_stages::ActionStage::Requested,
            None,
        );
    }
    let outbound = commands::publish_signed_event(ctx.kernel, raw, target, correlation_id);
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::RetryPublish`.
pub(super) fn retry_publish(
    handle: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::emit_now;
    let outbound = ctx.kernel.retry_publish_now(&handle);
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::CancelPublish`.
pub(super) fn cancel_publish(
    correlation_id: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::emit_now;
    ctx.kernel.cancel_publish(&correlation_id);
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::Follow` / `ActorCommand::Unfollow`.
pub(super) fn follow_or_unfollow(
    pubkey: String,
    follow: bool,
    correlation_id: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    if let Some(ref cid) = correlation_id {
        ctx.kernel.record_action_stage(
            cid,
            crate::kernel::action_stages::ActionStage::Requested,
            None,
        );
    }
    let outbound = commands::follow(
        ctx.identity,
        ctx.kernel,
        &pubkey,
        follow,
        correlation_id,
        ctx.parked_ops,
    );
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::FollowMany`.
pub(super) fn follow_many(
    pubkeys: Vec<String>,
    correlation_id: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    if let Some(ref cid) = correlation_id {
        ctx.kernel.record_action_stage(
            cid,
            crate::kernel::action_stages::ActionStage::Requested,
            None,
        );
    }
    let outbound = commands::follow_many(
        ctx.identity,
        ctx.kernel,
        &pubkeys,
        None, // active_pubkey_hint: actor has the live pubkey internally
        correlation_id,
        ctx.parked_ops,
    );
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::AddRelay`.
pub(super) fn add_relay(
    url: String,
    role: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // T158: add_relay now returns Some(canonical_url) on success so we
    // can dial a real socket immediately. User-added relays use
    // RelayRole::Content as the diagnostic lane (inbox/outbox bucket);
    // the NIP-65 read/write distinction lives in AppRelay, not in
    // the transport pool key (T105). ensure_relay_worker is idempotent —
    // a role-edit for an already-connected URL is a harmless no-op.
    //
    // T-nip65-auto-publish: snapshot the projection BEFORE the mutation
    // so we can compare-and-skip the re-publish when the call was a
    // pure no-op (re-adding the same URL with the same role). Without
    // this every harmless re-add re-published kind:10002 and burned a
    // relay write.
    let projection_before = ctx.kernel.configured_relays_snapshot().to_vec();
    let mut outbound = Vec::new();
    if let Some(canonical_url) = commands::add_relay(ctx.kernel, &url, &role) {
        ensure_relay_worker(
            ctx.relay_runtime,
            ctx.pool,
            ctx.kernel,
            nmp_network::role::RelayRole::Content,
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

/// Dispatch `ActorCommand::RemoveRelay`.
pub(super) fn remove_relay(
    url: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // T162 + T-relay-url-normalize: both shutdown_relay_worker and
    // commands::remove_relay canonicalize the URL internally (lowercase
    // scheme+host, strip empty-path trailing slash) so that the pool key
    // and AppRelay.url always agree regardless of how the FFI caller
    // spelled the URL. Shutdown the worker first so the socket is closed
    // before the projection row is removed. Idempotent: if no worker exists
    // for the URL, shutdown_relay_worker returns false and the projection
    // mutation still proceeds normally (D6: no silent drops).
    //
    // T-nip65-auto-publish: same compare-and-skip as `AddRelay` above.
    // Removing a URL that was never present is a no-op and must NOT
    // re-publish kind:10002.
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

/// Dispatch `ActorCommand::Relay(RelayCommand::ReconnectRelays)`.
pub(super) fn reconnect_relays_cmd(ctx: &mut ActorContext<'_>) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // #1689: kernel-driven "reconnect all". Fail-closed — a no-op before
    // `Start` (nothing consented to re-dial; never dial unconsented relays).
    if *ctx.running {
        reconnect_relays(ctx.relay_runtime, ctx.pool, ctx.kernel);
        maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    }
    Some(Vec::new())
}

/// Dispatch `ActorCommand::RecordActionFailure`.
pub(super) fn record_action_failure(
    correlation_id: String,
    reason: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // Writes `Failed { reason }` to `action_stages` and a terminal
    // verdict to `action_results` — both surfaces the host uses to
    // clear the spinner. Without this, an executor that fails before
    // emitting an ActorCommand would orphan the correlation_id.
    //
    // Prose-only (#1735): the `reason` is whatever the failing executor
    // supplied (opaque upstream / protocol-crate diagnostic text), not
    // curated kernel app copy — un-coded, mirroring #1711's guard. (An
    // executor that wants a localizable failure carries its own code on
    // the S7 lifecycle wire, #1754.)
    ctx.kernel.record_action_failure(correlation_id, reason);
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::SetRelayInfo`.
pub(super) fn set_relay_info(
    relay_url: String,
    doc_json: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // ADR-0051 — fold the nmp-nip11 fetch result onto the kernel's
    // per-URL transport row (marks the snapshot dirty so the
    // `relay_diagnostics` projection surfaces it). Malformed JSON is a
    // silent no-op (D6).
    if let Some(doc) = crate::substrate::RelayInfoDoc::from_json(&doc_json) {
        ctx.kernel
            .set_relay_info_at(&relay_url, doc, ctx.dispatch_now);
        maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    }
    Some(Vec::new())
}

/// Dispatch `ActorCommand::RecordActionSuccess`.
pub(super) fn record_action_success(
    correlation_id: String,
    result_json: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // Symmetric counterpart to RecordActionFailure: off-thread workers
    // and runtime responders fan success back through the actor
    // channel. Writes `Accepted` to `action_stages` and a terminal
    // verdict to `action_results`. `result_json` (ADR-0043 Decision 4)
    // rides into the `action_results` row's `result` field verbatim.
    ctx.kernel
        .record_action_success(correlation_id, result_json);
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::AckActionStage`.
pub(super) fn ack_action_stage(
    correlation_id: String,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    ctx.kernel.ack_action_stage(&correlation_id);
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}
