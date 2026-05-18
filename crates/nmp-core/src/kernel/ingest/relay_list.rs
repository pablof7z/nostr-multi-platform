//! Kind:10002 (NIP-65 relay list) ingest.

use super::super::*;
use crate::subs::CompileTrigger;

impl Kernel {
    /// Ingest a kind:10002 NIP-65 relay-list event into `author_relay_lists`.
    ///
    /// Only called after `verify_and_persist` returns `Inserted | Replaced` (D4).
    ///
    /// ## Empty-list semantics
    ///
    /// If the canonical kind:10002 carries an **empty** relay list (all three
    /// buckets are empty), this means the author has explicitly cleared their
    /// NIP-65 metadata.  In that case the existing cache entry is *removed*
    /// rather than left stale — an empty-but-canonical event must not allow the
    /// old relay list to persist indefinitely.
    ///
    /// ## Supersession guard
    ///
    /// The local cache guard uses strict `>` on `created_at` with a
    /// lexicographic event-id tiebreak, mirroring the store's supersession
    /// logic exactly.  This function is only reached after the store confirmed
    /// this event won, so the guard is belt-and-suspenders protection against
    /// any race or re-ordering at the call site.
    pub(in crate::kernel) fn ingest_relay_list(&mut self, event: NostrEvent) {
        let relay_list = parse_relay_list(&event.id, event.created_at, &event.tags);

        // Empty relay list from a canonical newer event: author cleared NIP-65.
        // Remove the stale cache entry so it does not outlive the author's intent.
        if relay_list.read_relays.is_empty()
            && relay_list.write_relays.is_empty()
            && relay_list.both_relays.is_empty()
        {
            self.author_relay_lists.remove(&event.pubkey);
            return;
        }

        // This function is only called after verify_and_persist returned
        // Inserted | Replaced, so the store already enforced strict `>` with
        // lexicographic event-id tiebreak. The local cache guard below is a
        // belt-and-suspenders check that mirrors the store's supersession
        // logic exactly (strict `>` on timestamp; same-ts resolved by
        // lexicographically smaller event id wins).
        let should_replace = self
            .author_relay_lists
            .get(&event.pubkey)
            .map(|current| {
                relay_list.created_at > current.created_at
                    || (relay_list.created_at == current.created_at && event.id < current.event_id)
            })
            .unwrap_or(true);
        if should_replace {
            self.log(format!(
                "NIP-65 {} read={} write={} both={}",
                short_hex(&event.pubkey),
                relay_list.read_relays.len(),
                relay_list.write_relays.len(),
                relay_list.both_relays.len()
            ));
            let is_timeline_author = self.timeline_authors.contains(&event.pubkey);
            // Capture created_at before `relay_list` moves into the map.
            let nip65_created_at = relay_list.created_at;
            self.author_relay_lists.insert(event.pubkey.clone(), relay_list);
            // A1: a kind:10002 replaced an author's mailbox. Fan a
            // Nip65Arrived trigger into the lifecycle inbox so the
            // subscription compiler recompiles on the next tick — the
            // author now routes via their declared NIP-65 write relays
            // instead of the indexer-discovery probe. Mirrors the A11
            // FollowListChanged pattern in `ingest/contacts.rs`. Per D8,
            // multiple kind:10002 arrivals within one tick collapse to a
            // single compile pass. This closes the auto-probe round-trip:
            // recompile emits the kinds:[10002] discovery REQ → relay
            // answers → ingest_relay_list lands it here → this trigger
            // re-plans the author onto their resolved write relays.
            self.lifecycle
                .enqueue_trigger(CompileTrigger::Nip65Arrived {
                    pubkey: event.pubkey.clone(),
                    created_at: nip65_created_at,
                });
            // T105 / A1 recompilation trigger: a kind:10002 arrived for an
            // author already in the follow-feed. The current `seed-timeline-*`
            // subs are partitioned by the previous (likely bootstrap) routing
            // — re-plan so the next emission picks up the resolved write
            // relays. Mark the timeline as not-yet-requested so the next
            // `maybe_open_timeline` re-fans out the partition; the existing
            // seed-timeline subs stay live until the actor's CLOSE goes out
            // on the next pending_view drain (the new subs use distinct
            // urlhash sub-ids, so they don't collide).
            if is_timeline_author && self.timeline_requested {
                self.timeline_requested = false;
                self.log(format!(
                    "NIP-65 arrival → re-plan timeline for {}",
                    short_hex(&event.pubkey)
                ));
                // Close the prior bootstrap-routed seed-timeline subs so
                // they're not double-billed against the new per-relay subs.
                // The actual CLOSE frames are queued via `pending_view_requests`
                // → `defer_outbound`; the next emission carries both the new
                // resolved-relay REQs and the explicit CLOSEs of the old subs.
                let closes = self.close_subscriptions_with_prefixes(&["seed-timeline-"]);
                for close in closes {
                    self.defer_outbound(close);
                }
            }
        }
    }
}
