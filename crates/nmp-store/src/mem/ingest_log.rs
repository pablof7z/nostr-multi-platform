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

/// Trim the log to at most `DEFAULT_LOG_MAX_ENTRIES`, advancing the GC floor.
///
/// Called after every `log_append` so the log is never unbounded (ADR-0058 R2.4).
pub(super) fn log_gc_trim(st: &mut MemState) {
    while st.ingest_log.len() as u64 > DEFAULT_LOG_MAX_ENTRIES {
        if let Some((seq, _)) = st.ingest_log.pop_first() {
            st.log_gc_floor = seq;
        } else {
            break;
        }
    }
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
