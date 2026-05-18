//! Kernel-side publish dispatch (T66a).
//!
//! D3: relay targets are resolved by `Nip65OutboxResolver` reading the
//! active account's kind:10002 from the shared store — never a hard-coded
//! constant. An empty resolution for a publish surfaces a `last_error_toast`
//! (D6: errors are state, never exceptions across FFI) and the event is
//! still recorded in the publish queue as `pending_relays_unknown` so the UI
//! can prompt the user to declare write-relays.
//!
//! D1: the queue entry is appended the moment the EVENT frame is emitted and
//! marked `accepted_locally`; full per-relay OK correlation is a follow-up
//! (refine in place). The socket fan-out is the kernel's existing
//! `RelayRole::Content` write path — true NIP-65 multi-relay fan-out needs a
//! relay-manager change tracked beyond T66a.

use super::*;
use crate::publish::{Nip65OutboxResolver, OutboxResolver, PublishTarget};
use crate::substrate::SignedEvent;

impl Kernel {
    /// Resolve outbox relays for `author_hex` + `p_tags` via NIP-65 (D3),
    /// emit the signed event as a wire `EVENT` frame on the write path, and
    /// record it in the publish queue. Returns the outbound frames (empty if
    /// no write-relays are declared — caller already set the toast).
    pub(crate) fn publish_signed(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
    ) -> Vec<OutboundMessage> {
        let resolver = Nip65OutboxResolver::new(
            Arc::clone(&self.store),
            crate::publish::DEFAULT_INDEXER_FALLBACK
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        );
        // D3: Auto target → resolver decides. We pass the author so the
        // resolver can read the active account's own kind:10002 write set.
        let relays = resolver.resolve(&signed.unsigned.pubkey, p_tags, &PublishTarget::Auto);

        let wire = json!([
            "EVENT",
            {
                "id": signed.id,
                "pubkey": signed.unsigned.pubkey,
                "kind": signed.unsigned.kind,
                "tags": signed.unsigned.tags,
                "content": signed.unsigned.content,
                "created_at": signed.unsigned.created_at,
                "sig": signed.sig,
            }
        ])
        .to_string();

        if relays.is_empty() {
            // Resolver returned nothing AND no indexer fallback applied — the
            // only way `resolve` is empty for Auto is an explicit-empty path,
            // which cannot happen here. Defensive: treat as "no targets".
            self.push_publish_entry(PublishQueueEntry {
                event_id: signed.id.clone(),
                kind: signed.unsigned.kind,
                target_relays: 0,
                status: "pending_relays_unknown".to_string(),
            });
            self.set_last_error_toast(Some(
                "active account has no write-relays declared — add a relay in \
                 Accounts → Relays and publish a fresh kind:10002"
                    .to_string(),
            ));
            return Vec::new();
        }

        self.log(format!(
            "PUBLISH kind:{} id={} → {} outbox relay(s)",
            signed.unsigned.kind,
            &signed.id[..signed.id.len().min(12)],
            relays.len()
        ));
        self.push_publish_entry(PublishQueueEntry {
            event_id: signed.id.clone(),
            kind: signed.unsigned.kind,
            target_relays: relays.len(),
            status: "accepted_locally".to_string(),
        });
        self.set_last_error_toast(None);
        self.changed_since_emit = true;

        // T105 (subsumes T99): NIP-65 multi-relay write fan-out — one
        // PublishAction → N per-relay EVENT frames addressed to the author's
        // resolved write relays + recipients' read relays. Each frame carries
        // its own `relay_url`; the transport dials the right socket per URL.
        // The diagnostic lane is `Content` (the write/publish lane).
        relays
            .into_iter()
            .map(|relay_url| OutboundMessage {
                role: RelayRole::Content,
                relay_url,
                text: wire.clone(),
            })
            .collect()
    }

    /// Latest kind:3 follow set for `author_hex` (hex pubkeys from `p` tags),
    /// read from the shared store. Empty if no kind:3 is known yet.
    pub(crate) fn current_follows(&self, author_hex: &str) -> Vec<String> {
        let Some(author) = crate::kernel::hex_to_pubkey_bytes(author_hex) else {
            return Vec::new();
        };
        let Ok(mut iter) = self
            .store
            .scan_by_author_kind(&author, &[3], None, None, 1)
        else {
            return Vec::new();
        };
        let stored = match iter.next() {
            Some(Ok(stored)) => stored,
            _ => return Vec::new(),
        };
        stored
            .raw
            .tags
            .iter()
            .filter(|t: &&Vec<String>| t.first().map(String::as_str) == Some("p"))
            .filter_map(|t| t.get(1).cloned())
            .filter(|pk| is_hex_pubkey(pk))
            .collect()
    }
}
