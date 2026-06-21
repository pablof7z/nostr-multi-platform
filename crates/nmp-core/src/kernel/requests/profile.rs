//! Profile (kind:0) claim/release — registry-backed (M2 migration).
//!
//! # M2 migration (compiler.md §3.5) — DONE
//!
//! `claim_profile` / `release_profile` no longer build kind:0 REQ frames
//! directly. Each claim registers a `LogicalInterest { kinds:[0],
//! authors:[P], limit:None }` through the [`crate::subs::InterestRegistry`]
//! (the same single-writer chokepoint the follow feed and `claim_event` use),
//! so a claimed profile inherits, for free:
//!
//! * **D3 implicit kind:10002 discovery** — `recompile_and_diff` auto-emits a
//!   batched `kinds:[10002]` probe for any author with no cached mailbox
//!   (`subs/recompile.rs`); when the relay-list arrives it fires
//!   `Nip65Arrived` and the next recompile re-routes the kind:0 onto the
//!   author's own write relays. This is what makes a STRANGER's profile
//!   (not in the follow set) resolve — the pre-migration bespoke path fetched
//!   kind:0 from the indexers only and never probed kind:10002, so a stranger
//!   whose kind:0 lived only on their own relays never resolved.
//! * **greedy set-cover relay selection** (`apply_selection_with_lookup`) —
//!   hundreds of avatar claims collapse onto a bounded relay set.
//! * **author-union coalescing** — `limit: None` lets same-shape kind:0
//!   interests merge into ONE batched `authors:[…]` REQ per relay (the merge
//!   lattice refuses to coalesce shapes carrying a `limit`). kind:0 is
//!   replaceable, so the missing explicit `limit` is harmless (one event per
//!   author maximum).
//! * **`interest.hints`** — nprofile/nevent NIP-19 relay TLVs seed the claim
//!   so a stranger whose kind:10002 is on no indexer still resolves from the
//!   embedded hint relay (parity with `claim_event`).
//!
//! Deleted alongside this migration: `profile_claim_request`,
//! `pending_profile_claim_requests`, the `ProfileRequestState` machine, and
//! `refresh_profile_after_mailbox` (the `Nip65Arrived` recompile replaces the
//! requested→pending re-queue). The obsolete "kind:0 must not leak onto the
//! content/app relay — indexer-only" contract is retired too: kind:0 claims
//! now use the compiler's default generic routing (author write relays when
//! warm; app relays + indexer when uncached), which is the intended behaviour.
//!
//! # Liveness (client-hintable freshness)
//!
//! The claim seam carries a [`ProfileLiveness`] hint:
//!
//! * [`ProfileLiveness::CacheOk`] → `OneShot` — serve from cache; on a miss a
//!   single kind:0 fetch that closes on EOSE. No live sub. Feed avatars.
//! * [`ProfileLiveness::Live`] → `Tailing` — the kind:0 sub stays open while
//!   claimed so profile edits (kind:0 replacements) arrive reactively. The
//!   profile screen.
//!
//! Mixed liveness on one pubkey resolves to **Tailing wins**: a `Live` claim
//! upgrades an existing `CacheOk` slot in place (`set_sub`), and the slot
//! stays `Tailing` until the last owner releases (downgrade only on full
//! teardown). Both liveness levels share ONE `(scope, key)` slot so they
//! dedup to a single wire REQ.
//!
//! `profile_claims` (the `HashMap<pubkey, BTreeSet<consumer_id>>` refcount)
//! is RETAINED as the `claimed_profiles` projection source-of-truth; the
//! registry interest is driven off it.

use super::super::{short_hex, truncate, Kernel, OutboundMessage};
use crate::kernel::refs::{ProfileShape, RefLiveness, RefNamespace, RefShape};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, RelayHint,
};
use crate::subs::{CompileTrigger, SubIdentity, SubKey, SubOwnerKey, SubScope};

/// Client-hintable freshness for a profile (kind:0) claim.
///
/// Maps to the registered interest's [`InterestLifecycle`]:
/// `CacheOk` → `OneShot` (one fetch, no live sub), `Live` → `Tailing`
/// (stays subscribed while claimed; reactive kind:0 replacements).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileLiveness {
    /// Serve from cache; on a miss, a one-shot kind:0 fetch. No tailing sub.
    /// The default for background / list-row avatar claims.
    CacheOk,
    /// Keep a tailing kind:0 sub alive while claimed so profile edits arrive
    /// reactively. For an open profile screen.
    Live,
}

