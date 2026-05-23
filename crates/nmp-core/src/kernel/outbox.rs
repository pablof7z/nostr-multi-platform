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
use crate::util::sort_dedup;

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
            sort_dedup(authors);
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
                sort_dedup(&mut out);
                out
            }
            _ => self.bootstrap_discovery_relays(),
        }
    }

    /// Resolve a single author's relays for **discovery** fetches (kind:0/3/10002).
    ///
    /// Cold-start: no cached kind:10002 ⇒ ONLY [`crate::relay::INDEXER_RELAY_URL`].
    /// Unlike `author_write_relays`, the shared content relay is never included
    /// — profile-claim REQs must not go there.
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
        match self.author_relay_lists.get(recipient) {
            Some(list) if !list.read_relays.is_empty() || !list.both_relays.is_empty() => {
                let mut out: Vec<String> = list
                    .read_relays
                    .iter()
                    .chain(list.both_relays.iter())
                    .cloned()
                    .collect();
                sort_dedup(&mut out);
                out
            }
            _ => self.bootstrap_discovery_relays(),
        }
    }


    /// Resolve a pubkey's NIP-17 **DM inbox** relays (the kind:10050 list).
    ///
    /// NIP-17 § 2: a kind:1059 gift-wrap MUST be published to the recipient's
    /// kind:10050 DM-relay list — a relay set that is *deliberately distinct*
    /// from the kind:10002 (NIP-65) generic mailbox. kind:10050 carries
    /// `["relay", <url>]` tags (note: `relay`, not the `r` marker NIP-65 uses),
    /// letting a user route private messages to a privacy-focused relay that is
    /// not in their public read set. Collapsing the two would silently leak DM
    /// routing onto public relays.
    ///
    /// This reads the **live** kind:10050 cache (`self.dm_relay_lists`),
    /// populated by `ingest_dm_relay_list` exactly as `author_write_relays`
    /// reads `author_relay_lists` for kind:10002. The DM send path
    /// (`commands::send_gift_wrapped_dm`) consults this method to pin each
    /// kind:1059 envelope to its receiver's DM-inbox relays.
    ///
    /// Returns `None` when no kind:10050 list is known for `pubkey` — either the
    /// pubkey has never published a kind:10050, or it published one carrying no
    /// `relay` tags (an empty list, which `ingest_dm_relay_list` treats as the
    /// author clearing their DM relays and so removes the cache entry). In both
    /// cases the send path must fail closed: a kind:1059 envelope is only safe
    /// to publish to a receiver's explicit kind:10050 DM-inbox relays, never to
    /// generic Content relays.
    pub(crate) fn recipient_dm_relays(&self, pubkey: &str) -> Option<Vec<String>> {
        let relays = self.dm_relay_lists.get(pubkey)?;
        // A cached entry is never stored empty — `ingest_dm_relay_list` removes
        // the entry on an empty kind:10050 rather than caching a `Vec::new()`.
        // The guard here is belt-and-suspenders so a future caller that seeds
        // the map directly cannot return an empty `Some(Vec)` that callers
        // would treat as "route to no relays".
        if relays.is_empty() {
            None
        } else {
            Some(relays.clone())
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
            sort_dedup(ids);
        }
        by_relay
    }

    /// T132 — borrow relay-list caches as a planner-facing [`MailboxCache`].
    ///
    /// The returned adapter is the single bridge between the kernel's
    /// authoritative NIP-65 / NIP-17 relay caches and the planner's compiler.
    /// Callers pass it
    /// into [`crate::subs::SubscriptionLifecycle::recompile_and_diff`] /
    /// [`crate::subs::SubscriptionLifecycle::drain_tick`].
    #[allow(dead_code)] // Used once the kernel wires the planner driver path
    pub(crate) fn mailbox_cache_view(&self) -> KernelMailboxes<'_> {
        KernelMailboxes::new(&self.author_relay_lists, &self.dm_relay_lists)
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
        let mailboxes = KernelMailboxes::new(&self.author_relay_lists, &self.dm_relay_lists);
        self.lifecycle.drain_tick(&mailboxes)
    }

    /// V-04 Stage 2 — `KernelReducer` / wasm bridge: drain one lifecycle tick
    /// and convert the resulting [`crate::subs::WireFrame`]s into
    /// [`crate::relay::OutboundMessage`]s ready to hand to the transport.
    ///
    /// This is the wasm/`KernelReducer`-side analogue of the native actor's
    /// `wire_frames_to_outbound` bridge (`actor/outbound.rs`). It exists
    /// because `KernelReducer` (used by `nmp-wasm`) does NOT have an actor
    /// idle loop; without an inline conversion, a `CompileTrigger::ViewOpened`
    /// enqueued by a `startup_requests`-style helper would never be drained on
    /// the wasm path and the REQs would never reach the wire.
    ///
    /// Empty inbox / empty diff is a zero-cost no-op (returns
    /// `Vec::new()` before allocating anything) — matches D8.
    ///
    /// Frame-to-outbound conversion is byte-for-byte the same as
    /// `actor::outbound::wire_frames_to_outbound`: same `["REQ", sub_id, filter]`
    /// / `["CLOSE", sub_id]` shape, same canonical URL stamp, same
    /// `RelayRole::Content` fallback for unrecognized relay URLs, same
    /// `register_planner_wire_frames` call so EOSE / keep-live bookkeeping
    /// matches the native path exactly. The duplication is deliberate —
    /// `wire_frames_to_outbound` is `pub(super)` to `actor` and crosses a
    /// module boundary the kernel must not depend on (D0). If this method
    /// ever drifts from the actor bridge, the
    /// `actor::outbound::tests` regression on canonicalization
    /// (`non_canonical_wire_frame_url_is_canonicalized_on_outbound`) is the
    /// canary — port any fix here too.
    pub(crate) fn drain_lifecycle_outbound(&mut self) -> Vec<crate::relay::OutboundMessage> {
        let frames = self.drain_lifecycle_tick();
        if frames.is_empty() {
            return Vec::new();
        }
        self.register_planner_wire_frames(&frames);
        frames
            .into_iter()
            .map(|f| {
                let (relay_url, text) = match f {
                    crate::subs::WireFrame::Req {
                        relay_url,
                        sub_id,
                        filter_json,
                        ..
                    } => (relay_url, format!(r#"["REQ","{sub_id}",{filter_json}]"#)),
                    crate::subs::WireFrame::Close { relay_url, sub_id } => {
                        (relay_url, format!(r#"["CLOSE","{sub_id}"]"#))
                    }
                };
                let relay_url =
                    crate::relay::canonical_relay_url(&relay_url).unwrap_or(relay_url);
                let role = self
                    .role_for_relay_url(&relay_url)
                    .unwrap_or(crate::relay::RelayRole::Content);
                crate::relay::OutboundMessage {
                    role,
                    relay_url,
                    text,
                }
            })
            .collect()
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
    // M11 will add a `None` path when the URL is unknown; the Option is
    // intentionally forward-reserved so call sites don't need signature churn.
    #[allow(clippy::unnecessary_wraps)]
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
    dm_relays: &'a HashMap<String, Vec<String>>,
}

impl<'a> KernelMailboxes<'a> {
    /// Constructor is kernel-private — outside callers obtain a view through
    /// [`Kernel::mailbox_cache_view`] so the underlying `AuthorRelayList` type
    /// stays kernel-encapsulated.
    fn new(
        inner: &'a HashMap<String, AuthorRelayList>,
        dm_relays: &'a HashMap<String, Vec<String>>,
    ) -> Self {
        Self { inner, dm_relays }
    }
}

impl MailboxCache for KernelMailboxes<'_> {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        self.inner.get(pubkey).map(|list| MailboxSnapshot {
            write_relays: list.write_relays.clone(),
            read_relays: list.read_relays.clone(),
            both_relays: list.both_relays.clone(),
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
#[path = "outbox/tests.rs"]
mod tests;
