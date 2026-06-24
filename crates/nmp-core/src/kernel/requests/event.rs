//! Generic event-claim primitive: the canonical raw-key resolver
//! (`resolve_event_ref` / `release_event_ref`).
//!
//! Symmetric with [`super::profile::resolve_profile_ref`] but addresses events.
//! A "ref" is a refcounted assertion from one consumer that it wants the event
//! identified by a raw key reachable in `self.events`. The kernel parses the
//! **raw key** (ADR-0063 / FFI contract) into a canonical
//! [`super::event_key::EventTarget`] (a 64-char lowercase hex event-id OR a
//! `kind:pubkey:d` coordinate), refcounts the `consumer_id` into
//! `event_claims[primary_id]`, registers a `OneShot + Global` interest via
//! [`crate::subs::OneshotApi::request`] (D4: single registration path), and
//! enqueues a [`crate::subs::CompileTrigger::ViewOpened`] so the planner
//! compiles a wire REQ. `primary_id` is the projection key `claimed_events`
//! uses (hex64 id, or `kind:pubkey:d_tag` matching `WireUri.primary_id`).
//!
//! ## One raw-key front door (ADR-0063 D1 — single path)
//!
//! [`Kernel::resolve_event_ref`] is the CANONICAL body and takes the raw key
//! directly (the `event` arm of the origin-blind `resolve_ref` seam). Callers
//! that start from `nostr:` URIs decode them before crossing this boundary.
//!
//! D0 — no name here names a higher-layer content concept (`nmp-content` owns
//! the render-side projections). D6 — every error path silently logs and returns
//! `Vec::new()`; a malformed raw key no-ops (no claim, no discovery REQ, no
//! panic). D8 — no polling; interest registers once on the cold-claim transition
//! (`event_claim_requested` dedupes) and the projection re-emits on the next tick.

use super::super::{truncate, Instant, Kernel, OutboundMessage};
use super::event_key::{parse_event_key, EventTarget, PendingEventClaim};
use crate::kernel::refs::{EventShape, RefLiveness};
use crate::planner::{HintSource, InterestScope, RelayHint};

impl Kernel {
    /// The `event` reference resolver (ADR-0063), CANONICAL raw-key entry. The
    /// `event` arm of the origin-blind `resolve_ref` seam (refs.rs) routes here.
    /// `key` is the raw FFI key — a 64-char lowercase hex event-id or a
    /// `kind:pubkey:d` coordinate — NOT a `nostr:` URI. Refcount the consumer,
    /// register the kernel-owned fetch, record the widest demanded shape, and
    /// bump the per-key rev. `shape` selects the projected bytes (Lane C)
    /// orthogonally to `liveness`.
    ///
    /// **Liveness (ADR-0063):** event refs were `OneShot`-only. A
    /// [`RefLiveness::Live`] ref on an **addressable** coordinate now registers a
    /// *tailing* interest so replacements (a newer kind:3xxxx) arrive reactively
    /// — the event twin of a `Live` profile ref. Immutable event-ids cannot
    /// change, so `Live` degrades to the one-shot fetch for them. `caller_hints`
    /// are optional relay hints carried by the raw-key caller.
    #[allow(clippy::too_many_arguments)] // origin-blind seam; trimmed in Lane H.
    pub(in crate::kernel) fn resolve_event_ref(
        &mut self,
        key: String,
        consumer_id: String,
        shape: EventShape,
        liveness: RefLiveness,
        force: bool,
        can_send: bool,
        caller_hints: Vec<String>,
    ) -> Vec<OutboundMessage> {
        // Raw seam: the author is derived from the key itself (coordinate pubkey
        // or, for a bare event-id, unknown). No URI author TLV exists here.
        self.resolve_event_ref_inner(
            key,
            consumer_id,
            shape,
            liveness,
            force,
            can_send,
            caller_hints,
        )
    }

