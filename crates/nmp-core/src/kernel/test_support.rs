//! Test-support helpers for the kernel.
//!
//! All items in this file are gated on `cfg(any(test, feature = "test-support"))`.
//! They provide fast, signature-verification-free injection paths that let
//! unit tests and the firehose/FFI stress harnesses exercise the same ingest
//! hot-paths as production code without needing real secp256k1 keys.
//!
//! New test-support helpers should be added here rather than to `kernel/mod.rs`
//! to keep the main module under the 300-LOC soft limit (AGENTS.md).
//!
//! D7: capability boundary respected — none of these methods appear in the
//! production FFI surface.

use super::*;

impl Kernel {
    /// Deliver a replaceable event (kind:0, 3, or 10002) to the kernel,
    /// bypassing signature verification.
    ///
    /// Mirrors the production `handle_event` dispatch for replaceable kinds but
    /// uses `VerifiedEvent::from_raw_unchecked` so unit tests don't need real
    /// secp256k1 signatures.  Returns the `InsertOutcome` so callers can assert
    /// on supersession behaviour.
    ///
    /// Test-support only — gated on `cfg(any(test, feature = "test-support"))`.
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn inject_replaceable_event(
        &mut self,
        id: &str,
        pubkey: &str,
        created_at: u64,
        kind: u32,
        tags: Vec<Vec<String>>,
        relay_url: &str,
        received_at_ms: u64,
    ) -> Option<crate::store::InsertOutcome> {
        use crate::store::{InsertOutcome, RawEvent, VerifiedEvent};
        let raw = RawEvent {
            id: id.to_string(),
            pubkey: pubkey.to_string(),
            created_at,
            kind,
            tags: tags.clone(),
            content: String::new(),
            sig: "a".repeat(128),
        };
        let verified = VerifiedEvent::from_raw_unchecked(raw);
        let outcome = match self.store.insert(verified, &relay_url.to_string(), received_at_ms) {
            Ok(o) => o,
            Err(_) => return None,
        };
        if matches!(outcome, InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }) {
            let event = NostrEvent {
                id: id.to_string(),
                pubkey: pubkey.to_string(),
                created_at,
                kind,
                tags,
                content: String::new(),
                sig: "a".repeat(128),
            };
            match kind {
                0 => self.ingest_profile(event),
                3 => self.ingest_contacts(event),
                10002 => self.ingest_relay_list(event),
                _ => {}
            }
        }
        Some(outcome)
    }

    /// Ingest a pre-verified event through the real kernel ingest path.
    ///
    /// Calls `ingest_timeline_event` directly with a `VerifiedEvent` that has
    /// already been constructed by the caller (either via `try_from_raw` for
    /// the full-verify path, or via `from_raw_unchecked` for the perf-harness
    /// fast path).  This is the test-support substitute for the relay delivery
    /// path; it exercises the same hot path as `handle_event` without a live
    /// relay connection.
    ///
    /// D7: capability boundary respected — this method is gated behind
    /// `cfg(any(test, feature = "test-support"))` and is never part of the
    /// production FFI surface.
    pub(crate) fn ingest_pre_verified_event(
        &mut self,
        role: crate::relay::RelayRole,
        sub_id: &str,
        verified: crate::store::VerifiedEvent,
    ) {
        use crate::store::InsertOutcome;

        let raw = verified.into_raw();
        let relay_url = role.url().to_string();
        let received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Re-wrap as VerifiedEvent for the store; from_raw_unchecked is used
        // here because the caller has already verified (or intentionally
        // bypassed) verification.  The store is the single authoritative writer
        // per D4.
        let verified_for_store = crate::store::VerifiedEvent::from_raw_unchecked(raw.clone());

        let proceed = match self.store.insert(verified_for_store, &relay_url, received_at_ms) {
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
        self.events.insert(id.clone(), cached);
        // diag-firehose-stress sub_id: always appended to timeline.
        // sort_timeline() is NOT called here; callers that inject a batch of
        // events must call kernel.sort_timeline_deferred() once after the loop
        // to avoid O(n²·log n) sort overhead for large batches.
        if sub_id.starts_with("diag-firehose-") {
            self.diagnostic_firehose_events =
                self.diagnostic_firehose_events.saturating_add(1);
            self.timeline.push_back(id);
        }
        self.events_since_last_update =
            self.events_since_last_update.saturating_add(1);
        self.changed_since_emit = true;
    }

    /// Sort the timeline once after a batch inject (deferred sort).
    ///
    /// Call this after a loop of `ingest_pre_verified_event` calls to amortize
    /// the O(n log n) sort cost across the whole batch rather than paying it
    /// per-event.
    pub(crate) fn sort_timeline_deferred(&mut self) {
        self.sort_timeline();
    }
}
