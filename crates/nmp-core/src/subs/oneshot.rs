//! `OneshotApi` — transient one-shot reads over the interest registry.
//!
//! Ported from notedeck's `OneshotApi` vs `ScopedSubApi` split in
//! `docs/design/nostrdb-notedeck-lessons.md` §3.9: register a short-lived
//! interest, let it deliver its first matching result(s), then auto-close — no
//! lingering subscription. Used for "fetch this one profile / quoted note then
//! forget it" (the [`crate::subs::UnknownIds`] drain target) without leaking a
//! permanent sub.
//!
//! ## Lifecycle (built on T81 primitives — nothing re-implemented)
//!
//! A oneshot is just a [`LogicalInterest`] with
//! [`InterestLifecycle::OneShot`] registered through
//! [`InterestRegistry::ensure_sub`]. The wire side CLOSEs the REQ on first
//! EOSE in `kernel/ingest`'s `handle_text` (the `keep_live` computation that
//! evicts non-persistent `wire_subs` rows) — this module adds **only** the
//! request → completion bookkeeping the actor polls. No parallel `OneShot`
//! tracker exists.
//!
//! ## Delivery model
//!
//! `nmp-core` has no async runtime and the kernel actor is synchronous, so
//! delivery is **poll-based**, not callback/future:
//! 1. [`OneshotApi::request`] → registers the interest, returns a
//!    [`OneshotToken`].
//! 2. The actor calls [`OneshotApi::complete`] from the ingest seam when a
//!    matching event lands (or on EOSE — "first result or end-of-stored").
//! 3. [`OneshotApi::drain_completed`] (idempotent) yields finished tokens; the
//!    actor releases each via [`OneshotApi::release`], dropping the registry
//!    owner so the interest GCs when no other owner holds it.
//!
//! Identical oneshots **dedup**: the registry owner is derived deterministically
//! from the `(scope, shape)` hash, so two `request`s for the same profile share
//! one registry slot (notedeck's dedup-across-owners, §3.2).
//!
//! Doctrine: **D4** the registry stays the single writer (this is a thin
//! facade — every mutation goes through `ensure_sub`/`drop_owner`).
//! **D6** no panics, no `Result` across FFI; internal state only.
//! **D8** `request` allocates one interest; `complete`/`drain` are O(touched)
//! and the token order is deterministic.

use std::collections::BTreeMap;

use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, RelayHint,
};
use crate::stable_hash::stable_hash64;
use crate::subs::registry::InterestRegistry;
use crate::subs::sub_key::{SubIdentity, SubKey, SubOwnerKey, SubScope};

/// Opaque handle to an in-flight oneshot. `Copy`/`Ord` so the actor can key
/// callbacks by it and iterate completions deterministically.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct OneshotToken(pub u64);

/// Per-request bookkeeping: the registry identity to release on teardown plus
/// the completion flag the actor flips from the ingest seam.
struct Pending {
    identity: SubIdentity,
    completed: bool,
}

/// Transient one-shot read coordinator. Owns the request table; borrows the
/// [`InterestRegistry`] on each call so the registry remains the single
/// writer (D4) — `OneshotApi` never holds a registry reference between calls.
#[derive(Default)]
pub struct OneshotApi {
    pending: BTreeMap<OneshotToken, Pending>,
    next_token: u64,
}

