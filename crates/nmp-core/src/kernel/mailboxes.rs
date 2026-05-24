//! NIP-65 cache-read helpers + planner-side [`MailboxCache`] adapter.
//!
//! Step 3 of `docs/architecture/crate-boundaries.md` (V-50) cuts the
//! kernel over to `Arc<dyn OutboxRouter>` + `Arc<dyn MailboxCache>`.
//! This file is the post-step-3 home of the survivors of the deleted
//! `kernel/outbox.rs`:
//!
//! - `author_write_relays` / `author_indexer_relays` /
//!   `recipient_read_relays` — cache-read helpers with bootstrap
//!   fallback policy. Read through [`Kernel::mailbox_cache`] (the
//!   substrate [`MailboxCache`] handle, which step 3 made the single
//!   source of truth for kind:10002 data); apply the kernel's
//!   bootstrap-discovery / indexer-seed fallback when the cache misses.
//!   Not routing decisions in the new model — those flow through
//!   [`crate::substrate::OutboxRouter`]. These helpers wrap "cache hit
//!   with fallback to the kernel-owned bootstrap seed" because the
//!   kernel owns the role-to-URL mapping (`RelayEditRow`); a Layer-2
//!   router doesn't.
//! - `recipient_dm_relays` — NIP-17 kind:10050 cache reader (still
//!   uses the bespoke `dm_relay_lists` HashMap; V-40 will move this to
//!   `nmp-nip17`).
//! - `partition_ids_by_author_write_relays` — thread-hydration outbox
//!   path. Wraps `author_write_relays`.
//! - [`KernelMailboxes`] — the planner-side adapter that bridges the
//!   substrate [`crate::substrate::MailboxCache`] to the planner's own
//!   `MailboxCache` trait (different shape: separate read/write/both
//!   fields plus `dm_inbox_relays`). Both traits coexist until step 9
//!   extracts the planner.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::Kernel;
use crate::planner::{MailboxCache as PlannerMailboxCache, MailboxSnapshot, Pubkey};
use crate::relay::RelayRole;
use crate::substrate::MailboxCache as SubstrateMailboxCache;
use crate::util::sort_dedup;

impl Kernel {
    /// Resolve a single author's NIP-65 write relays (write + both markers).
    ///
    /// Reads through the injected substrate [`MailboxCache`]. Cold-start:
    /// no cached kind:10002 ⇒ the [`Kernel::bootstrap_discovery_relays`]
    /// seed (discovery interest only, per D3).
    pub(crate) fn author_write_relays(&self, author: &str) -> Vec<String> {
        match self.mailbox_cache().snapshot(&author.to_string()) {
            Some(parsed) if !parsed.write.is_empty() || !parsed.both.is_empty() => {
                let mut out: Vec<String> =
                    parsed.write.iter().chain(parsed.both.iter()).cloned().collect();
                sort_dedup(&mut out);
                out
            }
            _ => self.bootstrap_discovery_relays(),
        }
    }

