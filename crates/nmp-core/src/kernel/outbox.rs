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
//!
//! ## T132 — adapter into the planner
//!
//! [`KernelMailboxes`] is a zero-allocation borrow of `author_relay_lists`
//! that implements the planner's [`crate::planner::MailboxCache`] trait. The
//! kernel passes `&KernelMailboxes(&self.author_relay_lists)` into
//! `SubscriptionLifecycle::recompile_and_diff` so the planner and the
//! publish/read-side outbox paths read from the same kind:10002 cache. This
//! eliminates the dual-source-of-truth hazard surfaced in HB44 research: the
//! orphan `MailboxCache` field on `SubscriptionLifecycle` was never populated
//! in production, so the planner saw an empty cache while the publish path
//! routed off real NIP-65 data.

use std::collections::{BTreeMap, HashMap};

use super::types::AuthorRelayList;
use super::Kernel;
use crate::planner::{MailboxCache, MailboxSnapshot, Pubkey};
use crate::relay::RelayRole;

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
    ///
    /// T140: the M1 follow-feed emitter (`maybe_open_timeline`) was the only
    /// production caller; it is retired (the M2 planner now partitions the
    /// follow feed by NIP-65 write relays). This helper is retained `cfg(test)`
    /// as the executable specification of the per-relay outbox partition the
    /// planner reproduces — deleting it would lose that regression coverage.
    #[cfg(test)]
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
            _ => self.bootstrap_discovery_relays(),
        }
    }

    /// Resolve a single author's relays for **discovery** fetches (kind:0/3/10002).
    ///
    /// Cold-start: no cached kind:10002 ⇒ ONLY [`crate::relay::INDEXER_RELAY_URL`]
    /// (purplepag.es). Unlike `author_write_relays`, the content relay (damus.io)
    /// is never included — profile-claim REQs must not go there.
    /// NIP-65 known: returns the author's declared write relays (they published
    /// kind:0 there, so that is the right place to read it back).
    pub(crate) fn author_indexer_relays(&self, author: &str) -> Vec<String> {
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
            _ => self.bootstrap_urls_for_role(RelayRole::Indexer),
        }
    }

    /// Resolve a single recipient's NIP-65 **read** relays (inbox direction —
    /// the relays a `#p`-tagged pubkey reads, where notifications/DMs land).
    ///
    /// Cold-start: no cached kind:10002 ⇒ the bootstrap discovery seed.
    ///
    /// T122 / codex R2: also serves the active account's hashtag firehose —
    /// the user is the recipient of their own hashtag interest, so the
    /// routing destination is their declared read relays.
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
            _ => self.bootstrap_discovery_relays(),
        }
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

    /// Partition `ids` by their **original-event author's** NIP-65 write
    /// relays — the thread hydration outbox path (T121, codex R1).
    ///
    /// For each id, look up the cached event in `self.events`. If found, route
    /// the id to every relay in the author's resolved write set. If the id is
    /// not in the local store (i.e. we have no record of who wrote it), route
    /// it to every [`BOOTSTRAP_DISCOVERY_RELAYS`] seed — the cold-start
    /// discovery path: that's the only socket we can ask "who wrote this id?"
    /// on without violating D3.
    ///
    /// D3 (outbox automatic): reply threads should not depend on bootstrap
    /// relays carrying the conversation — the original author's write relays
    /// are the canonical home of both their own event and (heuristically) the
    /// kind:1/6 replies that reference it via `#e`. Reply authors of course
    /// write to *their own* relays; routing reply-fetch to the root author's
    /// relays is a deliberate compromise: it converges on whichever relays
    /// already serve the thread context rather than fanning to every
    /// participant. See codex review R1 of T105 keystone for the rationale.
    ///
    /// Empty input yields an empty map (caller emits nothing).
    pub(crate) fn partition_ids_by_author_write_relays(
        &self,
        ids: &[String],
    ) -> BTreeMap<String, Vec<String>> {
        let mut by_relay: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in ids {
            let relays = match self.events.get(id) {
                Some(event) => self.author_write_relays(&event.author),
                None => self.bootstrap_discovery_relays(),
            };
            for relay in relays {
                by_relay.entry(relay).or_default().push(id.clone());
            }
        }
        // Stable id order within each relay slice (plan-id stability / D8).
        for ids in by_relay.values_mut() {
            ids.sort();
            ids.dedup();
        }
        by_relay
    }

    /// T132 — borrow `author_relay_lists` as a planner-facing [`MailboxCache`].
    ///
    /// The returned adapter is the single bridge between the kernel's
    /// authoritative NIP-65 cache and the planner's compiler. Callers pass it
    /// into [`crate::subs::SubscriptionLifecycle::recompile_and_diff`] /
    /// [`crate::subs::SubscriptionLifecycle::drain_tick`].
    #[allow(dead_code)] // Used once the kernel wires the planner driver path
    pub(crate) fn mailbox_cache_view(&self) -> KernelMailboxes<'_> {
        KernelMailboxes::new(&self.author_relay_lists)
    }

    /// T142 — actor idle-loop bridge: drain one tick of the subscription
    /// lifecycle and return wire frames.
    ///
    /// Splits the borrow across the two kernel fields so the Rust borrow
    /// checker is satisfied: `author_relay_lists` is borrowed immutably for the
    /// adapter, and `lifecycle` is borrowed mutably for the drain call. Both
    /// fields live on `Kernel` (not accessed via `&self`/`&mut self` method
    /// chains), which Rust allows as simultaneous non-overlapping field borrows.
    ///
    /// Per D8: an empty trigger inbox is a zero-cost no-op (no allocation, no
    /// compile pass). This is the common case on a quiet idle tick.
    pub(crate) fn drain_lifecycle_tick(&mut self) -> Vec<crate::subs::WireFrame> {
        let mailboxes = KernelMailboxes::new(&self.author_relay_lists);
        self.lifecycle.drain_tick(&mailboxes)
    }

    /// T142 — role lookup: map a resolved relay URL to its `RelayRole` lane.
    ///
    /// Option A from the spec §3.2: bootstrap-URL matching with Content fallback.
    /// The two bootstrap seeds are the only URLs with known role assignments at
    /// this stage; any other URL (per-author NIP-65 write relay resolved by the
    /// planner) falls through to `RelayRole::Content`, which accepts generic
    /// content-fetch REQs safely. This is correct because the planner only
    /// generates REQs for the content lane today (M2 scope).
    ///
    /// T105 / T-relay-url-normalize: the `url` argument is canonicalized before
    /// it is compared against `RelayEditRow.url`. `add_relay` always stores the
    /// canonical form (lowercase scheme+host, empty-path trailing slash
    /// stripped), so a raw, user-typed or non-canonical caller input — e.g. a
    /// kind:10002 NIP-65 write relay with a mixed-case host — would otherwise
    /// silently miss the matching edit row and fall through to the Content
    /// fallback, mislabelling an `indexer` relay's transport lane. The role is
    /// a diagnostic lane label only (T105), so a miss is not a routing fault,
    /// but the canonicalized compare keeps the projected lane accurate.
    ///
    /// M11 will sharpen this to a per-URL lookup once the URL→role index is
    /// maintained by the relay-lifecycle manager.
    pub(crate) fn role_for_relay_url(&self, url: &str) -> Option<crate::relay::RelayRole> {
        use crate::relay::RelayRole;
        // Canonicalize so a raw/non-canonical input matches the canonical
        // `RelayEditRow.url` keys. Fall back to the raw string for inputs that
        // do not parse as ws/wss (no edit row will match those anyway).
        let lookup = crate::relay::canonical_relay_url(url)
            .unwrap_or_else(|| url.to_string());
        for row in &self.relay_edit_rows {
            if row.url == lookup {
                if crate::actor::has_role(&row.role, "indexer") {
                    return Some(RelayRole::Indexer);
                }
                if crate::actor::has_role(&row.role, "read")
                    || crate::actor::has_role(&row.role, "write")
                {
                    return Some(RelayRole::Content);
                }
            }
        }
        // Returns `Some` unconditionally today (Content fallback). The `Option`
        // return is retained so M11's per-URL index can distinguish "no role
        // known for this URL" (`None`) from "explicitly Content" without a
        // signature change at every call site.
        Some(RelayRole::Content)
    }
}

