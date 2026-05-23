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
            .map_or(true, |existing| existing.created_at <= profile.created_at);
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
            self.ingest_contacts(event);
            self.changed_since_emit = true;
        }
    }
}
