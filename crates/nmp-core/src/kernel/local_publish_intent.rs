//! Local projection of signed replaceable events accepted for publish.

use crate::store::InsertOutcome;
use crate::substrate::SignedEvent;

use super::Kernel;

impl Kernel {
    pub(super) fn record_local_publish_intent(&mut self, signed: &SignedEvent) {
        self.record_local_profile_intent(signed);
        self.record_local_contacts_intent(signed);
    }

    fn record_local_profile_intent(&mut self, signed: &SignedEvent) {
        let Some(profile) = super::nostr::parse_profile_intent(signed) else {
            return;
        };
        let should_replace = self
            .local_profile_intents
            .get(&signed.unsigned.pubkey)
            .is_none_or(|existing| existing.created_at <= profile.created_at);
        if should_replace {
            self.local_profile_intents
                .insert(signed.unsigned.pubkey.clone(), profile);
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
