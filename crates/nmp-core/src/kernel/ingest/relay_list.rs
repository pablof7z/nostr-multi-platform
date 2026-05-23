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
            let had_entry = self.author_relay_lists.remove(&event.pubkey).is_some();
            // T140 (codex finding #5): clearing the mailbox cache without a
            // recompile left existing M2 plans routed to the now-stale relays.
            // Fan a `Nip65Arrived` so the next `drain_tick` re-plans this
            // author off the cleared relays (the planner falls back to the
            // bootstrap discovery seed when no mailbox is cached). Only when
            // an entry actually existed — an empty event for an
            // already-unknown author is a true no-op (no stale plan to fix).
            if had_entry {
                self.lifecycle
                    .enqueue_trigger(CompileTrigger::Nip65Arrived {
                        pubkey: event.pubkey.clone(),
                        created_at: relay_list.created_at,
                    });
            }
            return;
        }

        // This function is only called after verify_and_persist returned
        // Inserted | Replaced, so the store already enforced strict `>` with
        // lexicographic event-id tiebreak. The local cache guard below is a
        // belt-and-suspenders check that mirrors the store's supersession
        // logic exactly (strict `>` on timestamp; same-ts resolved by
        // lexicographically smaller event id wins).
        let should_replace =
            self.author_relay_lists
                .get(&event.pubkey)
                .map_or(true, |current| {
                    relay_list.created_at > current.created_at
                        || (relay_list.created_at == current.created_at
                            && event.id < current.event_id)
                });
        if should_replace {
            self.log(format!(
                "NIP-65 {} read={} write={} both={}",
                short_hex(&event.pubkey),
                relay_list.read_relays.len(),
                relay_list.write_relays.len(),
                relay_list.both_relays.len()
            ));
            // Capture created_at before `relay_list` moves into the map.
            let nip65_created_at = relay_list.created_at;
            self.author_relay_lists.insert(event.pubkey.clone(), relay_list);
            // A1: a kind:10002 replaced an author's mailbox. Fan a
            // Nip65Arrived trigger into the lifecycle inbox so the M2
            // subscription compiler recompiles on the next tick — the
            // author now routes via their declared NIP-65 write relays
            // instead of the indexer-discovery probe. Per D8, multiple
            // kind:10002 arrivals within one tick collapse to a single
            // compile pass. This closes the auto-probe round-trip:
            // recompile emits the kinds:[10002] discovery REQ → relay
            // answers → ingest_relay_list lands it here → this trigger
            // re-plans the author onto their resolved write relays.
            // T140: the M1 seed-timeline-* workaround (flip timeline_requested,
            // CLOSE old subs) is retired — drain_lifecycle_tick handles
            // re-routing via the Nip65Arrived trigger above.
            self.lifecycle
                .enqueue_trigger(CompileTrigger::Nip65Arrived {
                    pubkey: event.pubkey.clone(),
                    created_at: nip65_created_at,
                });
        }
    }
}