impl ProfileLiveness {
    /// Decode the FFI `liveness` int (`0 = CacheOk`, anything else = `Live`).
    #[must_use]
    pub fn from_ffi(liveness: i32) -> Self {
        if liveness == 0 {
            Self::CacheOk
        } else {
            Self::Live
        }
    }

    fn lifecycle(self) -> InterestLifecycle {
        match self {
            Self::CacheOk => InterestLifecycle::OneShot,
            Self::Live => InterestLifecycle::Tailing,
        }
    }
}

/// Stable `SubKey` for a profile claim slot — one slot per claimed pubkey, so
/// every consumer (and every liveness level) of the same pubkey dedups onto a
/// single deduped wire REQ.
fn profile_claim_sub_key(pubkey: &str) -> SubKey {
    SubKey::new(("profile-claim", pubkey))
}

/// Stable `InterestId` for a profile claim, derived from the same seed as the
/// `SubKey` so the planner's `WireFrame::Req { interest_id, .. }` correlates
/// back to this slot deterministically across recompiles.
fn profile_claim_interest_id(pubkey: &str) -> InterestId {
    InterestId(profile_claim_sub_key(pubkey).0)
}

impl Kernel {
    // integration-scaffold(#1671 Lane H): delete before final master cut.
    //
    // Thin delegator onto the unified [`Kernel::resolve_ref`] seam so Lanes C/D/E
    // keep compiling on the integration branch. The legacy `claim_profile`
    // surface resolves the full `ProfileCard` (`claimed_profiles` = full card),
    // so it maps to [`ProfileShape::Card`]. `can_send` is ignored (the registry
    // registers immediately; the planner lands the REQ on connect — V-87 #602).
    pub(crate) fn claim_profile(
        &mut self,
        pubkey: String,
        consumer_id: String,
        _can_send: bool,
        force: bool,
        liveness: ProfileLiveness,
    ) -> Vec<OutboundMessage> {
        self.resolve_ref(
            RefNamespace::Profile,
            pubkey,
            consumer_id,
            RefShape::Profile(ProfileShape::Card),
            liveness.into(),
            force,
            Vec::new(),
        )
    }

    // integration-scaffold(#1671 Lane H): delete before final master cut.
    //
    /// nprofile/nevent-originated claim: identical to [`Self::claim_profile`]
    /// but seeds the registered interest's `hints` with the NIP-19 relay TLVs
    /// embedded in the URI, so a stranger whose kind:10002 is on no indexer
    /// still resolves from the embedded relay (parity with `claim_event`).
    #[cfg(test)] // only called from profile_claim_discovery_tests
    pub(crate) fn claim_profile_with_hints(
        &mut self,
        pubkey: String,
        consumer_id: String,
        force: bool,
        liveness: ProfileLiveness,
        relay_hints: Vec<String>,
    ) -> Vec<OutboundMessage> {
        self.resolve_ref(
            RefNamespace::Profile,
            pubkey,
            consumer_id,
            RefShape::Profile(ProfileShape::Card),
            liveness.into(),
            force,
            relay_hints,
        )
    }