    /// Resolve a single author's relays for **discovery** fetches (kind:0/3/10002).
    ///
    /// Cold-start: no cached kind:10002 ⇒ ONLY `INDEXER_RELAY_URL`.
    /// Unlike `author_write_relays`, the shared content relay is never
    /// included — profile-claim REQs must not go there. NIP-65 known:
    /// returns the author's declared write relays (they published kind:0
    /// there, so that is the right place to read it back).
    pub(crate) fn author_indexer_relays(&self, author: &str) -> Vec<String> {
        match self.mailbox_cache().snapshot(&author.to_string()) {
            Some(parsed) if !parsed.write.is_empty() || !parsed.both.is_empty() => {
                let mut out: Vec<String> =
                    parsed.write.iter().chain(parsed.both.iter()).cloned().collect();
                sort_dedup(&mut out);
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
        match self.mailbox_cache().snapshot(&recipient.to_string()) {
            Some(parsed) if !parsed.read.is_empty() || !parsed.both.is_empty() => {
                let mut out: Vec<String> =
                    parsed.read.iter().chain(parsed.both.iter()).cloned().collect();
                sort_dedup(&mut out);
                out
            }
            _ => self.bootstrap_discovery_relays(),
        }
    }

    /// Resolve a pubkey's NIP-17 **DM inbox** relays (the kind:10050 list).
    ///
    /// NIP-17 § 2: a kind:1059 gift-wrap MUST be published to the
    /// recipient's kind:10050 DM-relay list — a relay set that is
    /// *deliberately distinct* from the kind:10002 (NIP-65) generic
    /// mailbox. kind:10050 carries `["relay", <url>]` tags (note:
    /// `relay`, not the `r` marker NIP-65 uses), letting a user route
    /// private messages to a privacy-focused relay that is not in their
    /// public read set. Collapsing the two would silently leak DM
    /// routing onto public relays.
    ///
    /// This reads the **live** kind:10050 cache (`self.dm_relay_lists`),
    /// populated by `ingest_dm_relay_list`. The DM send path
    /// (`commands::send_gift_wrapped_dm`) consults this method to pin
    /// each kind:1059 envelope to its receiver's DM-inbox relays.
    ///
    /// Returns `None` when no kind:10050 list is known for `pubkey` —
    /// either the pubkey has never published a kind:10050, or it
    /// published one carrying no `relay` tags (an empty list, which
    /// `ingest_dm_relay_list` treats as the author clearing their DM
    /// relays and so removes the cache entry). In both cases the send
    /// path must fail closed: a kind:1059 envelope is only safe to
    /// publish to a receiver's explicit kind:10050 DM-inbox relays,
    /// never to generic Content relays.
    ///
    /// V-40 (step 6 of the crate-boundary migration) moves this cache
    /// to `nmp-nip17`; the kernel field `dm_relay_lists` and this
    /// method go with it.
    pub(crate) fn recipient_dm_relays(&self, pubkey: &str) -> Option<Vec<String>> {
        let relays = self.dm_relay_lists.get(pubkey)?;
        // A cached entry is never stored empty — `ingest_dm_relay_list`
        // removes the entry on an empty kind:10050 rather than caching a
        // `Vec::new()`. The guard here is belt-and-suspenders so a
        // future caller that seeds the map directly cannot return an
        // empty `Some(Vec)` that callers would treat as "route to no
        // relays".
        if relays.is_empty() {
            None
        } else {
            Some(relays.clone())
        }
    }

    /// Partition `ids` by their **original-event author's** NIP-65 write
    /// relays — the thread hydration outbox path (T121, codex R1).
    ///
    /// For each id, look up the cached event in `self.events`. If found,
    /// route the id to every relay in the author's resolved write set.
    /// If the id is not in the local store (i.e. we have no record of
    /// who wrote it), route it to every bootstrap-discovery seed — the
    /// cold-start discovery path: that's the only socket we can ask
    /// "who wrote this id?" on without violating D3.
    ///
    /// D3 (outbox automatic): reply threads should not depend on
    /// bootstrap relays carrying the conversation — the original
    /// author's write relays are the canonical home of both their own
    /// event and (heuristically) the kind:1/6 replies that reference it
    /// via `#e`. Reply authors of course write to *their own* relays;
    /// routing reply-fetch to the root author's relays is a deliberate
    /// compromise: it converges on whichever relays already serve the
    /// thread context rather than fanning to every participant. See
    /// codex review R1 of T105 keystone for the rationale.
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
            sort_dedup(ids);
        }
        by_relay
    }
}

// ─── KernelMailboxes adapter (T132) ──────────────────────────────────────────

/// Adapter — present the substrate [`SubstrateMailboxCache`] (NIP-65
/// kind:10002, owned by the kernel via `mailbox_cache`) plus the
/// kernel's bespoke `dm_relay_lists` HashMap (NIP-17 kind:10050) as a
/// planner-side [`PlannerMailboxCache`].
///
/// Two traits, one bridge. The planner trait pre-dates the substrate
/// trait introduced in step 1.c / 1.d, and uses a different shape
/// (`get` → `MailboxSnapshot` with read/write/both *separate*, plus
/// `dm_inbox_relays`). Step 9 extracts the planner crate and the two
/// traits collapse into one then; until then this adapter is the
/// translation layer.
///
/// Lifetime: holds an `Arc` clone of the substrate cache (cheap — the
/// cache is `Arc<dyn …>` already) plus a borrow of the kernel's
/// `dm_relay_lists` HashMap. The adapter is built per
/// `drain_lifecycle_tick` call and dropped at the end of that call.
pub(crate) struct KernelMailboxes<'a> {
    inner: Arc<dyn SubstrateMailboxCache>,
    dm_relays: &'a HashMap<String, Vec<String>>,
}

impl<'a> KernelMailboxes<'a> {
    /// Constructor is kernel-private — outside callers obtain a view
    /// through [`Kernel::drain_lifecycle_tick`].
    pub(super) fn new(
        inner: Arc<dyn SubstrateMailboxCache>,
        dm_relays: &'a HashMap<String, Vec<String>>,
    ) -> Self {
        Self { inner, dm_relays }
    }
}

impl PlannerMailboxCache for KernelMailboxes<'_> {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        self.inner.snapshot(pubkey).map(|p| MailboxSnapshot {
            write_relays: p.write,
            read_relays: p.read,
            both_relays: p.both,
        })
    }

    fn dm_inbox_relays(&self, pubkey: &Pubkey) -> Option<Vec<String>> {
        self.dm_relays
            .get(pubkey)
            .filter(|relays| !relays.is_empty())
            .cloned()
    }

    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
        self.inner
            .snapshot_all()
            .into_iter()
            .map(|(pk, p)| {
                (
                    pk,
                    MailboxSnapshot {
                        write_relays: p.write,
                        read_relays: p.read,
                        both_relays: p.both,
                    },
                )
            })
            .collect()
    }

    fn generation(&self) -> u64 {
        // Phase 1: no generation counter on the substrate cache. Plan-id
        // stability is preserved at the kernel call site (the kernel
        // triggers a recompile only when a kind:10002 actually mutated
        // the cache — see `ingest::relay_list::ingest_relay_list`'s
        // empty-vs-non-empty guard).
        0
    }
}
