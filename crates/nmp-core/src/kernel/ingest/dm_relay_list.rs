//! Kind:10050 (NIP-17 DM-relay list) ingest.
//!
//! NIP-17 § 2: a kind:1059 gift-wrap MUST be published to the recipient's
//! kind:10050 DM-relay list — a relay set deliberately distinct from the
//! kind:10002 (NIP-65) generic mailbox. kind:10050 carries `["relay", <url>]`
//! tags (note: the `relay` marker, NOT the `r` marker NIP-65 uses), letting a
//! user route private messages to a privacy-focused relay that is not in their
//! public read set. Collapsing the two would silently leak DM routing onto
//! public relays.
//!
//! This module is the structural sibling of `relay_list.rs` (kind:10002): it
//! parses a canonical kind:10050 event into `kernel.dm_relay_lists` keyed by the
//! event author's pubkey. The read side is `Kernel::recipient_dm_relays`, which
//! the NIP-17 DM send path (`actor::commands::dm`) consults to pin each
//! kind:1059 envelope to its receiver's DM-inbox relays. The subscription
//! compiler also reads the same cache for active gift-wrap inbox interests, so
//! a kind:10050 change fans a recompile trigger.

use super::super::*;
use crate::subs::CompileTrigger;

/// Parse the `["relay", <url>]` tags of a kind:10050 event into a deduped,
/// canonicalized list of DM-inbox relay URLs.
///
/// kind:10050 has no read/write/both markers (unlike NIP-65 kind:10002) — every
/// `relay` tag is a DM-inbox relay. URLs are canonicalized with
/// [`CanonicalRelayUrl::parse_or_raw`] (lowercase scheme+host, empty-path
/// trailing slash stripped) so the cache keys match the wire-routing form, and
/// a `HashSet` drops duplicate tags while preserving first-seen tag order.
///
/// Non-`relay` tags, `relay` tags with no URL value, and URLs that do not start
/// with `wss://` are skipped — mirroring `parse_relay_list`'s defensive scheme
/// gate for kind:10002.
fn parse_dm_relay_list(tags: &[Vec<String>]) -> Vec<String> {
    let mut relays = Vec::new();
    let mut seen = HashSet::new();

    for tag in tags {
        if tag.first().map(String::as_str) != Some("relay") {
            continue;
        }
        let Some(url) = tag.get(1).filter(|url| url.starts_with("wss://")) else {
            continue;
        };
        let canonical = CanonicalRelayUrl::parse_or_raw(url).into_string();
        if seen.insert(canonical.clone()) {
            relays.push(canonical);
        }
    }

    relays
}

impl Kernel {
    /// Ingest a kind:10050 NIP-17 DM-relay-list event into `dm_relay_lists`.
    ///
    /// Only called after `verify_and_persist` returns `Inserted | Replaced`
    /// (D4) — the store has already enforced replaceable-event supersession
    /// (strict `>` on `created_at`, lexicographic event-id tiebreak), so the
    /// `dm_relay_lists` cache always reflects the canonical kind:10050 for the
    /// author. No created_at guard is kept on the cache itself: the store is the
    /// supersession authority and this handler runs only on the winning event.
    ///
    /// ## Empty-list semantics
    ///
    /// If the canonical kind:10050 carries no `relay` tags, the author has
    /// explicitly cleared their DM-relay list. The existing cache entry is
    /// *removed* rather than left stale — an empty-but-canonical event must not
    /// let an old list persist. With no entry, `recipient_dm_relays` returns
    /// `None` and the DM send path fails closed, exactly as for an author who
    /// never published a kind:10050.
    ///
    /// Unlike kind:10002, kind:10050 never populates generic mailbox routing.
    /// It does now feed the planner through the NIP-17 DM-specific p-tag
    /// routing mode, so accepted changes enqueue a recompile trigger. A DM
    /// gift-wrap inbox with no kind:10050 stays fail-closed rather than falling
    /// back to public kind:10002 read relays.
    pub(in crate::kernel) fn ingest_dm_relay_list(&mut self, event: NostrEvent) {
        let relays = parse_dm_relay_list(&event.tags);

        // Empty DM-relay list from a canonical newer event: the author cleared
        // their kind:10050. Drop the stale cache entry so it does not outlive
        // the author's intent.
        if relays.is_empty() {
            if self.dm_relay_lists.remove(&event.pubkey).is_some() {
                self.lifecycle
                    .enqueue_trigger(CompileTrigger::DmRelayListChanged {
                        pubkey: event.pubkey.clone(),
                        created_at: event.created_at,
                    });
            }
            return;
        }

        self.log(format!(
            "NIP-17 DM relays {} count={}",
            short_hex(&event.pubkey),
            relays.len()
        ));
        self.dm_relay_lists.insert(event.pubkey.clone(), relays);
        self.lifecycle
            .enqueue_trigger(CompileTrigger::DmRelayListChanged {
                pubkey: event.pubkey.clone(),
                created_at: event.created_at,
            });
    }
}
