//! Publish and follow dispatch arms.

use crate::actor::commands;
use crate::relay::OutboundMessage;

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
    // `pubkey` is intentionally left empty; the signer writes it at sign time.
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
            None,
            correlation_id,
            signer_pubkey,
            ctx.parked_ops,
        ),
        crate::publish::PublishTarget::Explicit {
            relays,
            route_class,
        } => commands::publish_unsigned_event_to_relays(
            ctx.identity,
            ctx.kernel,
            unsigned,
            None,
            relays,
            route_class,
            correlation_id,
            signer_pubkey,
            ctx.parked_ops,
        ),
    };
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::PublishReply`.
pub(super) fn publish_reply(
    content: String,
    reply_to_event_id: String,
    target: crate::publish::PublishTarget,
    signer_pubkey: Option<String>,
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
    let Some(pubkey) = ctx.identity.active_pubkey() else {
        let reason = "no active account for publish reply".to_string();
        ctx.kernel.set_last_error_token(
            &crate::ui_token::UiToken::error(
                crate::ui_token::codes::PUBLISH_REPLY_TARGET_UNKNOWN,
                reason.clone(),
            )
            .with_subject("active-account"),
        );
        if let Some(id) = correlation_id {
            ctx.kernel.record_action_failure(id, reason);
        }
        maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
        return Some(Vec::new());
    };
    let intent = crate::substrate::DraftIntent::Reply {
        content,
        reply_to_event_id: reply_to_event_id.clone(),
    };
    let unsigned = match ctx
        .kernel
        .build_draft(&intent, &pubkey, ctx.kernel.now_secs())
    {
        Ok(unsigned) => unsigned,
        Err(err) => {
            let reason = err.to_string();
            ctx.kernel.set_last_error_token(
                &crate::ui_token::UiToken::error(
                    crate::ui_token::codes::PUBLISH_REPLY_TARGET_UNKNOWN,
                    reason.clone(),
                )
                .with_subject(reply_to_event_id),
            );
            if let Some(id) = correlation_id {
                ctx.kernel.record_action_failure(id, reason);
            }
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            return Some(Vec::new());
        }
    };
    let outbound = match target {
        crate::publish::PublishTarget::Auto => commands::publish_unsigned_event(
            ctx.identity,
            ctx.kernel,
            unsigned,
            None,
            correlation_id,
            signer_pubkey,
            ctx.parked_ops,
        ),
        crate::publish::PublishTarget::Explicit {
            relays,
            route_class,
        } => commands::publish_unsigned_event_to_relays(
            ctx.identity,
            ctx.kernel,
            unsigned,
            None,
            relays,
            route_class,
            correlation_id,
            signer_pubkey,
            ctx.parked_ops,
        ),
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
    let outbound = match ctx.identity.active_pubkey() {
        Some(pubkey) => {
            let intent = crate::substrate::DraftIntent::Profile { fields };
            match ctx
                .kernel
                .build_draft(&intent, &pubkey, ctx.kernel.now_secs())
            {
                Ok(unsigned) => commands::publish_unsigned_event(
                    ctx.identity,
                    ctx.kernel,
                    unsigned,
                    None,
                    correlation_id,
                    None,
                    ctx.parked_ops,
                ),
                Err(err) => commands::publish_failures::fail_publish(
                    ctx.kernel,
                    format!("profile draft: {err}"),
                    correlation_id,
                ),
            }
        }
        None => commands::publish_failures::toast_no_account(
            ctx.kernel,
            "publish profile",
            correlation_id,
        ),
    };
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}

/// Dispatch `ActorCommand::PublishUnsignedEvent`.
pub(super) fn publish_unsigned_event(
    mut unsigned: nmp_signer_iface::UnsignedEvent,
    ownership: Option<nmp_ownership::EventOwnershipProvenance>,
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
        ownership,
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
    ownership: Option<nmp_ownership::EventOwnershipProvenance>,
    relays: Vec<String>,
    route_class: crate::publish::PublishRouteClass,
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
        ownership,
        relays,
        route_class,
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