    /// The `profile` reference resolver (ADR-0063). Generalizes the former
    /// `claim_profile_inner`: refcount the consumer, register/upgrade the
    /// kernel-owned kind:0 interest, record the widest demanded shape, and bump
    /// the per-key rev. `shape` selects the projected bytes (Lane C) and is
    /// orthogonal to `liveness` (the kind:0 fetch is identical either way).
    pub(in crate::kernel) fn resolve_profile_ref(
        &mut self,
        pubkey: String,
        consumer_id: String,
        shape: ProfileShape,
        liveness: RefLiveness,
        force: bool,
        relay_hints: Vec<String>,
    ) -> Vec<OutboundMessage> {
        // Liveness is the namespace-agnostic axis; the kind:0 routing below is
        // expressed in the profile-specific `ProfileLiveness` it has always used.
        let liveness: ProfileLiveness = liveness.into();

        // T114b — per-pubkey claim consumer-id retention bound. Drop-newest on
        // overflow mirrors the bounded actor channel; the dropped claim is a
        // silent no-op (D6) and bumps `claim_drops_total`.
        let (inserted, refcount) = {
            let consumers = self.profile_claims.entry(pubkey.clone()).or_default();
            if !consumers.contains(&consumer_id)
                && consumers.len() >= super::super::MAX_CLAIMS_PER_PUBKEY
            {
                self.claim_drops_total = self.claim_drops_total.saturating_add(1);
                // hot path
                return Vec::new();
            }
            let inserted = consumers.insert(consumer_id.clone());
            (inserted, consumers.len())
        };
        // BLOCKING 3 — a per-key rev bump (and the `changed_since_emit` flag) must
        // fire ONLY on an actual row mutation: a new consumer, a shape widen, a
        // liveness upgrade, or (at the ingest site) a content change. A duplicate
        // identical (key, consumer, shape, liveness) re-resolve is NOT a mutation
        // and must leave the rev untouched. Capture the widest demanded shape and
        // whether this key already has a Live owner BEFORE recording this claim so
        // the after-state comparison below detects a real widen / upgrade.
        let widest_before = self.ref_demanded_profile_shape(&pubkey);
        let live_before = self.live_profile_claims.contains_key(&pubkey);
        // ADR-0063 D5 — record THIS consumer's demanded shape (per-consumer so a
        // later release recomputes the widest among currently-live consumers —
        // HIGH 4). Only reached for a real claim (overflow returned above), so the
        // map stays bounded to claimed pubkeys. Independent of liveness.
        self.ref_profile_shapes
            .entry(pubkey.clone())
            .or_default()
            .insert(consumer_id.clone(), shape);
        let widest_after = self.ref_demanded_profile_shape(&pubkey);
        let shape_widened = widest_before != widest_after;
        let liveness_upgraded = liveness == ProfileLiveness::Live && !live_before;
        let mutated = inserted || shape_widened || liveness_upgraded;
        if inserted {
            self.log(format!(
                "claim profile {} consumer {} ref {} liveness {:?} hints {}",
                short_hex(&pubkey),
                truncate(&consumer_id, 80),
                refcount,
                liveness,
                relay_hints.len(),
            ));
        }
        // BLOCKING 3 — only stamp the projection when something actually changed.
        // A no-op re-resolve (same key/consumer/shape/liveness) re-asserts nothing,
        // so it must not advance any rev (the host already has the current row).
        if mutated {
            self.changed_since_emit = true;
            // ADR-0055 Rung 1: bump profile_claims_ver. (claimed_profiles projection
            // derives from `profile_claims`, untouched by the registry migration.)
            self.projection_rev_tracker.source_versions.bump_profile_claims();
            // ADR-0063 Lane B (D6a) — bump THIS pubkey's per-key rev (resolve site 1
            // of 3). The row appears / re-asserts; only this key's row need cross FFI.
            self.projection_rev_tracker.source_versions.bump_profile_row(&pubkey);
        }

        // F-TTL — a profile is a kind:0 replaceable identity. When it is already
        // cached the TTL gate decides whether a lazy re-verification REQ is due
        // (`force == false`) or unconditionally enqueues one (`force == true`:
        // the user opened this author's profile screen or pulled to refresh).
        let resident = self.profile_lookup().contains(&pubkey);
        if resident {
            if let Ok(pk) = nostr::PublicKey::from_hex(&pubkey) {
                self.claim_replaceable(0, pk.to_bytes(), None, force);
            }
        }

        // Warm-reclaim invariant: a `CacheOk` claim for an ALREADY-resident
        // profile must NOT register a network-fetching interest — the resident
        // store serves the card and the F-TTL gate above owns re-verification.
        // A `Live` claim still registers a Tailing sub (it wants future kind:0
        // edits even when the current one is resident). When the profile is not
        // resident, both liveness levels register so the cold fetch goes out.
        let want_register = !resident || liveness == ProfileLiveness::Live;
        if want_register {
            let hints: Vec<RelayHint> = relay_hints
                .into_iter()
                .map(|url| RelayHint {
                    url,
                    source: crate::planner::HintSource::UserConfigured,
                })
                .collect();
            self.register_profile_claim_interest(&pubkey, &consumer_id, liveness, hints);
        }
        Vec::new()
    }

