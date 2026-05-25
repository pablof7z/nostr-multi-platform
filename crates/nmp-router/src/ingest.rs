//! `Kind10002Parser` — the [`IngestParser`] that decodes kind:10002 events
//! and upserts the resolved [`ParsedRelayList`] into [`InMemoryMailboxCache`].
//!
//! NIP-65 tag shape:
//! ```text
//!   ["r", "<url>"]            → read + write (bidirectional / both)
//!   ["r", "<url>", "read"]    → read only
//!   ["r", "<url>", "write"]   → write only
//! ```
//! Unknown markers are ignored. Empty URLs are ignored. Only `wss://` URLs
//! are accepted (the same defensive scheme gate the legacy kernel-side
//! `parse_relay_list` applied — `ws://` / `https://` URLs in an `r` tag
//! are misconfiguration and routing must not consume them). Duplicates
//! within a single event are deduped lane-wise (an event with two
//! `["r","wss://x"]` tags upserts a single entry).
//!
//! ## Empty-list semantics
//!
//! A canonical kind:10002 carrying zero parseable `r` tags is the author's
//! "I cleared my NIP-65 metadata" signal. The parser removes the entry from
//! the cache rather than upserting an empty list, so subsequent
//! [`MailboxCache::known`] lookups fail closed exactly as for an author who
//! never published a kind:10002. The kernel's mailbox-change observer (in
//! `kernel/ingest/mod.rs`) sees the cache transition Some→None and fires
//! a recompile trigger so the M2 planner re-routes the author off the
//! now-stale relays.

use std::sync::Arc;

use nmp_core::store::VerifiedEvent;
use nmp_core::substrate::{IngestParser, MailboxCache, ParsedRelayList};

use crate::InMemoryMailboxCache;

/// The kind:10002 ingest parser. Constructed with a shared
/// [`InMemoryMailboxCache`] handle so multiple registrations (test code,
/// the planner, future consumers) read the same cache.
pub struct Kind10002Parser {
    cache: Arc<InMemoryMailboxCache>,
}

impl Kind10002Parser {
    #[must_use]
    pub fn new(cache: Arc<InMemoryMailboxCache>) -> Self {
        Self { cache }
    }

    /// Static-dispatch path for tests and direct callers. Identical effect
    /// to [`IngestParser::parse`].
    ///
    /// Empty-list (the author cleared their NIP-65) removes the cache entry
    /// rather than upserting an empty `ParsedRelayList`. The kernel's
    /// mailbox-change observer (see `kernel/ingest/mod.rs`) detects the
    /// transition and fires the recompile trigger; the parser itself does
    /// not name the kernel-side lifecycle.
    pub fn parse_event(&self, evt: &VerifiedEvent) {
        let raw = evt.raw();
        if raw.kind != 10_002 {
            return;
        }
        let parsed = parse_relay_list(&raw.tags);
        if parsed.read.is_empty() && parsed.write.is_empty() && parsed.both.is_empty() {
            self.cache.remove(&raw.pubkey);
        } else {
            self.cache.upsert(raw.pubkey.clone(), parsed);
        }
    }
}

impl IngestParser for Kind10002Parser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.parse_event(evt);
    }
}

fn parse_relay_list(tags: &[Vec<String>]) -> ParsedRelayList {
    let mut read = Vec::new();
    let mut write = Vec::new();
    let mut both = Vec::new();

    for tag in tags {
        // Spec: `["r", "<url>", "<marker?>"]`. Skip anything else.
        if tag.first().map(String::as_str) != Some("r") {
            continue;
        }
        // Defensive scheme gate — only `wss://` URLs are routable. The
        // legacy kernel-side parser applied this gate; the substrate
        // parser must too so a misconfigured `r` tag (`https://…`,
        // `ws://…`, bare host, etc.) does not poison the routing cache.
        let url = match tag.get(1) {
            Some(u) if u.starts_with("wss://") => u.clone(),
            _ => continue,
        };
        match tag.get(2).map(String::as_str) {
            None | Some("") => both.push(url),
            Some("read") => read.push(url),
            Some("write") => write.push(url),
            Some(_) => {
                // Unknown marker — ignore. Conservative: drop rather than
                // guess bidirectional.
            }
        }
    }

    sort_dedup(&mut read);
    sort_dedup(&mut write);
    sort_dedup(&mut both);

    ParsedRelayList { read, write, both }
}

