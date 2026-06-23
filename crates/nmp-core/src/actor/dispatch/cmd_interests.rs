//! Interest, pull-cursor, and test-support dispatch arms.
//!
//! Covers: `PushInterest`, `WithdrawInterest`, `EnsureInterest`,
//! `DropInterestOwner`, `OpenPullCursor`, `AdvancePullCursor`,
//! `UnregisterPullCursor`, `OpenInterest`, `OpenObservedInterest`,
//! `CloseInterest`, and the `#[cfg(test)]` ingest/GC arms.
//!
//! Extracted from `dispatch/mod.rs` to keep it under the 500-LOC ceiling.
//! No behaviour change — all logic is verbatim from the original file.
//!
//! ADR-0065 — the `dispatch` function below matches the `InterestsCommand`
//! sub-enum and routes each verb to its existing handler.

use crate::actor::InterestsCommand;
use crate::actor::KernelEventObserverId;
use crate::relay::OutboundMessage;

use super::build_open_interest;
use super::ActorContext;

/// Dispatch `ActorCommand::PushInterest`.
pub(super) fn push_interest(
    interest: crate::planner::LogicalInterest,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    // Route through the unified front-door. Derive the legacy identity
    // (synthetic single owner, planner-interest-id key) so the slot
    // matches what WithdrawInterest reconstructs for teardown.
    let identity = crate::subs::SubIdentity::from_legacy_interest(&interest);
    ctx.kernel.register_interest(
        &[crate::kernel::cache_serve::InterestRegistration {
            identity,
            interest,
            policy: crate::kernel::cache_serve::InterestWrite::Replace,
        }],
        "push-interest",
    );
    Some(Vec::new())
}

/// Dispatch `ActorCommand::WithdrawInterest`.
pub(super) fn withdraw_interest(
    id: crate::planner::InterestId,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    // Reconstruct the SubKey the legacy push path minted for this id,
    // then drop every slot carrying that key (covers all scopes).
    let key = crate::subs::InterestRegistry::legacy_key(&id);
    ctx.kernel
        .lifecycle_mut()
        .registry_mut()
        .drop_slot_by_key(key);
    ctx.kernel.lifecycle_mut().enqueue_trigger(
        crate::subs::CompileTrigger::InvalidateCompile {
            reason: crate::subs::InvalidateReason::External("withdraw-interest".to_string()),
        },
    );
    Some(Vec::new())
}

/// Dispatch `ActorCommand::EnsureInterest`.
pub(super) fn ensure_interest(
    identity: crate::subs::SubIdentity,
    interest: crate::planner::LogicalInterest,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    // Unified front-door — register-if-absent (EnsureAbsent). Store-serve
    // + recompile trigger fire only when the interest is newly installed.
    ctx.kernel.register_interest(
        &[crate::kernel::cache_serve::InterestRegistration {
            identity,
            interest,
            policy: crate::kernel::cache_serve::InterestWrite::EnsureAbsent,
        }],
        "ensure-interest",
    );
    Some(Vec::new())
}

/// Dispatch `ActorCommand::DropInterestOwner`.
pub(super) fn drop_interest_owner(
    identity: crate::subs::SubIdentity,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    let removed = ctx
        .kernel
        .lifecycle_mut()
        .registry_mut()
        .drop_owner(&identity);
    if removed {
        ctx.kernel.lifecycle_mut().enqueue_trigger(
            crate::subs::CompileTrigger::InvalidateCompile {
                reason: crate::subs::InvalidateReason::External(
                    "drop-interest-owner".to_string(),
                ),
            },
        );
    }
    Some(Vec::new())
}

/// Dispatch `ActorCommand::OpenPullCursor`.
pub(super) fn open_pull_cursor(
    handle: crate::kernel::pull_cursor::PullCursorHandle,
    spec: crate::kernel::pull_cursor::PullCursorSpec,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    ctx.kernel.open_pull_cursor(handle, spec);
    Some(Vec::new())
}

