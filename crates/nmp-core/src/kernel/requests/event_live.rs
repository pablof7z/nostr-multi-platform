//! Event-claim resolution support: the `Live`/Tailing addressable-event slot
//! (ADR-0063 #1671 Lane B), the read-cache lookup, and the cold-start parking
//! drain.
//!
//! Split out of `requests/event.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). All bodies are `impl Kernel`; no new state lives here.

use super::super::{Instant, Kernel, OutboundMessage};
use super::event_key::external_id_from_key;
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, RelayHint,
};
use crate::subs::{CompileTrigger, SubIdentity, SubKey, SubOwnerKey, SubScope};

/// Stable `SubKey` for an event-claim tailing slot — one slot per claimed
/// `primary_id`, so every `Live` consumer of the same coordinate dedups onto a
/// single wire REQ (the event twin of `profile_claim_sub_key`). Used by both the
/// per-consumer `release_event_claim_interest` and the unified
/// `teardown_event_claim_key` cleanup path.
fn event_claim_sub_key(primary_id: &str) -> SubKey {
    SubKey::new(("event-claim", primary_id))
}

fn event_claim_identity(primary_id: &str, consumer_id: &str) -> SubIdentity {
    let owner = SubOwnerKey::new(("event-claim-owner", consumer_id));
    SubIdentity::new(owner, event_claim_sub_key(primary_id), SubScope::Global)
}

impl Kernel {
    /// ADR-0063 — register or upgrade the deduped *tailing* interest for a
    /// `Live` claim on an addressable (naddr) coordinate. Mirrors
    /// [`Kernel::register_profile_claim_interest`]: one
    /// `(SubScope::Global, event-claim:<primary_id>)` slot per coordinate; each
    /// consumer attaches as a distinct `SubOwnerKey`; the front-door `Replace`
    /// policy installs the `Tailing` lifecycle and fires store-serve + recompile.
    ///
    /// `filter` is the addressable REQ filter already built by `resolve_event_ref`
    /// (`{kinds, authors, #d, limit:1}`); `hints` are the URI's NIP-19 relay TLVs.
    pub(in crate::kernel) fn register_event_claim_interest(
        &mut self,
        primary_id: &str,
        consumer_id: &str,
        filter: InterestShape,
        hints: Vec<RelayHint>,
    ) {
        let key = event_claim_sub_key(primary_id);
        let identity = event_claim_identity(primary_id, consumer_id);

        // Tailing wins: track THIS consumer's live owner per-coordinate so the
        // slot's owner is detached on each live release (BLOCKING 1). The slot
        // stays `Tailing` until the last live owner releases (in
        // `release_event_claim_interest`).
        self.live_event_claims
            .entry(primary_id.to_string())
            .or_default()
            .insert(consumer_id.to_string());

        let interest = LogicalInterest {
            id: InterestId(key.0),
            scope: InterestScope::Global,
            shape: filter,
            hints,
            lifecycle: InterestLifecycle::Tailing,
            // Author is known (naddr carries the pubkey), but opt into the
            // bootstrap-indexer fallback for parity with the profile claim path.
            is_indexer_discovery: true,
        };

        self.register_interest(
            &[crate::kernel::cache_serve::InterestRegistration {
                identity,
                interest,
                policy: crate::kernel::cache_serve::InterestWrite::Replace,
            }],
            "event-claim-live",
        );
    }

