//! Ingest-log helpers for `MemEventStore`.
//!
//! All mutations hold the `MemState` lock before calling these functions
//! (D4: single writer via mutex). `log_append` allocates the next seq, writes
//! the entry, then immediately trims to `DEFAULT_LOG_MAX_ENTRIES`.

use crate::ingest_log::{DeleteReason, LogOp, StoreLogEntry, DEFAULT_LOG_MAX_ENTRIES};
use crate::types::{EventId, RawEvent, RelayUrl};

use super::MemState;

/// Append one entry to the ingest log and return the allocated seq.
///
/// D4: caller MUST hold the `MemState` lock (i.e. pass `&mut MemState`).
/// Seq is allocated by incrementing `ingest_seq` within the lock.
pub(super) fn log_append(
    st: &mut MemState,
    op: LogOp,
    event_id: EventId,
    raw_event: Option<RawEvent>,
    source_relay: Option<RelayUrl>,
    received_at_ms: u64,
) -> u64 {
    st.ingest_seq += 1;
    let seq = st.ingest_seq;
    st.ingest_log.insert(
        seq,
        StoreLogEntry {
            seq,
            op,
            event_id,
            raw_event,
            source_relay,
            received_at_ms,
        },
    );
    log_gc_trim(st);
    seq
}

/// Trim the log, advancing the GC floor (ADR-0072 §6, step-4).
///
/// Called after every `log_append` so the log is never unbounded (ADR-0072
/// R2.4). The floor target is the normal retention floor
/// (`latest_seq - DEFAULT_LOG_MAX_ENTRIES`), capped by the slowest still-eligible
/// `Protected`-cursor claim so its unconsumed rows survive.
///
/// CRITICAL (step-4): eligibility is computed HERE against the current
/// `latest_seq` (`st.ingest_seq`) — NOT from a precomputed bare floor. Once a
/// protected cursor's lag exceeds its `max_lag_entries`, its claim is filtered
/// out at this trim, the floor advances normally, and that cursor later gets an
/// explicit `PullGap` (D5: a stuck consumer cannot pin the log unbounded).
pub(super) fn log_gc_trim(st: &mut MemState) {
    let latest_seq = st.ingest_seq;
    let normal_floor = latest_seq.saturating_sub(DEFAULT_LOG_MAX_ENTRIES);

    // Slowest still-eligible protected cursor (min after_seq among claims whose
    // lag is still within bound). Eligibility uses the CURRENT latest_seq.
    let protected_floor = st
        .retention_claims
        .iter()
        .filter(|c| latest_seq.saturating_sub(c.after_seq) <= c.max_lag_entries)
        .map(|c| c.after_seq)
        .min();

    let target_floor = match protected_floor {
        Some(p) => normal_floor.min(p),
        None => normal_floor,
    };
    let new_floor = st.log_gc_floor.max(target_floor);

    if new_floor <= st.log_gc_floor {
        return;
    }
    // Delete log rows with `current_gc_floor < seq <= new_floor`.
    let to_remove: Vec<u64> = st
        .ingest_log
        .range((
            std::ops::Bound::Excluded(st.log_gc_floor),
            std::ops::Bound::Included(new_floor),
        ))
        .map(|(seq, _)| *seq)
        .collect();
    for seq in to_remove {
        st.ingest_log.remove(&seq);
    }
    st.log_gc_floor = new_floor;
}

/// Emit an `Inserted` log entry (for a new distinct event).
pub(super) fn emit_inserted(
    st: &mut MemState,
    event_id: EventId,
    raw_event: RawEvent,
    source_relay: &RelayUrl,
    received_at_ms: u64,
) {
    log_append(
        st,
        LogOp::Inserted,
        event_id,
        Some(raw_event),
        Some(source_relay.clone()),
        received_at_ms,
    );
}

/// Emit a `Replaced` log entry (replaceable supersession).
pub(super) fn emit_replaced(
    st: &mut MemState,
    new_event_id: EventId,
    replaced_id: EventId,
    raw_event: RawEvent,
    source_relay: &RelayUrl,
    received_at_ms: u64,
) {
    log_append(
        st,
        LogOp::Replaced { replaced_id },
        new_event_id,
        Some(raw_event),
        Some(source_relay.clone()),
        received_at_ms,
    );
}

/// Emit a `Deleted` log entry (semantic removal).
pub(super) fn emit_deleted(
    st: &mut MemState,
    carrier_event_id: EventId,
    target_id: EventId,
    reason: DeleteReason,
    received_at_ms: u64,
) {
    log_append(
        st,
        LogOp::Deleted { target_id, reason },
        carrier_event_id,
        None,
        None,
        received_at_ms,
    );
}