    /// Register or upgrade the deduped kind:0 claim interest for `pubkey`.
    ///
    /// One `(SubScope::Global, profile-claim:<pubkey>)` slot per pubkey; each
    /// consumer attaches as a distinct `SubOwnerKey`. Liveness upgrade (Tailing
    /// wins): if any current owner — or this claim — wants `Live`, the slot's
    /// interest is `Tailing`; otherwise `OneShot`. `set_sub` replaces the
    /// interest in place while keeping the existing owner set, so an arriving
    /// `Live` claim upgrades a `CacheOk` slot without losing its refcount.
    fn register_profile_claim_interest(
        &mut self,
        pubkey: &str,
        consumer_id: &str,
        liveness: ProfileLiveness,
        hints: Vec<RelayHint>,
    ) {
        let key = profile_claim_sub_key(pubkey);
        let scope = SubScope::Global;
        let owner = SubOwnerKey::new(("profile-claim-owner", consumer_id));
        let identity = SubIdentity::new(owner, key, scope.clone());

        // Tailing wins: keep the slot Tailing while ANY consumer holds a Live
        // owner for this pubkey. HIGH 5 — track Live owners PER-CONSUMER (mirroring
        // `register_event_claim_interest`) so releasing the last Live owner while a
        // CacheOk owner remains downgrades the slot from Tailing in place, instead
        // of staying Tailing (a dangling Live tail) until full teardown.
        if liveness == ProfileLiveness::Live {
            self.live_profile_claims
                .entry(pubkey.to_string())
                .or_default()
                .insert(consumer_id.to_string());
        }
        let want_tailing = self.live_profile_claims.contains_key(pubkey);
        let lifecycle = if want_tailing {
            InterestLifecycle::Tailing
        } else {
            ProfileLiveness::CacheOk.lifecycle()
        };

        let mut authors = std::collections::BTreeSet::new();
        authors.insert(pubkey.to_string());
        let interest = LogicalInterest {
            id: profile_claim_interest_id(pubkey),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors,
                kinds: [0u32].into_iter().collect(),
                // `limit: None` — replaceable kind:0; lets same-shape author
                // sets coalesce into one batched REQ (merge Rule 5 refuses any
                // shape carrying a limit).
                limit: None,
                ..Default::default()
            },
            hints,
            lifecycle,
            // Opt into the bootstrap-indexer fallback so the cold-start
            // author-unknown case lands a REQ instead of going unroutable
            // (same flag `OneshotApi::request` / the self-kinds bootstrap set).
            is_indexer_discovery: true,
        };