/// Dispatch `InterestsCommand::RegisterPullCursor` (ADR-0065 / ADR-0058 step 3a).
/// Bridges the new sub-enum fields → the existing kernel `open_pull_cursor` call.
pub(super) fn register_pull_cursor(
    cursor_id: u64,
    consumer_id: String,
    scope: crate::kernel::pull::PullScope,
    mode: crate::kernel::pull_cursor::PullCursorMode,
    after_seq: u64,
    limits: crate::kernel::pull::PullLimits,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::kernel::pull_cursor::{PullConsumerId, PullCursorHandle, PullCursorSpec};
    let handle = PullCursorHandle::from_dispatch_id(cursor_id);
    let spec = PullCursorSpec {
        consumer_id: PullConsumerId(consumer_id),
        scope,
        mode,
        after_seq,
        limits,
    };
    ctx.kernel.open_pull_cursor(handle, spec);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::AdvancePullCursor`.
pub(super) fn advance_pull_cursor(
    cursor_id: crate::kernel::pull_cursor::PullCursorId,
    after_seq: u64,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    ctx.kernel.advance_pull_cursor(cursor_id, after_seq);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::UnregisterPullCursor`.
pub(super) fn unregister_pull_cursor(
    cursor_id: crate::kernel::pull_cursor::PullCursorId,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    ctx.kernel.unregister_pull_cursor(cursor_id);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::OpenInterest`.
pub(super) fn open_interest(
    filter_json: String,
    consumer_id: String,
    scope: u32,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // M2 (ADR-0042) — generic feed-subscription front door. Parse the
    // verbatim NIP-01 filter into an InterestShape, derive the
    // `(owner, key, scope)` identity from it, and run the same
    // ensure_sub + CompileTrigger body as the `EnsureInterest` arm.
    // D6: a malformed filter is a silent no-op (the FFI shim already
    // surfaced a toast before sending — see `nmp_app_open_interest`).
    if let Some((identity, interest)) =
        build_open_interest(&filter_json, &consumer_id, scope, None)
    {
        let _ = ctx.kernel.open_interest_sub(identity, interest);
    }
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::OpenObservedInterest`.
pub(super) fn open_observed_interest(
    filter_json: String,
    consumer_id: String,
    scope: u32,
    relay_pin: Option<String>,
    observer_id: KernelEventObserverId,
    replay_shapes: Vec<crate::planner::InterestShape>,
    replay_limit: usize,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // ADR-0062 — open interest + catch-up replay to a single muted observer,
    // then activate it. D6: a malformed filter is a silent no-op.
    if let Some((identity, interest)) =
        build_open_interest(&filter_json, &consumer_id, scope, relay_pin.as_deref())
    {
        let replay = crate::kernel::ObserverReplayRequest {
            observer_id,
            shapes: replay_shapes,
            limit: replay_limit,
        };
        let _ = ctx.kernel.open_interest_with_observer_replay(
            identity,
            interest,
            replay,
            "open-observed-interest",
        );
    }
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::CloseInterest`.
pub(super) fn close_interest(
    filter_json: String,
    consumer_id: String,
    scope: u32,
    relay_pin: Option<String>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // M2 (ADR-0042) — detach one owner; drop the live sub on the last
    // leave. The `(owner, key, scope)` identity is reconstructed from
    // the SAME filter + consumer + scope + relay_pin the open used, so
    // the InterestShape hash lands on the same registry slot.
    if let Some((identity, _interest)) =
        build_open_interest(&filter_json, &consumer_id, scope, relay_pin.as_deref())
    {
        let _ = ctx.kernel.close_interest_sub(&identity);
    }
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::IngestPreVerifiedEvents` (test-support only).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn ingest_pre_verified_events(
    events: Vec<crate::store::VerifiedEvent>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // D4: actor thread is the sole mutator. sort_timeline() deferred to
    // after the loop to avoid O(n²·log n) for large batches.
    for verified in events {
        ctx.kernel.ingest_pre_verified_event(
            crate::relay::RelayRole::Content,
            "diag-firehose-stress",
            verified,
        );
    }
    ctx.kernel.sort_timeline_deferred();
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::IngestPreVerifiedEventsForSubId` (test-support only).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn ingest_pre_verified_events_for_sub_id(
    sub_id: String,
    events: Vec<crate::store::VerifiedEvent>,
    ack: std::sync::mpsc::SyncSender<()>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    for verified in events {
        ctx.kernel.ingest_pre_verified_event(
            crate::relay::RelayRole::Content,
            &sub_id,
            verified,
        );
    }
    ctx.kernel.sort_timeline_deferred();
    maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    let _ = ack.send(());
    Some(Vec::new())
}

/// Dispatch `ActorCommand::TriggerGcStep` (test-support only).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn trigger_gc_step(
    ack: std::sync::mpsc::SyncSender<()>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    ctx.kernel.run_gc_step();
    let _ = ack.send(());
    Some(Vec::new())
}

/// ADR-0065 — `InterestsCommand` family dispatch. Matches the sub-enum and
/// routes each verb to its existing handler.
pub(super) fn dispatch(
    cmd: InterestsCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        InterestsCommand::PushInterest(interest) => push_interest(interest, ctx),
        InterestsCommand::WithdrawInterest(id) => withdraw_interest(id, ctx),
        InterestsCommand::EnsureInterest { identity, interest } =>
            ensure_interest(identity, interest, ctx),
        InterestsCommand::DropInterestOwner(identity) => drop_interest_owner(identity, ctx),
        InterestsCommand::RegisterPullCursor { cursor_id, consumer_id, scope, mode, after_seq, limits } =>
            register_pull_cursor(cursor_id, consumer_id, scope, mode, after_seq, limits, ctx),
        InterestsCommand::AdvancePullCursor { cursor_id, after_seq } =>
            advance_pull_cursor(crate::kernel::pull_cursor::PullCursorId(cursor_id), after_seq, ctx),
        InterestsCommand::UnregisterPullCursor { cursor_id } =>
            unregister_pull_cursor(crate::kernel::pull_cursor::PullCursorId(cursor_id), ctx),
        InterestsCommand::OpenInterest { filter_json, consumer_id, scope } =>
            open_interest(filter_json, consumer_id, scope, ctx),
        InterestsCommand::OpenObservedInterest {
            filter_json, consumer_id, scope, relay_pin,
            observer_id, replay_shapes, replay_limit,
        } => open_observed_interest(
            filter_json, consumer_id, scope, relay_pin,
            observer_id, replay_shapes, replay_limit, ctx,
        ),
        InterestsCommand::CloseInterest { filter_json, consumer_id, scope, relay_pin } =>
            close_interest(filter_json, consumer_id, scope, relay_pin, ctx),
    }
}
