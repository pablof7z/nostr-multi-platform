//! Read-side outbox resolver — T105.
//!
//! The publish path already resolves NIP-65 write relays via
//! `crate::publish::Nip65OutboxResolver` (reading kind:10002 from the shared
//! `EventStore`). This module is the *read-side* analogue: it turns a set of
//! authors into the per-relay author partition the live REQ emitters fan out
//! over, reading the same live NIP-65 cache (`self.author_relay_lists`,
//! populated by `ingest_relay_list`) the publish path reads.
//!
//! D3 (outbox automatic — `docs/product-spec/overview-and-dx.md` §1.5): an
//! author's events are subscribed for at *their declared write relays*. Only
//! when no kind:10002 is cached for an author does that author fall through to
//! the cold-start [`BOOTSTRAP_DISCOVERY_RELAYS`] seed — and that seed is the
//! discovery interest, not a routing default: once the author's kind:10002
//! lands (A1 / `Trigger::Nip65Arrived`), the next emission re-partitions onto
//! the resolved relays.
//!
//! D8 (no per-event alloc on the resolve path): resolution allocates once per
//! emission (a `BTreeMap<relay, Vec<author>>`), never per event. The hot ingest
//! path does not call the resolver.

use std::collections::BTreeMap;

use super::Kernel;

impl Kernel {
    /// Partition `authors` by their NIP-65 **write** relays (outbox direction).
    ///
    /// Returns a deterministically-ordered map `relay_url → authors served by
    /// that relay`. An author with a cached kind:10002 contributes to each of
    /// their declared write/both relays. An author with no cached relay list
    /// contributes to every [`BOOTSTRAP_DISCOVERY_RELAYS`] seed — the
    /// cold-start discovery path, replaced on the next emission once their
    /// relay list arrives.
    ///
    /// Empty input yields an empty map (caller emits nothing).
    pub(crate) fn partition_authors_by_write_relays(
        &self,
        authors: &[String],
    ) -> BTreeMap<String, Vec<String>> {
        let mut by_relay: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for author in authors {
            let relays = self.author_write_relays(author);
            for relay in relays {
                by_relay.entry(relay).or_default().push(author.clone());
            }
        }
        // Stable author order within each relay slice (plan-id stability / D8).
        for authors in by_relay.values_mut() {
            authors.sort();
            authors.dedup();
        }
        by_relay
    }

    /// Resolve a single author's NIP-65 write relays (write + both markers).
    ///
    /// Cold-start: no cached kind:10002 ⇒ the [`BOOTSTRAP_DISCOVERY_RELAYS`]
    /// seed (discovery interest only, per D3).
    pub(crate) fn author_write_relays(&self, author: &str) -> Vec<String> {
        match self.author_relay_lists.get(author) {
            Some(list) if !list.write_relays.is_empty() || !list.both_relays.is_empty() => {
                let mut out: Vec<String> = list
                    .write_relays
                    .iter()
                    .chain(list.both_relays.iter())
                    .cloned()
                    .collect();
                out.sort();
                out.dedup();
                out
            }
            _ => Self::bootstrap_discovery_relays(),
        }
    }

    /// Resolve a single recipient's NIP-65 **read** relays (inbox direction —
    /// the relays a `#p`-tagged pubkey reads, where notifications/DMs land).
    ///
    /// Cold-start: no cached kind:10002 ⇒ the bootstrap discovery seed.
    #[allow(dead_code)] // Reserved for inbox emitters (NIP-04/17/65 read fan-out)
    pub(crate) fn recipient_read_relays(&self, recipient: &str) -> Vec<String> {
        match self.author_relay_lists.get(recipient) {
            Some(list) if !list.read_relays.is_empty() || !list.both_relays.is_empty() => {
                let mut out: Vec<String> = list
                    .read_relays
                    .iter()
                    .chain(list.both_relays.iter())
                    .cloned()
                    .collect();
                out.sort();
                out.dedup();
                out
            }
            _ => Self::bootstrap_discovery_relays(),
        }
    }

