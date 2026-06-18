use crate::relay::RelayRoleTestExt;
use crate::store::InsertOutcome;
use crate::time::{SystemTime, UNIX_EPOCH};

use super::super::{Kernel, StoredEvent};

impl Kernel {
    /// Ingest a pre-verified event through the kernel ingest path.
    ///
    /// This method does NOT call `ingest_timeline_event`.  Instead it:
    /// 1. Calls `store.insert` via `from_raw_unchecked` to let the store record
    ///    provenance (D4: store is the single authoritative writer; re-wrap avoids
    ///    redundant re-verification).
    /// 2. Populates the lightweight read-cache (`self.events` HashMap + appends to
    ///    `self.timeline`) directly, mirroring the `Inserted/Replaced` branch of
    ///    `ingest_timeline_event` but without signature re-verification overhead.
    ///
    /// Sort is deferred: callers injecting a batch MUST call
    /// `sort_timeline_deferred()` once after the loop to avoid O(n²·log n) cost.
    ///
    /// D0: capability boundary respected — this method is gated behind
    /// `cfg(any(test, feature = "test-support"))` and is never part of the
    /// production FFI surface.
    pub(crate) fn ingest_pre_verified_event(
        &mut self,
        role: crate::relay::RelayRole,
        sub_id: &str,
        verified: crate::store::VerifiedEvent,
    ) {
        let raw = verified.into_raw();
        let relay_url = role.url().to_string();
        let received_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Re-wrap as VerifiedEvent for the store; from_raw_unchecked is used
        // here because the caller has already verified (or intentionally
        // bypassed) verification. The store is the single authoritative writer.
        let verified_for_store = crate::store::VerifiedEvent::from_raw_unchecked(raw.clone());

        let proceed = match self
            .store
            .insert(verified_for_store, &relay_url, received_at_ms)
        {
            Ok(outcome) => matches!(
                outcome,
                InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
            ),
            Err(e) => {
                self.log(format!("test ingest store error: {e}"));
                !self.events.contains_key(&raw.id)
            }
        };

        if !proceed {
            return;
        }

        let id = raw.id.clone();
        let cached = StoredEvent {
            id: raw.id.clone(),
            author: raw.pubkey.clone(),
            kind: raw.kind,
            created_at: raw.created_at,
            tags: raw.tags.clone(),
            content: raw.content.clone(),
            relay_count: 1,
        };
        let kernel_event = crate::substrate::KernelEvent {
            id: cached.id.clone(),
            author: cached.author.clone(),
            kind: cached.kind,
            created_at: cached.created_at,
            tags: cached.tags.clone(),
            content: cached.content.clone(),
            relay_provenance: Vec::new(),
        };
        self.metric_stored_events = self.metric_stored_events.saturating_add(1);
        if cached.kind == 1 {
            self.metric_note_events = self.metric_note_events.saturating_add(1);
        }
        self.events.insert(id.clone(), cached);
        self.notify_event_observers(&kernel_event);
        // Step 2: raw observer tap removed from kernel; the dispatcher
        // handles external event sinks from the persistence chokepoint.
        {
            let verified_for_dispatch =
                crate::store::VerifiedEvent::from_raw_unchecked(raw.clone());
            if let Ok(d) = self.ingest_dispatcher_slot().read() {
                d.dispatch(&verified_for_dispatch);
            }
        }
        if sub_id.starts_with("diag-firehose-") {
            self.diagnostic_firehose.events = self.diagnostic_firehose.events.saturating_add(1);
            self.timeline.push_back(id);
        }
        self.events_since_last_update = self.events_since_last_update.saturating_add(1);
        self.changed_since_emit = true;
    }
}
