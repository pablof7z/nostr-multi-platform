//! Interest, pull-cursor, and test-support dispatch arms.
//!
//! Covers: `EnsureInterest`, `ReplaceDependentInterestSet`, `DropInterestOwner`,
//! `OpenPullCursor`, `AdvancePullCursor`, `UnregisterPullCursor`,
//! `OpenInterest`, `OpenObservedInterest`, `CloseInterest`, and the
//! `#[cfg(test)]` ingest/GC arms.
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
use super::InterestsPorts;

/// Dispatch `InterestsCommand::EnsureInterest`.
pub(super) fn ensure_interest(
    identity: crate::subs::SubIdentity,
    interest: crate::planner::LogicalInterest,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    // Delegates to the shared Kernel::ensure_interest helper (#2045 PR-A)
    // so the headless `apply_actor_command` interpreter uses the same path.
    ports.kernel.ensure_interest(identity, interest);
    Some(Vec::new())
}

/// Dispatch `InterestsCommand::ReplaceDependentInterestSet`.
pub(super) fn replace_dependent_interest_set(
    owner: crate::subs::SubOwnerKey,
    children: Vec<crate::kernel::DependentInterestChild>,
    reason: String,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    ports.kernel.replace_dependent_interest_set(owner, children, &reason);
    maybe_emit_after_dispatch(ports.kernel, ports.running, ports.update_tx, ports.last_emit);
    Some(Vec::new())
}