    /// The cold-start discovery seed as an owned `Vec` (D3: discovery only).
    pub(crate) fn bootstrap_discovery_relays() -> Vec<String> {
        crate::relay::BOOTSTRAP_DISCOVERY_RELAYS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    /// True iff every author in `authors` has a cached kind:10002 relay list
    /// (i.e. the next emission will route entirely off resolved relays, no
    /// bootstrap seed). Used by the A1 recompilation trigger to decide whether
    /// a kind:10002 arrival should re-emit a live REQ onto resolved relays.
    #[allow(dead_code)] // Used by recompilation trigger once wired
    pub(crate) fn all_authors_have_relay_lists(&self, authors: &[String]) -> bool {
        authors
            .iter()
            .all(|a| self.author_relay_lists.contains_key(a))
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::kernel::types::AuthorRelayList;
    use crate::relay::{BOOTSTRAP_DISCOVERY_RELAYS, DEFAULT_VISIBLE_LIMIT};

    fn relay_list(read: &[&str], write: &[&str], both: &[&str]) -> AuthorRelayList {
        AuthorRelayList {
            event_id: "x".to_string(),
            created_at: 1,
            read_relays: read.iter().map(|s| s.to_string()).collect(),
            write_relays: write.iter().map(|s| s.to_string()).collect(),
            both_relays: both.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn author_write_relays_returns_write_plus_both_when_cached() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.author_relay_lists.insert(
            "alice".to_string(),
            relay_list(&["wss://r.in"], &["wss://r.out"], &["wss://r.both"]),
        );

        let relays = kernel.author_write_relays("alice");
        assert_eq!(relays, vec!["wss://r.both", "wss://r.out"]);
    }

    #[test]
    fn author_write_relays_falls_back_to_bootstrap_when_uncached() {
        let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let relays = kernel.author_write_relays("never-seen");
        assert_eq!(
            relays,
            BOOTSTRAP_DISCOVERY_RELAYS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn author_write_relays_falls_back_when_all_buckets_empty() {
        // Defensive: an entry with no write/both falls back to bootstrap so
        // we don't silently drop the author from the plan.
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel
            .author_relay_lists
            .insert("alice".to_string(), relay_list(&["wss://r.in"], &[], &[]));
        let relays = kernel.author_write_relays("alice");
        assert_eq!(
            relays,
            BOOTSTRAP_DISCOVERY_RELAYS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn partition_authors_groups_by_resolved_write_relays() {
        // Two authors with DISTINCT write relays — the test the task pins:
        // a follow-feed REQ must fan out to each followed author's resolved
        // write relays, NOT the constants.
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.author_relay_lists.insert(
            "alice".to_string(),
            relay_list(&[], &["wss://alice.relay"], &[]),
        );
        kernel.author_relay_lists.insert(
            "bob".to_string(),
            relay_list(&[], &["wss://bob.relay"], &["wss://shared.relay"]),
        );
        let parts = kernel
            .partition_authors_by_write_relays(&["alice".to_string(), "bob".to_string()]);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts.get("wss://alice.relay").unwrap(), &vec!["alice"]);
        assert_eq!(parts.get("wss://bob.relay").unwrap(), &vec!["bob"]);
        assert_eq!(parts.get("wss://shared.relay").unwrap(), &vec!["bob"]);
    }

    #[test]
    fn partition_authors_uses_bootstrap_for_uncached_authors() {
        // Cold-start: author has no cached kind:10002. The bootstrap seed
        // must appear in the plan so the first discovery REQ has somewhere
        // to leave on; once the kind:10002 arrives the next emission
        // re-partitions onto the resolved relays (A1 recompilation trigger).
        let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let parts = kernel.partition_authors_by_write_relays(&["uncached".to_string()]);
        for seed in BOOTSTRAP_DISCOVERY_RELAYS {
            assert!(
                parts.contains_key(*seed),
                "bootstrap seed {seed} must serve uncached author"
            );
        }
    }

    #[test]
    fn all_authors_have_relay_lists_distinguishes_cold_warm() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        assert!(!kernel.all_authors_have_relay_lists(&["alice".to_string()]));
        kernel
            .author_relay_lists
            .insert("alice".to_string(), relay_list(&[], &["wss://a"], &[]));
        assert!(kernel.all_authors_have_relay_lists(&["alice".to_string()]));
        assert!(!kernel
            .all_authors_have_relay_lists(&["alice".to_string(), "bob".to_string()]));
    }

    #[test]
    fn recipient_read_relays_returns_read_plus_both() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.author_relay_lists.insert(
            "bob".to_string(),
            relay_list(&["wss://r.in"], &["wss://r.out"], &["wss://r.both"]),
        );
        let relays = kernel.recipient_read_relays("bob");
        assert_eq!(relays, vec!["wss://r.both", "wss://r.in"]);
    }
}
