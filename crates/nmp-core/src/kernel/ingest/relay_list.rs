//! Kind:10002 (NIP-65 relay list) ingest.

use super::super::mailboxes::BootstrapSeed;
use super::super::{parse_relay_list_to_substrate, Kernel, NostrEvent, short_hex};
use crate::stable_hash::stable_hash64;
use crate::subs::CompileTrigger;

impl Kernel {
    /// Ingest a kind:10002 NIP-65 relay-list event into the substrate
    /// [`crate::substrate::MailboxCache`] (step 3 of
    /// `docs/architecture/crate-boundaries.md` §3).
    ///
    /// Only called after `verify_and_persist` returns `Inserted | Replaced`
    /// (D4). The store has already enforced kind:10002 supersession
    /// (strict `>` on `created_at` with lexicographic event-id tiebreak),
    /// so no kernel-side guard is needed — step 3 collapses the
    /// pre-step-3 "belt-and-suspenders" mirror to a single source of
    /// truth per the planning-discipline rule (`AGENTS.md`: "single
    /// source of truth per fact"). If a future caller bypasses
    /// `verify_and_persist`, the call-site path — not this function — is
    /// what needs hardening.
    ///
    /// ## Empty-list semantics
    ///
    /// If the canonical kind:10002 carries an **empty** relay list (all
    /// three buckets are empty), the author has explicitly cleared their
    /// NIP-65 metadata. The cache entry is *removed* rather than left
    /// stale, and a `Nip65Arrived` trigger is fanned so the next
    /// `drain_tick` re-plans the author off the cleared relays (the
    /// router falls back to AppRelays / bootstrap discovery seed when
    /// no mailbox is cached). Only when an entry actually existed — an
    /// empty event for an already-unknown author is a true no-op (no
    /// stale plan to fix).
    pub(in crate::kernel) fn ingest_relay_list(&mut self, event: NostrEvent) {
        let parsed = parse_relay_list_to_substrate(&event.id, event.created_at, &event.tags);

        if parsed.read.is_empty() && parsed.write.is_empty() && parsed.both.is_empty() {
            // Empty relay list from a canonical newer event: author cleared NIP-65.
            // Remove the stale cache entry so it does not outlive the author's intent.
            let had_entry = self.mailbox_cache.known(&event.pubkey);
            self.mailbox_cache.remove(&event.pubkey);
            // T140 (codex finding #5): clearing the mailbox cache without
            // a recompile left existing M2 plans routed to the now-stale
            // relays. Fan a `Nip65Arrived` so the next `drain_tick`
            // re-plans this author off the cleared relays. Only when an
            // entry actually existed — an empty event for an
            // already-unknown author is a true no-op (no stale plan to
            // fix).
            if had_entry {
                self.lifecycle
                    .enqueue_trigger(CompileTrigger::Nip65Arrived {
                        pubkey: event.pubkey.clone(),
                        created_at: event.created_at,
                    });
            }
            return;
        }

        self.log(format!(
            "NIP-65 {} read={} write={} both={}",
            short_hex(&event.pubkey),
            parsed.read.len(),
            parsed.write.len(),
            parsed.both.len()
        ));
        let created_at = event.created_at;
        let pubkey = event.pubkey.clone();
        self.mailbox_cache.upsert(event.pubkey.clone(), parsed);
        // Debt A — fire the injected `OutboxRouter` immediately after the
        // cache update, BEFORE the recompile trigger. The router now sees
        // the freshly-landed NIP-65 read set and the trace observer
        // records a routing decision attributed to lane 1 (`Nip65/Read`)
        // — exactly what the chirp-repl `routing-trace` smoke and the
        // V-51 phase 4 real-relay validation harness assert. Without
        // this call the projection would only carry the pre-cache-update
        // lane-7 (`AppRelay/Fallback`) observations from
        // `author_requests` / `profile_claim_request` and the lane-attribution
        // assertion would fail. The synthetic interest mirrors the
        // per-author timeline subscription shape `author_requests`
        // builds; the returned URL set is discarded (we only need the
        // trace fire — the cache mutation above is the canonical state
        // change, and any future REQ for this author routes through
        // the same router slot).
        let _ = self.route_subscription_relays(
            stable_hash64(("nip65-arrived", event.id.as_str(), created_at)),
            &[pubkey.as_str()],
            &[1, 6],
            BootstrapSeed::Discovery,
        );
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
        self.lifecycle.enqueue_trigger(CompileTrigger::Nip65Arrived {
            pubkey: event.pubkey,
            created_at,
        });
    }
}