    /// Shared resolver body for the raw-key seam and the cold-park replay path.
    /// `relay_hints` are caller-supplied relay hints that seed the first
    /// one-shot interest and the claim-expansion candidate queue.
    #[allow(clippy::too_many_arguments)] // origin-blind seam; trimmed in Lane H.
    pub(in crate::kernel) fn resolve_event_ref_inner(
        &mut self,
        key: String,
        consumer_id: String,
        shape: EventShape,
        liveness: RefLiveness,
        force: bool,
        can_send: bool,
        relay_hints: Vec<String>,
    ) -> Vec<OutboundMessage> {
        // D6: a malformed raw key fails closed (no claim, no discovery REQ, no
        // panic). The two valid forms are a 64-char lowercase hex event-id and a
        // `kind:pubkey:d` coordinate.
        let Some(EventTarget {
            primary_id,
            replaceable_coord,
            filter,
            author: derived_author,
        }) = parse_event_key(&key)
        else {
            self.log(format!(
                "resolve_event_ref: ignoring malformed key {}",
                truncate(&key, 80)
            ));
            return Vec::new();
        };

        // Author for the claim-expansion Phase-1 warm filter. Raw event-id refs
        // do not carry an author; addressable coordinates derive it from the key.
        let author = derived_author;

        // Refcount + bound check (mirror of `resolve_profile_ref`). Drop-newest
        // on overflow bumps the diagnostic counter and silently no-ops (D6).
        let (inserted, refcount) = {
            let consumers = self.event_claims.entry(primary_id.clone()).or_default();
            if !consumers.contains(&consumer_id)
                && consumers.len() >= super::super::MAX_EVENT_CLAIMS_PER_KEY
            {
                self.event_claim_drops_total = self.event_claim_drops_total.saturating_add(1);
                return Vec::new();
            }
            let inserted = consumers.insert(consumer_id.clone());
            (inserted, consumers.len())
        };
        if inserted {
            self.log(format!(
                "claim event {} consumer {} ref {}",
                truncate(&primary_id, 80),
                truncate(&consumer_id, 80),
                refcount
            ));
        }
        // BLOCKING 3 — bump the per-key rev ONLY on a real row mutation: a new
        // consumer, a shape widen, or a liveness upgrade (CacheOk → Live). A
        // duplicate identical re-resolve re-asserts nothing and must not bump.
        // Capture the before-state (the live-owner set mutates in the `Live` branch).
        let widest_before = self.ref_demanded_event_shape(&primary_id);
        let live_before = self.live_event_claims.contains_key(&primary_id);
        // ADR-0063 D5 (HIGH 4) — record THIS consumer's demanded event shape
        // (per-consumer so a release recomputes the widest; bounded to claimed
        // keys). Orthogonal to liveness; the wire filter is shape-independent.
        self.ref_event_shapes
            .entry(primary_id.clone())
            .or_default()
            .insert(consumer_id.clone(), shape);
        let shape_widened = widest_before != self.ref_demanded_event_shape(&primary_id);
        // A `Live` claim on an addressable coord with no prior live owner is an
        // upgrade; immutable nevent/note ids (no coord) never become Live.
        let liveness_upgraded =
            liveness == RefLiveness::Live && replaceable_coord.is_some() && !live_before;
        let mutated = inserted || shape_widened || liveness_upgraded;
        // Must run BEFORE the already-resolved short-circuit so the projection
        // re-emits on the next tick even when no REQ goes out (the host needs the
        // `claimed_events[primary_id]` entry to render the embed card).
        if mutated {
            self.changed_since_emit = true;
            // ADR-0055 Rung 1: bump claimed_event_content_ver (codex #1 condition 1).
            self.projection_rev_tracker
                .source_versions
                .bump_claimed_event_content();
            // ADR-0063 Lane B (D6a) — per-key rev (resolve site 1 of 3).
            self.projection_rev_tracker
                .source_versions
                .bump_event_row(&primary_id);
        }

        // ADR-0063 — `Live` on an ADDRESSABLE coordinate registers a tailing
        // interest (the event twin of a `Live` profile claim) so kind:3xxxx
        // replacements arrive reactively. Immutable event-ids
        // (`replaceable_coord == None`) can never change, so `Live` falls through
        // to the one-shot path. The tailing slot dedups on a stable
        // `event-claim:<primary_id>` SubKey; marking `event_claim_requested`
        // makes a later `CacheOk` claim for the same coord a no-op (Live wins).
        if liveness == RefLiveness::Live {
            if replaceable_coord.is_some() {
                // BLOCKING 3 — Live wins: retire any `CacheOk` one-shot already
                // registered for this key (CacheOk-then-Live) so exactly ONE
                // interest / wire REQ survives. No-op for Live-first.
                self.cancel_event_oneshot(&primary_id);
                let hints: Vec<RelayHint> = relay_hints
                    .iter()
                    .map(|url| RelayHint {
                        url: url.clone(),
                        source: HintSource::UserConfigured,
                    })
                    .collect();
                self.register_event_claim_interest(&primary_id, &consumer_id, filter, hints);
                self.event_claim_requested.insert(primary_id.clone());
                // Run the F-TTL freshness gate when the coord is already cached
                // (a Live claim still wants a re-verification REQ on the open).
                if self.event_already_known(&primary_id) {
                    if let Some((kind, pubkey_hex, d_tag)) = replaceable_coord {
                        if let Ok(pk) = nostr::PublicKey::from_hex(&pubkey_hex) {
                            self.claim_replaceable(kind, pk.to_bytes(), Some(d_tag), force);
                        }
                    }
                }
                return Vec::new();
            }
        }

        // Already resolved or already requested → no fetch needed.
        if self.event_already_known(&primary_id) {
            // F-TTL — the event is cached, so no cold fetch goes out. For an
            // addressable coordinate, run the freshness gate: a lazy
            // re-verification REQ fires only if the TTL has elapsed
            // (`force == false`), or unconditionally when the user explicitly
            // navigated to / refreshed this entity (`force == true`). Immutable
            // event-ids (`replaceable_coord == None`) skip this entirely —
            // `force` is a silent no-op for them.
            if let Some((kind, pubkey_hex, d_tag)) = replaceable_coord {
                if let Ok(pk) = nostr::PublicKey::from_hex(&pubkey_hex) {
                    self.claim_replaceable(kind, pk.to_bytes(), Some(d_tag), force);
                }
            }
            return Vec::new();
        }
        if self.event_claim_requested.contains(&primary_id) {
            return Vec::new();
        }

        // Fix B (universal latent-bug fix): a cold claim (`!can_send`) parks ONLY
        // when it has no usable relay hint. When a relay hint is present, the
        // claim has a concrete publisher-provided relay to leave on right now — so
        // it falls through to the registration path below, which seeds the
        // OneshotApi interest with those hints. The planner then compiles a REQ
        // targeting the hint relay (empirically confirmed even with zero bootstrap
        // relays connected and no cached mailbox — see the hint-relay cold
        // resolve regression in `event_claim_tests`), and
        // `send_outbound` dials that URL on demand (relay_mgmt.rs:358-389).
        if !can_send && relay_hints.is_empty() {
            // Cold-start parking: the claim is already refcounted into
            // `event_claims` (so the renderer sees the row immediately) but no
            // OneshotApi interest is registered yet — no relay is reachable, so
            // there is nowhere to send a REQ. `pending_event_claim_requests`
            // drains this queue from `pending_view_requests` once `can_send`
            // flips, replaying each CANONICAL target through this same body.
            // Idempotent on the refcount side (`BTreeSet::insert` returns `false`
            // for the duplicate consumer) so the replay only registers the
            // OneshotApi interest the cold path skipped.
            self.log(format!(
                "event claim parked until relay connects: {}",
                truncate(&primary_id, 80)
            ));
            self.pending_event_claims.push(PendingEventClaim {
                key,
                consumer_id,
                shape,
                liveness,
                force,
                relay_hints,
            });
            return Vec::new();
        }

        // W5/§7.3 — seed the INITIAL OneshotApi REQ with the relay hints so the
        // first request fans out to publisher-provided content relays ∪ the
        // planner's bootstrap lanes, instead of bootstrap-only. The same hints
        // still flow to the tracker below for Phase-2 candidate scoring.
        // `UserConfigured` mirrors the variant `advance_to_phase2` already uses
        // for URI-sourced hints.
        let initial_hints: Vec<RelayHint> = relay_hints
            .iter()
            .map(|url| RelayHint {
                url: url.clone(),
                source: HintSource::UserConfigured,
            })
            .collect();

        // Unified front-door path: prepare mints the token and derives
        // identity+interest; register_interest installs via EnsureAbsent and
        // fires the store-serve + recompile trigger.
        let (token, interest_id, identity, interest) =
            self.oneshot
                .prepare(InterestScope::Global, filter, initial_hints);
        self.register_interest(
            &[crate::kernel::cache_serve::InterestRegistration {
                identity,
                interest,
                policy: crate::kernel::cache_serve::InterestWrite::EnsureAbsent,
            }],
            "oneshot-event-claim",
        );
        self.pending_discovery_oneshots
            .insert(interest_id.clone(), token);
        self.event_claim_requested.insert(primary_id.clone());
        // W5 — register claim-expansion tracker. Must be called AFTER prepare so
        // `interest_id` is resolved. The tracker stores the interest_id, author,
        // and relay hints for the Phase 1/2/3 state machine (§7.3 retarget).
        self.register_claim_expansion(
            primary_id,
            Some(interest_id),
            author,
            relay_hints,
            Instant::now(), // doctrine-allow: D9 — residual claim-expansion registration time tracked in #1952
        );
        // register_interest already enqueued InvalidateCompile on install.

        Vec::new()
    }

