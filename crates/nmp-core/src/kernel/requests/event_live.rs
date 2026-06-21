//! Event-claim resolution support: the `Live`/Tailing addressable-event slot
//! (ADR-0063 #1671 Lane B), the read-cache lookup, and the cold-start parking
//! drain.
//!
//! Split out of `requests/event.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). All bodies are `impl Kernel`; no new state lives here.

use super::super::{Kernel, OutboundMessage};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, RelayHint,
};
use crate::subs::{CompileTrigger, SubIdentity, SubKey, SubOwnerKey, SubScope};

/// Stable `SubKey` for an event-claim tailing slot — one slot per claimed
/// `primary_id`, so every `Live` consumer of the same coordinate dedups onto a
/// single wire REQ (the event twin of `profile_claim_sub_key`).
fn event_claim_sub_key(primary_id: &str) -> SubKey {
    SubKey::new(("event-claim", primary_id))
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
        let owner = SubOwnerKey::new(("event-claim-owner", consumer_id));
        let identity = SubIdentity::new(owner, key, SubScope::Global);

        // Tailing wins: once any `Live` claim is seen for this coordinate, the
        // slot stays `Tailing` until the last owner releases (cleared in
        // `release_event_claim_interest`).
        self.live_event_claims.insert(primary_id.to_string());

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

    /// ADR-0063 — tear down the `Live` tailing slot for `primary_id` when its
    /// last consumer releases. No-op when the coordinate never had a `Live`
    /// claim (immutable nevent/note ids, or CacheOk-only addressable claims).
    /// Mirrors the `drop_owner` + recompile tail of `release_profile_ref`.
    pub(in crate::kernel) fn release_event_claim_interest(
        &mut self,
        primary_id: &str,
        consumer_id: &str,
    ) {
        if !self.live_event_claims.remove(primary_id) {
            return;
        }
        let key = event_claim_sub_key(primary_id);
        let owner = SubOwnerKey::new(("event-claim-owner", consumer_id));
        let identity = SubIdentity::new(owner, key, SubScope::Global);
        let slot_removed = self.lifecycle.registry_mut().drop_owner(&identity);
        if slot_removed {
            // The CLOSE diff only materialises when the planner recompiles.
            self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened {
                interest_ids: Vec::new(),
            });
        }
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
    /// `pending_profile_claim_requests` semantics: processes each parked
    /// `(uri, consumer_id)` pair as a warm claim, skipping any that are
    /// already resolved or already in-flight.
    pub(crate) fn pending_event_claim_requests(&mut self) -> Vec<OutboundMessage> {
        if self.pending_event_claims.is_empty() {
            return Vec::new();
        }
        let parked: Vec<(String, String)> = std::mem::take(&mut self.pending_event_claims);
        let mut out = Vec::new();
        for (uri, consumer_id) in parked {
            // Cold-start replay is the gated path (`force = false`): a parked
            // claim is for an as-yet-unknown event, so it cold-fetches fresh
            // on replay regardless — force only matters for an already-cached
            // replaceable identity (the user-navigation refresh case).
            out.extend(self.claim_event(uri, consumer_id, true, false));
        }
        out
    }
}

/// `true` when `s` is exactly 64 lowercase hex chars (a canonical
/// event-id). Coordinate-form `primary_id` strings never match.
fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
