//! `Kind3Parser` — the [`IngestParser`] that decodes kind:3 contact-list
//! events and upserts the parsed follow set into [`ContactsCache`].
//!
//! Structural sibling of `Kind0Parser` (kind:0), `nmp_router::Kind10002Parser`
//! (NIP-65 kind:10002), and `nmp_nip17::Kind10050Parser` (kind:10050). The
//! kernel's [`nmp_core::substrate::EventIngestDispatcher`] fans every accepted
//! `Inserted | Replaced` event to every registered parser; this parser filters
//! on `evt.raw().kind == 3` so an unintended dispatch is a silent no-op rather
//! than corrupting the contacts cache.
//!
//! # Parse contract — exact port of the kernel's former `ingest_contacts`
//!
//! The follow-set extraction is `nmp_core::tags::contact_follows` — the
//! SAME pure function the kernel's old `ingest_contacts` and the `nmp-nip02`
//! `ActiveFollowSet` / `FollowListProjection` observers call. So the cache's
//! follow set is byte-identical across all consumers: every valid-hex `p`-tag
//! pubkey, in document order (uncapped, #1497 amendment 6).
//!
//! # Side-effect-free against kernel state (the `IngestParser` contract)
//!
//! This parser writes ONLY the capability-owned cache. The kernel detects
//! contacts-cache transitions in `project_accepted_event` (before/after
//! snapshot of this cache for the author) and enqueues the generic feed-source
//! recompile trigger for the active account. The parser never names the active
//! account or the lifecycle registry — it cannot, structurally (no `&mut Kernel`).
//!
//! Supersession (newest kind:3 wins, lexicographic event-id tiebreak) is owned
//! by [`ContactsCache::upsert_view`].

use std::sync::Arc;

use nmp_core::substrate::{ContactsView, IngestParser};
use nmp_store::VerifiedEvent;

use crate::contacts_cache::ContactsCache;

/// NIP-02 — the kind number for contact-list events.
const KIND_CONTACT_LIST: u32 = 3;

/// The kind:3 ingest parser. Constructed with a shared `Arc<ContactsCache>`
/// handle — the same `Arc` the kernel holds as its `Arc<dyn ContactsLookup>`,
/// so the writer side (this parser) and the reader side (the kernel's
/// feed-source recompile / byte-estimate / RAM-eviction paths) see one
/// source of truth.
pub struct Kind3Parser {
    cache: Arc<ContactsCache>,
}

impl Kind3Parser {
    /// Construct a parser writing into the supplied [`ContactsCache`].
    #[must_use]
    pub fn new(cache: Arc<ContactsCache>) -> Self {
        Self { cache }
    }

    /// Static-dispatch path for tests and direct callers. Returns `false`
    /// (no-op) when `evt`'s kind is not 3; otherwise parses + upserts and
    /// returns whether the candidate superseded the cached entry (the change
    /// signal — newest kind:3 wins).
    pub fn parse_event(&self, evt: &VerifiedEvent) -> bool {
        let raw = evt.raw();
        if raw.kind != KIND_CONTACT_LIST {
            return false;
        }
        let follows = nmp_core::tags::contact_follows(&raw.tags);
        self.cache.upsert_view(
            raw.pubkey.clone(),
            ContactsView {
                event_id: raw.id.clone(),
                created_at: raw.created_at,
                follows,
            },
        )
    }
}

impl IngestParser for Kind3Parser {
    fn parse(&self, evt: &VerifiedEvent) {
        let _ = self.parse_event(evt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::{ContactsLookup, EventIngestDispatcher};
    use nmp_store::RawEvent;

    fn evt(
        pubkey: &str,
        id: &str,
        kind: u32,
        created_at: u64,
        tags: Vec<Vec<String>>,
    ) -> VerifiedEvent {
        VerifiedEvent::from_raw_unchecked(RawEvent {
            id: id.into(),
            pubkey: pubkey.into(),
            created_at,
            kind,
            tags,
            content: String::new(),
            sig: "22".repeat(64),
        })
    }

    fn p(pk: &str) -> Vec<String> {
        vec!["p".to_string(), pk.to_string()]
    }

    #[test]
    fn ignores_non_kind_3() {
        let cache = Arc::new(ContactsCache::new());
        let parser = Kind3Parser::new(Arc::clone(&cache));
        assert!(!parser.parse_event(&evt("alice", "aa", 1, 100, vec![p(&"1".repeat(64))])));
        assert!(cache.is_empty());
    }

    #[test]
    fn extracts_capped_hex_p_tags_in_order() {
        let cache = Arc::new(ContactsCache::new());
        let parser = Kind3Parser::new(Arc::clone(&cache));
        let a = "1".repeat(64);
        let b = "2".repeat(64);
        let tags = vec![p(&a), p(&b), p("not-hex"), vec!["e".to_string(), a.clone()]];
        assert!(parser.parse_event(&evt("alice", "aa", 3, 100, tags)));
        assert_eq!(cache.follows("alice"), Some(vec![a, b]));
    }

    #[test]
    fn empty_kind3_stores_cleared_follow_set() {
        let cache = Arc::new(ContactsCache::new());
        let parser = Kind3Parser::new(Arc::clone(&cache));
        assert!(parser.parse_event(&evt("alice", "aa", 3, 100, Vec::new())));
        assert_eq!(cache.follows("alice"), Some(Vec::new()));
    }

    #[test]
    fn newer_kind3_supersedes_via_dispatcher() {
        let cache = Arc::new(ContactsCache::new());
        let parser: Arc<dyn IngestParser> = Arc::new(Kind3Parser::new(Arc::clone(&cache)));
        let mut d = EventIngestDispatcher::new();
        d.register_kind(3, parser);

        let a = "1".repeat(64);
        d.dispatch(&evt("alice", "old", 3, 100, vec![p(&a)]));
        // Newer cleared list wins.
        d.dispatch(&evt("alice", "new", 3, 200, Vec::new()));

        assert_eq!(cache.follows("alice"), Some(Vec::new()));
    }
}
