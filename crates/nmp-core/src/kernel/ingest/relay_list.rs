//! Kind:10002 (NIP-65 relay list) ingest.

use super::super::*;

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
                    || (relay_list.created_at == current.created_at
                        && event.id < current.event_id)
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
            self.author_relay_lists.insert(event.pubkey, relay_list);
        }
    }
}