// ─── KernelMailboxes adapter (T132) ──────────────────────────────────────────

/// Borrowed `MailboxCache` view over the kernel's `author_relay_lists`.
///
/// Builds a [`MailboxSnapshot`] on demand from the kernel's `AuthorRelayList`
/// entries. Allocation occurs only on `get()` hit (clone of three relay
/// `Vec`s) and only on the cold recompile path — the live ingest hot path
/// never touches this adapter (D8).
///
/// Lifetime: borrows the kernel's map, so the kernel must outlive the
/// adapter. In practice the adapter is created at the call site of
/// `recompile_and_diff` and dropped at the end of that call.
pub(crate) struct KernelMailboxes<'a> {
    inner: &'a HashMap<String, AuthorRelayList>,
}

impl<'a> KernelMailboxes<'a> {
    /// Constructor is kernel-private — outside callers obtain a view through
    /// [`Kernel::mailbox_cache_view`] so the underlying `AuthorRelayList` type
    /// stays kernel-encapsulated.
    fn new(inner: &'a HashMap<String, AuthorRelayList>) -> Self {
        Self { inner }
    }
}

impl<'a> MailboxCache for KernelMailboxes<'a> {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        self.inner.get(pubkey).map(|list| MailboxSnapshot {
            write_relays: list.write_relays.clone(),
            read_relays: list.read_relays.clone(),
            both_relays: list.both_relays.clone(),
        })
    }

    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
        self.inner
            .iter()
            .map(|(pk, list)| {
                (
                    pk.clone(),
                    MailboxSnapshot {
                        write_relays: list.write_relays.clone(),
                        read_relays: list.read_relays.clone(),
                        both_relays: list.both_relays.clone(),
                    },
                )
            })
            .collect()
    }

    fn generation(&self) -> u64 {
        // Phase 1: no generation counter on the kernel-side cache. Plan-id
        // stability is preserved at the kernel call site (the kernel triggers
        // a recompile only when a kind:10002 actually mutated the map — see
        // `ingest_relay_list`'s `should_replace` guard). Phase 2: a monotonic
        // counter on `Kernel` advances on every `should_replace` insert.
        0
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
            kernel.bootstrap_discovery_relays()
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
            kernel.bootstrap_discovery_relays()
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

    // ── T132 parity tests ────────────────────────────────────────────────
    //
    // After T132, the planner consumes mailbox data through a `KernelMailboxes`
    // adapter that borrows `Kernel::author_relay_lists`. These tests pin the
    // invariant the task closes: the publish-path resolver
    // (`author_write_relays`) and the planner-path adapter return identical
    // data for the same NIP-65 input. If they ever drift, the kernel-managed
    // ingest path and the planner compile path will be looking at different
    // truths — exactly the dual-source-of-truth hazard T132 was filed to fix.

    #[test]
    fn t132_parity_publish_path_and_planner_adapter_agree_on_kind10002() {
        use crate::planner::MailboxCache;
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.author_relay_lists.insert(
            "alice".to_string(),
            relay_list(
                &["wss://r.read"],
                &["wss://r.write.a", "wss://r.write.b"],
                &["wss://r.both"],
            ),
        );

        // Publish-path view: write + both, sorted/deduped.
        let publish_path = kernel.author_write_relays("alice");
        assert_eq!(
            publish_path,
            vec!["wss://r.both", "wss://r.write.a", "wss://r.write.b"]
        );

        // Planner-path view via the adapter — outbox_relays iterates
        // write ∪ both in the same order they appear in the snapshot.
        let view = kernel.mailbox_cache_view();
        let snap = view.get(&"alice".to_string()).expect("alice cached");
        let mut planner_path: Vec<String> = snap.outbox_relays().cloned().collect();
        planner_path.sort();
        planner_path.dedup();
        assert_eq!(planner_path, publish_path);
    }

    #[test]
    fn t132_parity_empty_kind10002_clears_both_views() {
        use crate::planner::MailboxCache;
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel
            .author_relay_lists
            .insert("alice".to_string(), relay_list(&[], &["wss://a"], &[]));
        // Simulate the "empty kind:10002" branch of ingest_relay_list — the
        // entry is removed entirely (see relay_list.rs lines 30-36).
        kernel.author_relay_lists.remove("alice");

        // Publish path falls back to bootstrap seed.
        let publish_path = kernel.author_write_relays("alice");
        assert_eq!(
            publish_path,
            kernel.bootstrap_discovery_relays()
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );

        // Planner adapter sees None (cold-start) — the planner Case A then
        // routes the author through indexer_relays / bootstrap, matching the
        // publish-path fallback semantically (both surfaces use the same
        // cold-start fallback strategy via their respective code paths).
        let view = kernel.mailbox_cache_view();
        assert!(view.get(&"alice".to_string()).is_none());
    }

    #[test]
    fn t132_parity_newer_kind10002_supersedes_on_both_views() {
        use crate::planner::MailboxCache;
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // Older entry.
        kernel.author_relay_lists.insert(
            "alice".to_string(),
            AuthorRelayList {
                event_id: "older".to_string(),
                created_at: 100,
                read_relays: vec![],
                write_relays: vec!["wss://old.write".to_string()],
                both_relays: vec![],
            },
        );
        // Newer entry replaces (simulating the should_replace branch in
        // ingest_relay_list).
        kernel.author_relay_lists.insert(
            "alice".to_string(),
            AuthorRelayList {
                event_id: "newer".to_string(),
                created_at: 200,
                read_relays: vec![],
                write_relays: vec!["wss://new.write".to_string()],
                both_relays: vec![],
            },
        );

        // Publish path returns only the new write relay.
        let publish_path = kernel.author_write_relays("alice");
        assert_eq!(publish_path, vec!["wss://new.write".to_string()]);

        // Planner adapter sees the same new data.
        let view = kernel.mailbox_cache_view();
        let snap = view.get(&"alice".to_string()).expect("alice cached");
        let planner_path: Vec<String> = snap.outbox_relays().cloned().collect();
        assert_eq!(planner_path, vec!["wss://new.write".to_string()]);
    }

    // ── role_for_relay_url canonicalization (T105 / T-relay-url-normalize) ──
    //
    // `RelayEditRow.url` is always stored canonical (`add_relay` canonicalizes
    // before insert). `role_for_relay_url` must canonicalize its *input* too,
    // or a raw / mixed-case caller URL silently misses the matching edit row
    // and mislabels the transport lane as Content.

    #[test]
    fn role_for_relay_url_matches_indexer_via_noncanonical_input() {
        use crate::relay::RelayRole;
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // Edit row stored canonical (lowercase host, no trailing slash) — the
        // exact form `add_relay` would persist.
        kernel.set_relay_edit_rows(vec![RelayEditRow {
            url: "wss://purplepag.es".to_string(),
            role: "indexer".to_string(),
        }]);

        // A non-canonical caller input (mixed-case host + trailing slash) must
        // still resolve to the Indexer lane, not fall through to Content.
        assert_eq!(
            kernel.role_for_relay_url("wss://Purplepag.es/"),
            Some(RelayRole::Indexer),
            "non-canonical input must canonicalize before matching edit row"
        );
        // Canonical input keeps working.
        assert_eq!(
            kernel.role_for_relay_url("wss://purplepag.es"),
            Some(RelayRole::Indexer),
        );
    }

    #[test]
    fn role_for_relay_url_unknown_url_falls_back_to_content() {
        use crate::relay::RelayRole;
        let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // No edit rows configured — any URL falls through to the Content lane.
        assert_eq!(
            kernel.role_for_relay_url("wss://some.unknown.relay"),
            Some(RelayRole::Content),
        );
    }

    #[test]
    fn t132_recompile_uses_kernel_mailbox_cache_for_plan_partition() {
        // The seam-proof test: build a SubscriptionLifecycle, push a
        // LogicalInterest with `alice` as the author, and feed it the kernel's
        // mailbox view. Assert the resulting plan partitions onto alice's
        // resolved write relays, NOT the indexer / bootstrap seed.
        use crate::planner::{
            InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
        };
        use crate::subs::SubscriptionLifecycle;
        use std::collections::BTreeSet;

        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.author_relay_lists.insert(
            "alice-pubkey".to_string(),
            relay_list(&[], &["wss://alice.write"], &[]),
        );

        let mut lifecycle = SubscriptionLifecycle::new();
        let interest = LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: {
                    let mut s = BTreeSet::new();
                    s.insert("alice-pubkey".to_string());
                    s
                },
                kinds: [1u32].into_iter().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
        };
        lifecycle.registry_mut().push(interest);

        let view = kernel.mailbox_cache_view();
        let frames = lifecycle
            .recompile_and_diff(&view)
            .expect("recompile should succeed");

        // The plan must include at least one REQ on alice's resolved write
        // relay — proving the kernel-side mailbox view fed the planner, not
        // the (now-deleted) lifecycle-internal cache.
        let alice_relay_frames: Vec<_> = frames
            .iter()
            .filter(|f| match f {
                crate::subs::WireFrame::Req { relay_url, .. } => {
                    relay_url == "wss://alice.write"
                }
                _ => false,
            })
            .collect();
        assert!(
            !alice_relay_frames.is_empty(),
            "expected at least one REQ on alice's resolved write relay; got: {frames:?}",
        );
    }
}
