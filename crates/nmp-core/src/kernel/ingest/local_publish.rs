//! Local replaceable-event cache updates for freshly signed publishes.

use super::super::*;
use super::NostrEvent;

impl Kernel {
    /// Persist a locally signed replaceable event and refresh the same caches
    /// an inbound relay echo would update.
    ///
    /// Account creation signs kind:0 / kind:3 / kind:10002 before any relay can
    /// echo them back. Remembering those canonical events locally lets the UI
    /// show the new profile immediately and lets subsequent publishes resolve
    /// the account's just-declared NIP-65 write relays.
    pub(crate) fn remember_local_replaceable_publish(
        &mut self,
        signed: &crate::substrate::SignedEvent,
        relay_url: &str,
    ) {
        let event = NostrEvent {
            id: signed.id.clone(),
            pubkey: signed.unsigned.pubkey.clone(),
            created_at: signed.unsigned.created_at,
            kind: signed.unsigned.kind,
            tags: signed.unsigned.tags.clone(),
            content: signed.unsigned.content.clone(),
            sig: signed.sig.clone(),
        };
        let Some(outcome) = self.verify_and_persist(relay_url, &event) else {
            return;
        };
        if !matches!(
            outcome,
            crate::store::InsertOutcome::Inserted { .. }
                | crate::store::InsertOutcome::Replaced { .. }
        ) {
            return;
        }
        match event.kind {
            0 => self.ingest_profile(event),
            3 => self.ingest_contacts(event),
            10002 => self.ingest_relay_list(event),
            _ => {}
        }
        self.changed_since_emit = true;
    }
}