impl OneshotApi {
    /// Empty coordinator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a one-shot interest for `shape` under `scope` and return its
    /// token paired with the [`InterestId`] the registry assigned. Idempotent
    /// at the registry layer: a second `request` for the same `(scope, shape)`
    /// attaches a *distinct token* but shares the single deduped registry slot
    /// (notedeck §3.2) — so two views asking for the same profile produce one
    /// wire REQ.
    ///
    /// The interest is `OneShot`, so the existing wire lifecycle CLOSEs it on
    /// first EOSE; no extra close machinery here.
    ///
    /// The returned `InterestId` lets callers correlate the registered
    /// interest with the `WireFrame::Req { interest_id, … }` the planner
    /// later emits for it — the bridge `kernel::Kernel` uses to map the
    /// planner-assigned `sub_id` back to the `OneshotToken` so EOSE routing
    /// and store-gate decisions key on the actual wire sub-id (PD-033-C
    /// Stage 1). Identical `(scope, shape)` inputs return the same
    /// `InterestId` across calls — the dedup invariant the registry
    /// guarantees on the underlying `SubKey`.
    ///
    /// `hints` seeds the constructed [`LogicalInterest::hints`] so the first
    /// REQ for a hint-bearing read (e.g. a `nostr:nevent…` claim carrying
    /// NIP-19 relay TLVs) can fan out to publisher-provided relays in
    /// addition to the planner's bootstrap lanes. **The dedup key is
    /// `(scope, shape)` only** — `shape_key` does NOT hash `hints` — so
    /// callers passing `Vec::new()` observe byte-identical registry,
    /// `InterestId`, and dedup behavior to before this parameter existed.
    pub fn request(
        &mut self,
        registry: &mut InterestRegistry,
        scope: InterestScope,
        shape: InterestShape,
        hints: Vec<RelayHint>,
    ) -> (OneshotToken, InterestId) {
        let token = OneshotToken(self.next_token);
        self.next_token = self.next_token.saturating_add(1);

        let sub_scope = scope_to_sub_scope(&scope);
        let key = shape_key(&sub_scope, &shape);
        // Per-token owner, shared (scope, shape) key. The registry dedups the
        // *interest* across owners on the shared key (one slot, one wire REQ —
        // notedeck §3.2) while refcounting per token: the slot survives until
        // the last token releases its distinct owner.
        let owner = SubOwnerKey::new(("oneshot-owner", token.0));
        let identity = SubIdentity::new(owner, key, sub_scope);

        let interest_id = InterestId(key.0);
        let interest = LogicalInterest {
            id: interest_id.clone(),
            scope,
            shape,
            hints,
            lifecycle: InterestLifecycle::OneShot,
            // `OneshotApi::request` is the legacy discovery-direction
            // fan-out (`kernel/discovery.rs::drain_unknown_oneshots`); opt
            // into the planner-extension bootstrap-indexer fallback so the
            // cold-start author-unknown case keeps landing instead of
            // collapsing to `unroutable` (PD-033-C invariant).
            is_indexer_discovery: true,
        };
        // `ensure_sub`: register-if-absent. A re-request never clobbers an
        // in-flight filter (§3.3); it just attaches this token's owner.
        // Return value (newly installed?) intentionally unused — we only
        // need the side effect of the registration.
        let _ = registry.ensure_sub(identity.clone(), interest);

        self.pending.insert(
            token,
            Pending {
                identity,
                completed: false,
            },
        );
        (token, interest_id)
    }

    /// Mark `token`'s oneshot complete (first matching result observed, or
    /// EOSE reached). No-op for an unknown/already-complete token (D6: never
    /// panics). The interest is **not** dropped here — the actor decides when
    /// to [`Self::release`] (it may want to read the delivered result first).
    pub fn complete(&mut self, token: OneshotToken) {
        if let Some(p) = self.pending.get_mut(&token) {
            p.completed = true;
        }
    }

    /// True iff `token` is registered and has been completed.
    #[must_use]
    pub fn is_complete(&self, token: OneshotToken) -> bool {
        self.pending.get(&token).is_some_and(|p| p.completed)
    }

    /// Drain the set of completed tokens in deterministic order, leaving each
    /// still registered (the actor releases explicitly via [`Self::release`]
    /// once it has consumed the result). **Idempotent**: calling twice with no
    /// intervening [`Self::complete`] returns an empty vec the second time.
    #[must_use]
    pub fn drain_completed(&mut self) -> Vec<OneshotToken> {
        let done: Vec<OneshotToken> = self
            .pending
            .iter()
            .filter(|(_, p)| p.completed)
            .map(|(t, _)| *t)
            .collect();
        // Clear the completed flag so a second drain is empty (idempotent)
        // while the token stays registered until `release`.
        for t in &done {
            if let Some(p) = self.pending.get_mut(t) {
                p.completed = false;
            }
        }
        done
    }