        // Unified front-door (Replace = set_sub semantics): attach this owner
        // and replace the interest in place. The Tailing liveness upgrade works
        // because Replace always installs the new lifecycle, and `changed == true`
        // when lifecycle changed → recompile fires, upgrading OneShot→Tailing on
        // the wire. Store-serve is also triggered (fixing the kind:0 cold-start
        // bug: consequence §5a of the design doc).
        self.register_interest(
            &[crate::kernel::cache_serve::InterestRegistration {
                identity,
                interest,
                policy: crate::kernel::cache_serve::InterestWrite::Replace,
            }],
            "profile-claim",
        );
    }

    // integration-scaffold(#1671 Lane H): delete before final master cut.
    pub(crate) fn release_profile(
        &mut self,
        pubkey: &str,
        consumer_id: &str,
    ) -> Vec<OutboundMessage> {
        self.release_ref(RefNamespace::Profile, pubkey, consumer_id)
    }

    /// The `profile` reference release (ADR-0063). Generalizes the former
    /// `release_profile`: drop the consumer's refcount + registry owner, tear the
    /// slot down on the last owner, and bump the per-key rev.
    pub(in crate::kernel) fn release_profile_ref(
        &mut self,
        pubkey: &str,
        consumer_id: &str,
    ) -> Vec<OutboundMessage> {
        // Widest demanded shape BEFORE this consumer's shape is dropped, so a
        // release that drops the widest consumer (narrowing the row) counts as a
        // mutation even though no claim was added (HIGH 4 + BLOCKING 3).
        let widest_before = self.ref_demanded_profile_shape(pubkey);
        let mut actually_removed = false;
        let mut remove_claim = false;
        let mut remaining = 0;
        if let Some(consumers) = self.profile_claims.get_mut(pubkey) {
            actually_removed = consumers.remove(consumer_id);
            remaining = consumers.len();
            remove_claim = consumers.is_empty();
        }
        // ADR-0063 D5 (HIGH 4) — drop THIS consumer's per-consumer shape so the
        // widest demanded shape recomputes over the CURRENTLY-LIVE consumers. A
        // Card consumer leaving while a Ref consumer remains narrows the row to
        // Ref (instead of staying Card until full teardown); the per-key rev bump
        // below carries the narrowing to the wire.
        if let Some(consumers) = self.ref_profile_shapes.get_mut(pubkey) {
            consumers.remove(consumer_id);
            if consumers.is_empty() {
                self.ref_profile_shapes.remove(pubkey);
            }
        }
        let shape_narrowed = widest_before != self.ref_demanded_profile_shape(pubkey);

        // HIGH 5 — detach THIS consumer's Live owner (per-consumer, mirroring
        // `release_event_claim_interest`). When the LAST Live owner leaves while
        // CacheOk consumers remain, the slot must downgrade from Tailing to OneShot
        // in place rather than dangle a Live tail until full teardown.
        let mut liveness_downgraded = false;
        if let Some(live_owners) = self.live_profile_claims.get_mut(pubkey) {
            if live_owners.remove(consumer_id) && live_owners.is_empty() {
                self.live_profile_claims.remove(pubkey);
                liveness_downgraded = !remove_claim;
            }
        }
        if remove_claim {
            self.profile_claims.remove(pubkey);
        }

        // Detach this consumer's owner from the registry slot. When the last
        // owner leaves, the registry drops the interest and the planner emits
        // the CLOSE diff on the next drain.
        let key = profile_claim_sub_key(pubkey);
        let owner = SubOwnerKey::new(("profile-claim-owner", consumer_id));
        let identity = SubIdentity::new(owner, key, SubScope::Global);
        let slot_removed = self.lifecycle.registry_mut().drop_owner(&identity);
        if slot_removed {
            // The CLOSE diff only materialises when the planner recompiles.
            self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened {
                interest_ids: Vec::new(),
            });
        } else if liveness_downgraded {
            // HIGH 5 — the slot survives (CacheOk consumers remain) but no Live
            // owner is left: re-register it as OneShot (Replace = set_sub) so the
            // tailing kind:0 sub is downgraded in place. Re-attach an arbitrary
            // surviving consumer as the owner (any valid owner keeps the slot; the
            // refcount is `profile_claims`, not the registry owner set).
            if let Some(survivor) = self
                .profile_claims
                .get(pubkey)
                .and_then(|c| c.iter().next())
                .cloned()
            {
                self.register_profile_claim_interest(
                    pubkey,
                    &survivor,
                    ProfileLiveness::CacheOk,
                    Vec::new(),
                );
            }
        }

        // BLOCKING 3 — only stamp the projection on a real change: a refcount drop,
        // a shape narrow, or a liveness downgrade. A spurious release of a
        // never-claimed pubkey (no refcount change, no shape, no live owner) leaves
        // every rev untouched, so it never creates a permanent row (BLOCKING 2 (a)).
        let mutated = actually_removed || shape_narrowed || liveness_downgraded;
        if mutated {
            self.changed_since_emit = true;
            self.projection_rev_tracker.source_versions.bump_profile_claims();
        }
        // ADR-0063 Lane B (D6a) — per-key rev. On a full teardown the row is gone:
        // `clear_profile_row` bumps it to the final post-clear rev and immediately
        // removes the entry IN THE SAME CALL (BLOCKING 2 (b) — no retained-rev /
        // pending-clear state; the map stays bounded to live keys, D8). Otherwise a
        // surviving-row mutation bumps the rev so the narrowed/downgraded row
        // crosses FFI.
        if remove_claim && actually_removed {
            let _final_rev = self
                .projection_rev_tracker
                .source_versions
                .clear_profile_row(pubkey);
            // `_final_rev` is the value an ADR-0055 `Cleared` row would carry; the
            // downstream row-delta emitter (Lane A) is out of this branch's scope,
            // so it is dropped here and the rev entry is already gone.
        } else if mutated {
            self.projection_rev_tracker.source_versions.bump_profile_row(pubkey);
        }
        self.log(format!(
            "release profile {} consumer {} ref {}",
            short_hex(pubkey),
            truncate(consumer_id, 80),
            remaining
        ));
        Vec::new()
    }
}