/// Dispatch `InterestsCommand::DropInterestOwner`.
pub(super) fn drop_interest_owner(
    identity: crate::subs::SubIdentity,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    // Delegates to the shared Kernel::drop_interest_owner helper (#2045 PR-A).
    ports.kernel.drop_interest_owner(identity);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::OpenPullCursor`.
pub(super) fn open_pull_cursor(
    handle: crate::kernel::pull_cursor::PullCursorHandle,
    spec: crate::kernel::pull_cursor::PullCursorSpec,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    ports.kernel.open_pull_cursor(handle, spec);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::AdvancePullCursor`.
pub(super) fn advance_pull_cursor(
    cursor_id: crate::kernel::pull_cursor::PullCursorId,
    after_seq: u64,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    ports.kernel.advance_pull_cursor(cursor_id, after_seq);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::UnregisterPullCursor`.
pub(super) fn unregister_pull_cursor(
    cursor_id: crate::kernel::pull_cursor::PullCursorId,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    ports.kernel.unregister_pull_cursor(cursor_id);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::OpenInterest`.
pub(super) fn open_interest(
    filter_json: String,
    consumer_id: String,
    scope: u32,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // M2 (ADR-0042) — generic feed-subscription front door. Parse the
    // verbatim NIP-01 filter into an InterestShape, derive the
    // `(owner, key, scope)` identity from it, and run the same
    // ensure_sub + CompileTrigger body as the `EnsureInterest` arm.
    // D6: a malformed filter is a silent no-op (the FFI shim already
    // surfaced a toast before sending — see `nmp_app_open_interest`).
    if let Some((identity, interest)) = build_open_interest(&filter_json, &consumer_id, scope, None)
    {
        let _ = ports.kernel.open_interest_sub(identity, interest);
    }
    maybe_emit_after_dispatch(ports.kernel, ports.running, ports.update_tx, ports.last_emit);
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
    ports: &mut InterestsPorts<'_>,
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
        let _ = ports.kernel.open_interest_with_observer_replay(
            identity,
            interest,
            replay,
            "open-observed-interest",
        );
    }
    maybe_emit_after_dispatch(ports.kernel, ports.running, ports.update_tx, ports.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::CloseInterest`.
pub(super) fn close_interest(
    filter_json: String,
    consumer_id: String,
    scope: u32,
    relay_pin: Option<String>,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // M2 (ADR-0042) — detach one owner; drop the live sub on the last
    // leave. The `(owner, key, scope)` identity is reconstructed from
    // the SAME filter + consumer + scope + relay_pin the open used, so
    // the InterestShape hash lands on the same registry slot.
    if let Some((identity, _interest)) =
        build_open_interest(&filter_json, &consumer_id, scope, relay_pin.as_deref())
    {
        let _ = ports.kernel.close_interest_sub(&identity);
    }
    maybe_emit_after_dispatch(ports.kernel, ports.running, ports.update_tx, ports.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::IngestPreVerifiedEvents` (test-support only).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn ingest_pre_verified_events(
    events: Vec<crate::store::VerifiedEvent>,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    // D4: actor thread is the sole mutator. sort_timeline() deferred to
    // after the loop to avoid O(n²·log n) for large batches.
    for verified in events {
        ports.kernel.ingest_pre_verified_event(
            nmp_network::role::RelayRole::Content,
            "diag-firehose-stress",
            verified,
        );
    }
    ports.kernel.sort_timeline_deferred();
    maybe_emit_after_dispatch(ports.kernel, ports.running, ports.update_tx, ports.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::IngestPreVerifiedEventsForRelay` (test-support only).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn ingest_pre_verified_events_for_relay(
    relay_url: String,
    events: Vec<crate::store::VerifiedEvent>,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    for verified in events {
        ports.kernel.ingest_pre_verified_event_from_relay(
            &relay_url,
            "diag-firehose-stress",
            verified,
        );
    }
    ports.kernel.sort_timeline_deferred();
    maybe_emit_after_dispatch(ports.kernel, ports.running, ports.update_tx, ports.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::IngestPreVerifiedEventsForSubId` (test-support only).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn ingest_pre_verified_events_for_sub_id(
    sub_id: String,
    events: Vec<crate::store::VerifiedEvent>,
    ack: std::sync::mpsc::SyncSender<()>,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::maybe_emit_after_dispatch;
    for verified in events {
        ports.kernel
            .ingest_pre_verified_event(nmp_network::role::RelayRole::Content, &sub_id, verified);
    }
    ports.kernel.sort_timeline_deferred();
    maybe_emit_after_dispatch(ports.kernel, ports.running, ports.update_tx, ports.last_emit);
    let _ = ack.send(());
    Some(Vec::new())
}

/// Dispatch `ActorCommand::TriggerGcStep` (test-support only).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn trigger_gc_step(
    ack: std::sync::mpsc::SyncSender<()>,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    ports.kernel.run_gc_step();
    let _ = ack.send(());
    Some(Vec::new())
}

/// ADR-0065 — `InterestsCommand` family dispatch. Matches the sub-enum and
/// routes each verb to its existing handler.
pub(super) fn dispatch(
    cmd: InterestsCommand,
    ports: &mut InterestsPorts<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        InterestsCommand::EnsureInterest { identity, interest } => {
            ensure_interest(identity, interest, ports)
        }
        InterestsCommand::ReplaceDependentInterestSet {
            owner,
            children,
            reason,
        } => replace_dependent_interest_set(owner, children, reason, ports),
        InterestsCommand::DropInterestOwner(identity) => drop_interest_owner(identity, ports),
        InterestsCommand::OpenPullCursor { handle, spec } => open_pull_cursor(handle, spec, ports),
        InterestsCommand::AdvancePullCursor {
            cursor_id,
            after_seq,
        } => advance_pull_cursor(cursor_id, after_seq, ports),
        InterestsCommand::UnregisterPullCursor { cursor_id } => {
            unregister_pull_cursor(cursor_id, ports)
        }
        InterestsCommand::OpenInterest {
            filter_json,
            consumer_id,
            scope,
        } => open_interest(filter_json, consumer_id, scope, ports),
        InterestsCommand::OpenObservedInterest {
            filter_json,
            consumer_id,
            scope,
            relay_pin,
            observer_id,
            replay_shapes,
            replay_limit,
        } => open_observed_interest(
            filter_json,
            consumer_id,
            scope,
            relay_pin,
            observer_id,
            replay_shapes,
            replay_limit,
            ports,
        ),
        InterestsCommand::CloseInterest {
            filter_json,
            consumer_id,
            scope,
            relay_pin,
        } => close_interest(filter_json, consumer_id, scope, relay_pin, ports),
    }
}