    /// Tear down `token`: drop its registry owner (the slot GCs when the last
    /// owner — across deduped oneshots — leaves) and forget the token. No-op
    /// for an unknown token. Returns `true` iff a token was released.
    pub fn release(&mut self, registry: &mut InterestRegistry, token: OneshotToken) -> bool {
        let Some(p) = self.pending.remove(&token) else {
            return false;
        };
        let _ = registry.drop_owner(&p.identity);
        true
    }

    /// Number of in-flight (registered) oneshots. Diagnostics/tests.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }
}

/// Map the planner's account-context scope onto the registry's [`SubScope`].
/// `ActiveAccount` is not resolved to a concrete pubkey until compile time, so
/// it shares the global slot space here (mirrors `registry::legacy_scope`).
fn scope_to_sub_scope(scope: &InterestScope) -> SubScope {
    match scope {
        InterestScope::Account(pk) => SubScope::Account(pk.clone()),
        InterestScope::ActiveAccount | InterestScope::Global => SubScope::Global,
    }
}

/// Deterministic dedup key for a oneshot: hash `(scope, shape)` so two
/// requests for the same transient read collapse to one registry slot.
fn shape_key(scope: &SubScope, shape: &InterestShape) -> SubKey {
    SubKey(stable_hash64(("oneshot", scope, shape)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_shape(pk: &str) -> InterestShape {
        InterestShape::profile_for(pk.to_string())
    }

    #[test]
    fn oneshot_shape_key_is_restart_stable() {
        let key = shape_key(&SubScope::Global, &profile_shape("alice"));
        assert_eq!(key, SubKey(0x3ed4_bcb5_89bf_8034));
        assert_ne!(key, shape_key(&SubScope::Global, &profile_shape("bob")));
    }

    #[test]
    fn request_registers_a_oneshot_interest() {
        let mut reg = InterestRegistry::new();
        let mut api = OneshotApi::new();
        let (t, _id) = api.request(
            &mut reg,
            InterestScope::Global,
            profile_shape("alice"),
            Vec::new(),
        );
        assert_eq!(api.in_flight(), 1);
        assert_eq!(reg.iter_active().len(), 1);
        assert!(matches!(
            reg.iter_active()[0].lifecycle,
            InterestLifecycle::OneShot
        ));
        assert!(!api.is_complete(t));
    }

    #[test]
    fn identical_oneshots_dedup_to_one_registry_slot() {
        let mut reg = InterestRegistry::new();
        let mut api = OneshotApi::new();
        let (a, a_id) = api.request(
            &mut reg,
            InterestScope::Global,
            profile_shape("alice"),
            Vec::new(),
        );
        let (b, b_id) = api.request(
            &mut reg,
            InterestScope::Global,
            profile_shape("alice"),
            Vec::new(),
        );
        assert_ne!(a, b, "distinct tokens");
        assert_eq!(
            a_id, b_id,
            "deduped (scope,shape) returns the same interest_id across calls"
        );
        assert_eq!(api.in_flight(), 2);
        assert_eq!(
            reg.iter_active().len(),
            1,
            "deduped to a single (scope,shape) slot"
        );

        // Slot survives while either token holds it.
        api.release(&mut reg, a);
        assert_eq!(reg.iter_active().len(), 1);
        // Last owner leaves → slot GCs.
        api.release(&mut reg, b);
        assert!(reg.is_empty());
    }

    #[test]
    fn complete_then_drain_is_idempotent() {
        let mut reg = InterestRegistry::new();
        let mut api = OneshotApi::new();
        let (t, _id) = api.request(
            &mut reg,
            InterestScope::Global,
            profile_shape("bob"),
            Vec::new(),
        );
        api.complete(t);
        assert!(api.is_complete(t));

        let first = api.drain_completed();
        assert_eq!(first, vec![t]);
        let second = api.drain_completed();
        assert!(second.is_empty(), "second drain is empty, not errored");

        // Token is still registered until explicitly released.
        assert_eq!(api.in_flight(), 1);
        api.release(&mut reg, t);
        assert_eq!(api.in_flight(), 0);
        assert!(reg.is_empty());
    }

    #[test]
    fn complete_and_release_of_unknown_token_is_noop() {
        let mut reg = InterestRegistry::new();
        let mut api = OneshotApi::new();
        let ghost = OneshotToken(999);
        api.complete(ghost); // no panic
        assert!(!api.is_complete(ghost));
        assert!(!api.release(&mut reg, ghost));
    }

    #[test]
    fn distinct_shapes_get_distinct_slots() {
        let mut reg = InterestRegistry::new();
        let mut api = OneshotApi::new();
        let (_, alice_id) = api.request(
            &mut reg,
            InterestScope::Global,
            profile_shape("alice"),
            Vec::new(),
        );
        let (_, carol_id) = api.request(
            &mut reg,
            InterestScope::Global,
            profile_shape("carol"),
            Vec::new(),
        );
        assert_eq!(reg.iter_active().len(), 2);
        assert_ne!(
            alice_id, carol_id,
            "distinct shapes produce distinct interest_ids"
        );
    }

    /// Regression guard for the `hints` parameter (V-59 rung 1, #3): a caller
    /// passing `Vec::new()` must see BYTE-IDENTICAL behavior to before the
    /// parameter existed — empty `hints` on the registered interest, the same
    /// `InterestId`, and the same single deduped registry slot. This is the
    /// "no behavior change for non-hint callers" invariant the discovery
    /// oneshots rely on.
    #[test]
    fn empty_hints_registers_interest_with_no_hints() {
        let mut reg = InterestRegistry::new();
        let mut api = OneshotApi::new();
        let (_, id) = api.request(
            &mut reg,
            InterestScope::Global,
            profile_shape("alice"),
            Vec::new(),
        );

        let active = reg.iter_active();
        assert_eq!(active.len(), 1, "exactly one slot");
        let interest = &active[0];
        assert_eq!(
            interest.id, id,
            "returned id matches the registered interest"
        );
        assert!(
            interest.hints.is_empty(),
            "an empty-hints request must register an interest with no hints — \
             byte-identical to the pre-parameter behavior"
        );
    }

    /// Hints DO flow onto the constructed `LogicalInterest`, but they do NOT
    /// participate in dedup: a hint-bearing request and an otherwise-identical
    /// empty-hints request for the same `(scope, shape)` share one slot and one
    /// `InterestId`. `shape_key` hashes `(scope, shape)` only — proving hints
    /// cannot fork the registry slot (so a claim's hints never accidentally
    /// split a deduped read into two wire REQs).
    #[test]
    fn hints_populate_interest_but_do_not_affect_dedup() {
        let mut reg = InterestRegistry::new();
        let mut api = OneshotApi::new();

        let hint = RelayHint {
            url: "wss://relay.example.com".to_string(),
            source: crate::planner::HintSource::UserConfigured,
        };
        let (_, hinted_id) = api.request(
            &mut reg,
            InterestScope::Global,
            profile_shape("alice"),
            vec![hint.clone()],
        );
        // The registered interest carries the hint verbatim.
        let active = reg.iter_active();
        assert_eq!(active.len(), 1);
        assert_eq!(
            active[0].hints,
            vec![hint],
            "the constructed LogicalInterest must carry the supplied hints"
        );

        // A second request for the same (scope, shape) with EMPTY hints dedups
        // to the same slot and returns the same InterestId — hints are not part
        // of the dedup key.
        let (_, empty_id) = api.request(
            &mut reg,
            InterestScope::Global,
            profile_shape("alice"),
            Vec::new(),
        );
        assert_eq!(
            hinted_id, empty_id,
            "hints must not fork the dedup key — same (scope, shape) → same InterestId"
        );
        assert_eq!(
            reg.iter_active().len(),
            1,
            "hints must not create a second registry slot"
        );
    }
}
