//! Local projection of signed replaceable events accepted for publish.

use crate::store::InsertOutcome;
use crate::substrate::SignedEvent;

use super::Kernel;

impl Kernel {
    pub(super) fn record_local_publish_intent(&mut self, signed: &SignedEvent) {
        self.record_local_profile_intent(signed);
        self.record_local_contacts_intent(signed);
        self.record_local_replaceable_intent(signed);
    }

    fn record_local_profile_intent(&mut self, signed: &SignedEvent) {
        if signed.unsigned.kind != 0 {
            return;
        }
        let event = super::nostr::signed_event_to_nostr(signed);
        let outcome = self.verify_and_persist("local://publish", &event);
        if matches!(
            outcome,
            Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
        ) {
            // Single-mechanism (ADR-0045 Rev 2): route the locally-published
            // kind:0 through the EXACT sequence the relay ingest arm uses
            // (`ingest/mod.rs` kind:0) — `ingest_profile` then the observer
            // fan-out — so profile-driven projections / observers reflect the
            // local edit immediately, without waiting for the relay echo (which
            // dedups to `Duplicate` and never re-fires fan-out). `kernel_event`
            // is built before the `ingest_profile(event)` move, via the same
            // single construction site the relay arm uses, so the local event
            // and its later relay echo carry byte-identical observer payloads.
            // D4: the fan-out is gated on the `Inserted | Replaced` outcome
            // above — the duplicate relay echo does not re-fire it. Retires the
            // `local_profile_intents` overlay (closes #1193).
            let kernel_event = super::ingest::helpers::kernel_event_from_nostr(&event);
            self.ingest_profile(event);
            self.notify_event_observers(&kernel_event);
            self.changed_since_emit = true;
        }
    }

    fn record_local_replaceable_intent(&mut self, signed: &SignedEvent) {
        // kind:0 and kind:3 are handled by their own arms above; this covers
        // every OTHER locally-published replaceable (kind:10002 relay lists,
        // kind:10050 DM-relay lists, parameterized lists, …) the same way the
        // relay wildcard ingest arm does: `verify_and_persist` (which fans
        // through the `EventIngestDispatcher` / per-NIP parsers) followed by
        // the observer fan-out, so routing/mailbox-driven projections react to
        // the local edit immediately rather than waiting for the relay echo.
        if signed.unsigned.kind == 0 || signed.unsigned.kind == 3 {
            return;
        }
        // Only replaceables take the local-store path; non-replaceable kinds
        // (kind:1 notes, kind:6 reposts, …) are timeline events published
        // through other seams and must not be force-persisted here.
        use crate::kinds::{is_parameterized_replaceable, is_replaceable};
        if !is_replaceable(signed.unsigned.kind)
            && !is_parameterized_replaceable(signed.unsigned.kind)
        {
            return;
        }
        let event = super::nostr::signed_event_to_nostr(signed);
        let outcome = self.verify_and_persist("local://publish", &event);
        if matches!(
            outcome,
            Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
        ) {
            // D4: gated on `Inserted | Replaced`; the duplicate relay echo
            // never re-fires the observer.
            let kernel_event = super::ingest::helpers::kernel_event_from_nostr(&event);
            self.notify_event_observers(&kernel_event);
            self.changed_since_emit = true;
        }
    }

    fn record_local_contacts_intent(&mut self, signed: &SignedEvent) {
        if signed.unsigned.kind != 3 {
            return;
        }
        let event = super::nostr::signed_event_to_nostr(signed);
        let outcome = self.verify_and_persist("local://publish", &event);
        if matches!(
            outcome,
            Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
        ) {
            // Read-your-writes (FINDING A): route the locally-published kind:3
            // through the EXACT sequence the relay ingest arm uses
            // (`ingest/mod.rs` kind:3) — `ingest_contacts` then the observer
            // fan-out — so `FollowListProjection` / `ActiveFollowSet` reflect
            // the follow/unfollow immediately, without waiting for the relay
            // echo (which dedups to `Duplicate` and never re-fires fan-out) or
            // an account switch / restart. `kernel_event` is built before the
            // `ingest_contacts(event)` move, via the same single construction
            // site the relay arm uses, so the local event and its later relay
            // echo carry byte-identical observer payloads. D4: the fan-out is
            // gated on the `Inserted | Replaced` outcome above — the duplicate
            // relay echo does not re-fire it.
            let kernel_event = super::ingest::helpers::kernel_event_from_nostr(&event);
            self.ingest_contacts(event);
            self.notify_event_observers(&kernel_event);
            self.changed_since_emit = true;
        }
    }
}
