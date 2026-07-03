//! Narrow, protocol-neutral cached-event reads for [`ProtocolCommand`](crate::substrate::ProtocolCommand)s.
//!
//! #2917 (epic #2864 W8) — the nutzap send flow needs to read ANOTHER
//! account's kind:10019 (mints/relays/Cashu P2PK pubkey) before it can build
//! a kind:9321. `nmp-core` must not learn Cashu/nutzap/mint nouns (D0), so
//! this is deliberately the same shape as [`super::diagnostic_counters`]'s
//! `lnurl_for_pubkey` (the `ZapProfileLookup` precedent): a bare read against
//! whatever the store already has cached, named only in terms of protocol-
//! neutral primitives (event id, author, kind) — never "wallet" or "nutzap".
//!
//! This does **not** proactively fetch anything. A pubkey the kernel has no
//! interest in and has never seen an event from returns `None` here exactly
//! as it did before this file existed; the caller (`nmp-wallet`) is
//! responsible for deciding what to do about that (fail closed, or
//! `ctx.ensure_interest(..)` to warm the cache for next time — see
//! `crate::substrate::CachedEventLookup`'s doc comment).

use super::kernel_misc::hex_to_pubkey_bytes;
use super::Kernel;
use crate::store::StoredEvent;
use crate::substrate::KernelEvent;

impl Kernel {
    /// The cached event for `id` (64-char lowercase hex), or `None` if absent
    /// or the store has never seen it. Malformed hex is also `None` (D6).
    pub(crate) fn cached_event_by_id(&self, id: &str) -> Option<KernelEvent> {
        let bytes = hex_to_pubkey_bytes(id)?;
        let stored = self.store.get_by_id(&bytes).ok()??;
        Some(kernel_event_from_stored(&stored))
    }

    /// The newest cached event authored by `author` (64-char lowercase hex)
    /// with kind `kind`, or `None` if the store holds none. Point-in-time
    /// cache read only — does not open a subscription or block on a fetch.
    pub(crate) fn cached_latest_author_kind(&self, author: &str, kind: u32) -> Option<KernelEvent> {
        let author_bytes = hex_to_pubkey_bytes(author)?;
        let mut iter = self
            .store
            .scan_by_author_kind(&author_bytes, &[kind], None, None, 1)
            .ok()?;
        let stored = iter.next()?.ok()?;
        Some(kernel_event_from_stored(&stored))
    }
}

fn kernel_event_from_stored(stored: &StoredEvent) -> KernelEvent {
    KernelEvent {
        id: stored.raw.id.clone(),
        author: stored.raw.pubkey.clone(),
        kind: stored.raw.kind,
        created_at: stored.raw.created_at,
        tags: stored.raw.tags.clone(),
        content: stored.raw.content.clone(),
        relay_provenance: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::kernel::{Kernel, NostrEvent};
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use nmp_network::role::RelayRole;

    /// Sign a minimal event and convert it into the kernel's own hex-string
    /// `NostrEvent` shape — mirrors `gc_step_tests::signed_expiring_note`'s
    /// conversion pattern.
    fn signed_event(keys: &::nostr::Keys, kind: u16, content: &str) -> NostrEvent {
        use ::nostr::EventBuilder;
        let nostr_event = EventBuilder::new(::nostr::Kind::from(kind), content)
            .sign_with_keys(keys)
            .expect("sign_with_keys cannot fail with a generated keypair");
        NostrEvent {
            id: nostr_event.id.to_hex(),
            pubkey: nostr_event.pubkey.to_hex(),
            created_at: nostr_event.created_at.as_secs(),
            kind: nostr_event.kind.as_u16() as u32,
            tags: nostr_event
                .tags
                .iter()
                .map(|t: &::nostr::Tag| t.as_slice().to_vec())
                .collect(),
            content: nostr_event.content.clone(),
            sig: nostr_event.sig.to_string(),
        }
    }

    #[test]
    fn cached_event_by_id_reads_an_ingested_event() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let keys = ::nostr::Keys::generate();
        let event = signed_event(&keys, 10019, "");
        let id = event.id.clone();
        kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.example", "sub", event);

        let found = kernel.cached_event_by_id(&id).expect("event must be cached");
        assert_eq!(found.id, id);
        assert_eq!(found.kind, 10019);
    }

    #[test]
    fn cached_event_by_id_returns_none_for_unknown_id() {
        let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let unknown = "a".repeat(64);
        assert!(kernel.cached_event_by_id(&unknown).is_none());
    }

    #[test]
    fn cached_latest_author_kind_finds_the_newest_match() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let keys = ::nostr::Keys::generate();
        let author = keys.public_key().to_hex();
        let event = signed_event(&keys, 10019, "");
        let expected_id = event.id.clone();
        kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.example", "sub", event);

        let found = kernel
            .cached_latest_author_kind(&author, 10019)
            .expect("author/kind must resolve");
        assert_eq!(found.id, expected_id);
        assert_eq!(found.author, author);
    }

    #[test]
    fn cached_latest_author_kind_returns_none_when_nothing_cached() {
        let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let unknown = "b".repeat(64);
        assert!(kernel.cached_latest_author_kind(&unknown, 10019).is_none());
    }
}