    /// The `event` reference release (ADR-0063), CANONICAL raw-key entry. The
    /// `event` arm of the origin-blind `release_ref` seam routes here with the
    /// raw key. Drop the consumer's refcount, tear the slot down on the last
    /// owner (incl. the `Live` tailing registry owner, if any), and bump the
    /// per-key rev. The OneshotApi row is NOT released here — the existing
    /// `complete_unknown_oneshot` path releases it on EOSE.
    pub(in crate::kernel) fn release_event_ref(
        &mut self,
        key: &str,
        consumer_id: &str,
    ) -> Vec<OutboundMessage> {
        // D6: a malformed raw key parses to `None` — a release of a never-valid
        // key is a silent no-op (it never created a claim row).
        let Some(EventTarget { primary_id, .. }) = parse_event_key(key) else {
            self.log(format!(
                "release_event_ref: ignoring malformed key {}",
                truncate(key, 80)
            ));
            return Vec::new();
        };

        let mut actually_removed = false;
        let mut remove_claim = false;
        let mut remaining = 0;
        // Widest demanded shape BEFORE this consumer's shape is dropped, so a
        // release that drops the widest consumer (narrowing a surviving row) counts
        // as a mutation (HIGH 4 + BLOCKING 3).
        let widest_before = self.ref_demanded_event_shape(&primary_id);
        if let Some(consumers) = self.event_claims.get_mut(&primary_id) {
            actually_removed = consumers.remove(consumer_id);
            remaining = consumers.len();
            remove_claim = consumers.is_empty();
        }
        // ADR-0063 D5 (HIGH 4) — drop THIS consumer's per-consumer shape so the
        // widest demanded shape recomputes over the currently-live consumers.
        if let Some(consumers) = self.ref_event_shapes.get_mut(&primary_id) {
            consumers.remove(consumer_id);
            if consumers.is_empty() {
                self.ref_event_shapes.remove(&primary_id);
            }
        }
        let shape_narrowed = widest_before != self.ref_demanded_event_shape(&primary_id);
        // BLOCKING 1 — detach THIS consumer's `Live` tailing owner on EVERY release
        // (no-op for CacheOk-only); the slot tears down on the last live owner.
        self.release_event_claim_interest(&primary_id, consumer_id);
        // ADR-0063 Lane B — drop THIS consumer's COLD-PARK stake from
        // `pending_event_claims` so a hintless claim released before the
        // relay-ready drain is not resurrected (rationale on the fn).
        self.remove_parked_event_claim(&primary_id, consumer_id);
        if remove_claim {
            // BLOCKING 1 — route last-consumer teardown through the SINGLE unified
            // key-teardown fn shared with the terminal-miss path (D4: one writer).
            // It drops `event_claims`, `event_claim_requested`, the per-consumer
            // shape map, the `Live` slot, the W5 claim-expansion tracker, stamps
            // the projection (`changed_since_emit`, `claimed_event_content_ver`),
            // and bump-then-clears the per-key rev — so the inline teardown the
            // legacy raw path performed (event_claims.remove +
            // event_claim_requested.remove + release_claim_expansion + the rev
            // bump) is fully subsumed here.
            self.teardown_event_claim_key(&primary_id);
        } else if actually_removed || shape_narrowed {
            // Surviving-row update on a real change only (BLOCKING 3): a real
            // refcount drop or a widest-shape narrowing. Stamp the projection and
            // bump the per-key rev so the host re-emits the narrowed row.
            self.changed_since_emit = true;
            self.projection_rev_tracker
                .source_versions
                .bump_claimed_event_content();
            // ADR-0063 Lane B (D6a) — per-key rev (release site 2 of 3) ONLY on a
            // real surviving-row change (BLOCKING 2 (a): a spurious release of a
            // never-claimed key must not create a permanent row).
            self.projection_rev_tracker
                .source_versions
                .bump_event_row(&primary_id);
        }
        self.log(format!(
            "release event {} consumer {} ref {}",
            truncate(&primary_id, 80),
            truncate(consumer_id, 80),
            remaining
        ));
        Vec::new()
    }
}