fn sort_dedup(v: &mut Vec<String>) {
    v.sort();
    v.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::store::RawEvent;

    fn evt(pubkey: &str, kind: u32, tags: Vec<Vec<String>>) -> VerifiedEvent {
        VerifiedEvent::from_raw_unchecked(RawEvent {
            id: "00".repeat(32),
            pubkey: pubkey.into(),
            created_at: 0,
            kind,
            tags,
            content: String::new(),
            sig: "22".repeat(64),
        })
    }

    #[test]
    fn unmarked_r_tag_lands_in_both() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser = Kind10002Parser::new(cache.clone());
        parser.parse_event(&evt("alice", 10_002, vec![
            vec!["r".into(), "wss://both.example".into()],
        ]));

        let r = cache.read_relays(&"alice".into()).unwrap();
        let w = cache.write_relays(&"alice".into()).unwrap();
        assert_eq!(r, vec!["wss://both.example".to_string()]);
        assert_eq!(w, vec!["wss://both.example".to_string()]);
    }

    #[test]
    fn marked_read_and_write_separate() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser = Kind10002Parser::new(cache.clone());
        parser.parse_event(&evt("alice", 10_002, vec![
            vec!["r".into(), "wss://r.example".into(), "read".into()],
            vec!["r".into(), "wss://w.example".into(), "write".into()],
            vec!["r".into(), "wss://b.example".into()],
        ]));

        let r = cache.read_relays(&"alice".into()).unwrap();
        let w = cache.write_relays(&"alice".into()).unwrap();
        // read = explicit-read + both
        assert!(r.contains(&"wss://r.example".to_string()));
        assert!(r.contains(&"wss://b.example".to_string()));
        assert!(!r.contains(&"wss://w.example".to_string()));
        // write = explicit-write + both
        assert!(w.contains(&"wss://w.example".to_string()));
        assert!(w.contains(&"wss://b.example".to_string()));
        assert!(!w.contains(&"wss://r.example".to_string()));
    }

    #[test]
    fn ignores_non_kind_10002() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser = Kind10002Parser::new(cache.clone());
        parser.parse_event(&evt("alice", 1, vec![
            vec!["r".into(), "wss://x.example".into()],
        ]));
        assert!(cache.is_empty());
    }

    #[test]
    fn empty_url_dropped() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser = Kind10002Parser::new(cache.clone());
        parser.parse_event(&evt("alice", 10_002, vec![
            vec!["r".into(), "".into()],
            vec!["r".into(), "wss://ok.example".into()],
        ]));

        let r = cache.read_relays(&"alice".into()).unwrap();
        assert_eq!(r, vec!["wss://ok.example".to_string()]);
    }

    #[test]
    fn unknown_marker_ignored() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser = Kind10002Parser::new(cache.clone());
        parser.parse_event(&evt("alice", 10_002, vec![
            vec!["r".into(), "wss://weird.example".into(), "sideways".into()],
            vec!["r".into(), "wss://ok.example".into()],
        ]));

        let r = cache.read_relays(&"alice".into()).unwrap();
        assert!(!r.contains(&"wss://weird.example".to_string()));
        assert!(r.contains(&"wss://ok.example".to_string()));
    }

    #[test]
    fn non_wss_url_dropped() {
        // The legacy kernel parser filtered URLs by the `wss://` scheme.
        // The substrate parser must match — `https://` / `ws://` URLs in
        // an `r` tag are misconfiguration and must not enter the cache.
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser = Kind10002Parser::new(cache.clone());
        parser.parse_event(&evt("alice", 10_002, vec![
            vec!["r".into(), "https://not-a-relay.example".into()],
            vec!["r".into(), "ws://insecure.example".into()],
            vec!["r".into(), "wss://ok.example".into()],
        ]));

        let r = cache.read_relays(&"alice".into()).unwrap();
        assert_eq!(r, vec!["wss://ok.example".to_string()]);
    }

    #[test]
    fn empty_relay_list_removes_known_entry() {
        // Seed a non-empty list, then ingest an empty kind:10002 — the
        // entry must be REMOVED (not upserted as an empty list) so
        // `MailboxCache::known` returns false, mirroring the pre-delete
        // kernel `ingest_relay_list` behaviour.
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser = Kind10002Parser::new(cache.clone());
        parser.parse_event(&evt("alice", 10_002, vec![
            vec!["r".into(), "wss://x.example".into()],
        ]));
        assert!(cache.read_relays(&"alice".into()).is_some());

        // Now an empty list (e.g. all tags filtered out, or no `r` tags
        // at all) — the parser must remove the entry.
        parser.parse_event(&evt("alice", 10_002, Vec::new()));
        assert!(
            cache.read_relays(&"alice".into()).is_none(),
            "an empty kind:10002 must remove the cache entry, not upsert []",
        );
    }

    #[test]
    fn empty_relay_list_for_unknown_author_is_noop() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser = Kind10002Parser::new(cache.clone());
        parser.parse_event(&evt("alice", 10_002, Vec::new()));
        assert!(cache.is_empty());
    }

    #[test]
    fn duplicate_urls_within_event_deduped() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser = Kind10002Parser::new(cache.clone());
        parser.parse_event(&evt("alice", 10_002, vec![
            vec!["r".into(), "wss://x.example".into()],
            vec!["r".into(), "wss://x.example".into()],
        ]));

        let r = cache.read_relays(&"alice".into()).unwrap();
        assert_eq!(r, vec!["wss://x.example".to_string()]);
    }

    #[test]
    fn registers_as_ingest_parser_trait_object() {
        // Compile-check the IngestParser shape — confirms the trait is
        // satisfied so EventIngestDispatcher::register_kind accepts it.
        let cache = Arc::new(InMemoryMailboxCache::new());
        let parser: Arc<dyn IngestParser> = Arc::new(Kind10002Parser::new(cache.clone()));

        let mut dispatcher = nmp_core::substrate::EventIngestDispatcher::new();
        dispatcher.register_kind(10_002, parser);
        dispatcher.dispatch(&evt("alice", 10_002, vec![
            vec!["r".into(), "wss://via.dispatcher".into()],
        ]));

        assert_eq!(
            cache.read_relays(&"alice".into()),
            Some(vec!["wss://via.dispatcher".into()]),
        );
    }
}