    /// ADR-0063 (BLOCKING 1) — detach THIS consumer's `Live` tailing owner for
    /// `primary_id`, tearing the deduped slot down when the LAST live owner
    /// leaves. Called on EVERY event release (per-consumer lifecycle), not only
    /// on total teardown: that is what stops the first of two `Live` consumers —
    /// or a `Live` consumer released ahead of a surviving `CacheOk` consumer —
    /// from leaking its registry owner and the tailing sub. No-op when this
    /// consumer never held a live owner (CacheOk-only), or the coordinate never
    /// had a `Live` claim (immutable nevent/note ids). Mirrors the `drop_owner` +
    /// recompile tail of `release_profile_ref`.
    pub(in crate::kernel) fn release_event_claim_interest(
        &mut self,
        primary_id: &str,
        consumer_id: &str,
    ) {
        // Only this consumer's owner is detached, and only if it registered one.
        let last_live_owner = match self.live_event_claims.get_mut(primary_id) {
            Some(owners) => {
                if !owners.remove(consumer_id) {
                    // CacheOk-only consumer (no live owner) — nothing to detach,
                    // and other live owners (if any) keep the slot alive.
                    return;
                }
                owners.is_empty()
            }
            None => return, // coordinate never had a Live claim
        };
        let identity = event_claim_identity(primary_id, consumer_id);
        let slot_removed = self.lifecycle.registry_mut().drop_owner(&identity);
        if last_live_owner {
            // Last live owner gone: drop the per-coordinate live-owner record so a
            // future `Live` claim starts fresh.
            self.live_event_claims.remove(primary_id);
        }
        if slot_removed {
            // The CLOSE diff only materialises when the planner recompiles.
            self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened {
                interest_ids: Vec::new(),
            });
        }
    }

    /// ADR-0063 (BLOCKING 3) — retire a `CacheOk` one-shot interest for
    /// `primary_id` when a `Live` claim upgrades the key to a tailing slot, so
    /// exactly ONE interest / wire REQ exists per key (Live wins). The profile
    /// path shares ONE slot across liveness levels via in-place `set_sub`; events
    /// keep their bespoke `OneshotApi` cold-fetch (the Phase 1/2/3 NIP-19 retarget
    /// machinery), so the upgrade explicitly retires the one-shot rather than
    /// mutating it in place. No-op when no one-shot is pending for the key
    /// (`Live`-first, an already-cached coordinate, or already retired).
    pub(in crate::kernel) fn cancel_event_oneshot(&mut self, primary_id: &str) {
        // The claim-expansion tracker records the one-shot's `InterestId` keyed by
        // `primary_id`; that id IS the registry `SubKey` (`InterestId(key.0)`).
        let Some(interest_id) = self
            .pending_claims
            .values()
            .find(|c| c.primary_id == primary_id)
            .map(|c| c.interest_id.clone())
        else {
            return;
        };
        // Release the OneshotApi token bookkeeping (else it lingers until EOSE).
        if let Some(token) = self.pending_discovery_oneshots.remove(&interest_id) {
            let registry = self.lifecycle.registry_mut();
            self.oneshot.release(registry, token);
        }
        // Drop the Phase 1/2/3 retarget tracker for the retired one-shot.
        self.release_claim_expansion(primary_id);
        self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened {
            interest_ids: Vec::new(),
        });
    }

    /// Is the event identified by `primary_id` already in the kernel's
    /// read-cache? Hex64 keys look up `events` directly; coordinate
    /// keys (`kind:pubkey:d_tag`) scan `events.values()` for the
    /// matching addressable triple.
    ///
    /// Used by the event resolver to short-circuit the OneshotApi
    /// registration when no fetch is needed. The store-side equivalent
    /// is the snapshot projection in `kernel/update.rs::lookup_for_primary_id`
    /// which performs the same lookup against the same map.
    pub(in crate::kernel) fn event_already_known(&self, primary_id: &str) -> bool {
        if is_hex64(primary_id) {
            return self.events.contains_key(primary_id);
        }
        // NIP-73 external ref `i:<external-id>` (#1654): cached iff any event in
        // the in-memory timeline carries a matching `["i", <external-id>]` tag.
        // The store-side equivalent lives in `views.rs::lookup_for_primary_id`.
        if let Some(external_id) = external_id_from_key(primary_id) {
            return self.events.values().any(|e| {
                e.tags
                    .iter()
                    .any(|t| t.len() >= 2 && t[0] == "i" && t[1] == external_id)
            });
        }
        // d-tags can legally contain `:` (rare but spec-allowed); split
        // only on the first two colons so `kind:author:foo:bar` round-
        // trips correctly.
        let mut parts = primary_id.splitn(3, ':');
        let kind = parts.next().and_then(|s| s.parse::<u32>().ok());
        let pubkey = parts.next();
        let d_tag = parts.next();
        let (Some(kind), Some(pubkey), Some(d_tag)) = (kind, pubkey, d_tag) else {
            return false;
        };
        self.events.values().any(|e| {
            e.kind == kind
                && e.author == pubkey
                && e.tags
                    .iter()
                    .any(|t| t.len() >= 2 && t[0] == "d" && t[1] == d_tag)
        })
    }

    /// Drain the cold-start parking queue. Called from `pending_view_requests`
    /// once at least one relay is connected (`can_send = true`). Mirrors
    /// `pending_profile_claim_requests` semantics: replays each parked CANONICAL
    /// [`PendingEventClaim`] target through the raw resolver body, so a raw key
    /// parked while cold actually resolves; the body skips any that are already
    /// resolved or already in-flight.
    ///
    /// Defensive (D4 / lifecycle): a parked claim whose consumer has since
    /// RELEASED its ref (`event_claims[primary_id]` no longer holds it) must NOT
    /// be replayed — replaying would resurrect a released claim with a fresh rev.
    /// The primary fix removes the parked stake on release (`release_event_ref` →
    /// `remove_parked_event_claim`); this filter is a belt-and-suspenders guard so
    /// the drain can never resurrect a key whose refcount row is gone.
    #[cfg(test)]
    pub(crate) fn pending_event_claim_requests(&mut self) -> Vec<OutboundMessage> {
        self.pending_event_claim_requests_at(crate::kernel::test_support::test_support_now())
    }

    pub(crate) fn pending_event_claim_requests_at(&mut self, now: Instant) -> Vec<OutboundMessage> {
        if self.pending_event_claims.is_empty() {
            return Vec::new();
        }
        let parked = std::mem::take(&mut self.pending_event_claims);
        let mut out = Vec::new();
        for claim in parked {
            // Defensive (B): skip a parked claim whose consumer no longer holds a
            // live refcount on the resolved key — it was released before the drain
            // ran. `release_event_ref` already removes the parked stake on release
            // (`remove_parked_event_claim`); this is the belt-and-suspenders guard
            // so the drain can never resurrect a key whose refcount row is gone.
            if let Some(primary_id) = parked_event_primary_id(&claim.key) {
                let still_claimed = self
                    .event_claims
                    .get(&primary_id)
                    .is_some_and(|consumers| consumers.contains(&claim.consumer_id));
                if !still_claimed {
                    continue;
                }
            }
            // Replay the parked target's OWN shape / liveness / force /
            // URI-decoded metadata through the canonical raw body (D) —
            // `can_send = true` now that a relay is connected. (The parked
            // `force` is preserved verbatim so the original caller's freshness
            // intent survives the cold-start delay.)
            out.extend(self.resolve_event_ref_inner(
                claim.key,
                claim.consumer_id,
                claim.shape,
                claim.liveness,
                claim.force,
                true,
                claim.event_author,
                claim.relay_hints,
                now,
            ));
        }
        out
    }

    /// ADR-0063 (#1671 Lane B) — drop `consumer_id`'s COLD-PARK stake for
    /// `primary_id` from `pending_event_claims`, the queue the relay-ready drain
    /// replays. Called from the release path BEFORE `teardown_event_claim_key`:
    /// the unified teardown only cleans the LIVE per-key maps (`event_claims`,
    /// `ref_event_shapes`, the `Live` slot, the rev), so without this a claim
    /// that PARKED hintless (no relay connected) and was then RELEASED before the
    /// drain ran would be resurrected with a fresh rev when
    /// `pending_event_claim_requests` later replays the stale pair — a cleared+
    /// removed key with a still-parked reference (D4 / lifecycle violation).
    ///
    /// Refcount/owner semantics mirror `event_claims`: only THIS consumer's
    /// parked stake for the key is removed; any parked entry belonging to another
    /// consumer (or to a different key whose URI happens to share a prefix) is
    /// left intact, so a sibling parked claim still drains on relay-ready.
    pub(in crate::kernel) fn remove_parked_event_claim(
        &mut self,
        primary_id: &str,
        consumer_id: &str,
    ) {
        self.pending_event_claims.retain(|claim| {
            if claim.consumer_id != consumer_id {
                return true;
            }
            // Keep entries whose raw key resolves to a DIFFERENT primary_id. A
            // malformed parked key can never have produced this `primary_id`, so
            // keep it.
            parked_event_primary_id(&claim.key).as_deref() != Some(primary_id)
        });
    }

    /// ADR-0063 (#1671 Lane B, BLOCKING 1 + 2) — the SINGLE internal teardown for a
    /// fully-released `event` ref key. Called from BOTH `release_event_ref` (last
    /// consumer gone) and the controller's terminal-miss path
    /// (`terminate_claim`, `Exhausted` / `Budget`: no relay holds the event). It
    /// drops every piece of per-key state in one place so the two paths can never
    /// diverge and leave a deleted claim's shape map or rev row live (D4: one
    /// writer; a stale `ref_event_shapes` / `event_row_revs` entry would resurrect
    /// a ghost ref row).
    ///
    /// Teardown order (BLOCKING 2 ordering, all in this one call):
    /// 1. drop the refcount row (`event_claims`) + the cold-fetch dedup marker
    ///    (`event_claim_requested`),
    /// 2. drop the per-consumer demanded-shape map for the key (D5),
    /// 3. drop the `Live` tailing registry slot + the per-coordinate live-owner
    ///    record (idempotent — the per-consumer release may already have done this),
    /// 4. release the W5 claim-expansion retarget tracker and its one-shot
    ///    registry owner,
    /// 5. stamp the projection (`changed_since_emit`, `claimed_event_content_ver`),
    /// 6. `clear_event_row`: bump the per-key rev to its final post-clear value
    ///    (the value an ADR-0055 `Cleared` row would carry) and IMMEDIATELY remove
    ///    the entry in the same call — no retained-rev / pending state, so the map
    ///    stays bounded to live keys (D8). The downstream row-delta emitter
    ///    (Lane A) is out of this branch's scope, so the returned final rev is
    ///    discarded here.
    pub(in crate::kernel) fn teardown_event_claim_key(&mut self, primary_id: &str) {
        let had_claim = self.event_claims.remove(primary_id).is_some();
        // Allow a re-claim to re-register interest with the OneshotApi (a stale
        // `event_claim_requested` entry would otherwise short-circuit the next
        // cold-claim).
        self.event_claim_requested.remove(primary_id);
        // D5 — the key is gone; drop its whole per-consumer demanded-shape map.
        self.ref_event_shapes.remove(primary_id);
        // BLOCKING 1 — drop the `Live` tailing slot for ANY remaining live owners
        // (the terminal-miss path never ran per-consumer releases, so there may be
        // several). Idempotent when the per-consumer release already cleared it.
        if let Some(owners) = self.live_event_claims.remove(primary_id) {
            for consumer_id in owners {
                let identity = event_claim_identity(primary_id, &consumer_id);
                let _ = self.lifecycle.registry_mut().drop_owner(&identity);
            }
            self.lifecycle
                .enqueue_trigger(crate::subs::CompileTrigger::ViewOpened {
                    interest_ids: Vec::new(),
                });
        }
        // The last consumer / terminal-miss released, so cancel the W5
        // claim-expansion retargeting tracker for this id (keeps Phase 1/2 hint
        // retargeting from outliving an event nobody wants).
        self.release_claim_expansion(primary_id);

        self.changed_since_emit = true;
        // ADR-0055 Rung 1: bump claimed_event_content_ver (codex #1 condition 1).
        self.projection_rev_tracker
            .source_versions
            .bump_claimed_event_content();
        // BLOCKING 2 — bump-to-final then drop the per-key rev in the SAME call.
        // No-op (returns 0) when the key had no rev entry (terminal-miss of a key
        // whose resolve was a no-op), so a teardown never creates a ghost row.
        let _final_rev = self
            .projection_rev_tracker
            .source_versions
            .clear_event_row(primary_id);
        let _ = had_claim;
    }
}

/// Map a parked claim's CANONICAL raw `key` (hex64 event-id or `kind:pubkey:d`
/// coordinate) back to the `primary_id` the resolver computed for it. Returns
/// `None` for a malformed key — it can never have produced a live claim row, so
/// callers leave such entries untouched. Delegates to the canonical
/// `parse_event_key` parser (single path) so the derivation MUST match
/// `resolve_event_ref` / `release_event_ref` exactly and a release reliably
/// finds the stake its claim parked.
fn parked_event_primary_id(key: &str) -> Option<String> {
    super::event_key::parse_event_key(key).map(|t| t.primary_id)
}

/// `true` when `s` is exactly 64 lowercase hex chars (a canonical event-id).
/// Coordinate-form `primary_id` strings never match. Delegates to the canonical
/// strict check in `event_key` (single path — no duplicate hex predicate).
fn is_hex64(s: &str) -> bool {
    super::event_key::is_lower_hex64(s)
}
