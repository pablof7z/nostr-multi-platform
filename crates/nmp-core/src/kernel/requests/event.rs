//! Generic event-claim primitive: `claim_event` / `release_event`.
//!
//! Symmetric with [`super::profile::claim_profile`] / `release_profile` but
//! addresses events instead of authors. A "claim" is a refcounted assertion
//! from one consumer (a view, a renderer, anything that surfaces an embed
//! card) that it wants the event identified by a `nostr:` URI to be
//! reachable in `self.events`. The kernel:
//!
//! 1. parses the URI into [`crate::nip21::NostrUri::Event`] (nevent/note)
//!    or [`crate::nip21::NostrUri::Address`] (naddr),
//! 2. inserts the `consumer_id` into `event_claims[primary_id]`,
//! 3. registers a `OneShot + Global` interest on the lifecycle registry
//!    via [`crate::subs::OneshotApi::request`] (D4: single registration
//!    path; no `self.req(...)` dual-write), passing
//!    [`crate::planner::InterestShape::event_ids`] for event-id URIs and
//!    [`crate::planner::InterestShape::addresses`] for naddr coordinates,
//!    and
//! 4. enqueues a [`crate::subs::CompileTrigger::ViewOpened`] so the
//!    planner's next `drain_tick` compiles the new interest into a wire
//!    REQ.
//!
//! `primary_id` is the projection key used by `claimed_events`:
//! - hex64 event id for nevent/note URIs (matches `StoredEvent.id`),
//! - `kind:pubkey:d_tag` coordinate string for naddr URIs (matches the
//!   renderer-side `WireUri.primary_id`).
//!
//! D0 — none of the names in this module name a higher-layer content
//! concept; the kernel primitive is content-shape agnostic. The
//! `nmp-content` crate owns the render-side projections that consume
//! this projection; the kernel never names those types.
//!
//! D6 — every error path silently logs and returns `Vec::new()`; no panic
//! and no propagated `Result` cross the FFI boundary.
//!
//! D8 — no polling. The kernel registers interest exactly once on the
//! cold-claim transition (`event_claim_requested` dedupes); ingest is
//! push, and the projection re-emits on the next snapshot tick.

use super::super::{truncate, Instant, Kernel, OutboundMessage};
use crate::kernel::refs::{EventShape, RefLiveness, RefNamespace};
use crate::nip21::{parse_nostr_uri, NostrUri};
use crate::planner::{HintSource, InterestScope, InterestShape, NaddrCoord, RelayHint};

impl Kernel {
    /// Refcount a consumer's interest in the event identified by `uri` and,
    /// on the cold-claim transition, register a `OneShot + Global`
    /// interest on the lifecycle registry so the planner compiles a REQ
    /// that fetches it.
    ///
    /// Mirrors [`Kernel::claim_profile`] line-for-line on the refcount,
    /// bound check (`MAX_EVENT_CLAIMS_PER_KEY` = 256, drop-newest +
    /// `event_claim_drops_total` increment), `changed_since_emit` flag,
    /// and the deferred-until-relay-connect log when `!can_send`. Cold-
    /// start callers re-enter once `relays_ready` flips; this primitive
    /// does NOT carry a separate pending queue (`pending_event_claims`).
    // integration-scaffold(#1671 Lane H): delete before final master cut.
    //
    // Thin delegator onto the generalized [`Kernel::resolve_event_ref`] body.
    // The legacy `claim_event` surface renders an embed card (`claimed_events` =
    // embed shape) at cache-ok freshness, so it maps to
    // [`EventShape::Embed`] + [`RefLiveness::CacheOk`]. It threads its caller's
    // `can_send` verbatim (the origin-blind [`Kernel::resolve_ref`] seam instead
    // derives readiness from `any_relay_connected`).
    pub(crate) fn claim_event(
        &mut self,
        uri: String,
        consumer_id: String,
        can_send: bool,
        force: bool,
    ) -> Vec<OutboundMessage> {
        self.resolve_event_ref(
            uri,
            consumer_id,
            EventShape::Embed,
            RefLiveness::CacheOk,
            force,
            can_send,
            Vec::new(),
        )
    }

    /// The `event` reference resolver (ADR-0063). Generalizes the former
    /// `claim_event`: refcount the consumer, register the kernel-owned fetch,
    /// record the widest demanded shape, and bump the per-key rev. `shape`
    /// selects the projected bytes (Lane C) orthogonally to `liveness`.
    ///
    /// **Liveness (new in ADR-0063):** event claims were `OneShot`-only. A
    /// [`RefLiveness::Live`] claim on an **addressable** (naddr) coordinate now
    /// registers a *tailing* interest so replacements (a newer kind:3xxxx) arrive
    /// reactively — the event twin of a `Live` profile claim. Immutable
    /// nevent/note ids cannot change, so `Live` degrades to the one-shot fetch for
    /// them. `caller_hints` are NIP-19 relay TLVs from a structured caller (the
    /// scaffold passes none; the URI's own TLVs are always parsed below).
    #[allow(clippy::too_many_arguments)] // origin-blind seam; trimmed in Lane H.
    pub(in crate::kernel) fn resolve_event_ref(
        &mut self,
        uri: String,
        consumer_id: String,
        shape: EventShape,
        liveness: RefLiveness,
        force: bool,
        can_send: bool,
        _caller_hints: Vec<String>,
    ) -> Vec<OutboundMessage> {
        // D6: silently swallow parse failures. The host may surface
        // arbitrary user-typed URIs (text content, mention pickers,
        // shared-link routing); a malformed string is never an FFI
        // error.
        let parsed = match parse_nostr_uri(&uri) {
            Ok(p) => p,
            Err(e) => {
                self.log(format!(
                    "claim_event: ignoring unparseable URI {}: {}",
                    truncate(&uri, 80),
                    e
                ));
                return Vec::new();
            }
        };

        // `claim_profile` is the right primitive for npub/nprofile —
        // routing kind:0 fetches through the indexer lane rather than
        // through this generic OneshotApi seam.
        // W5: carry author and relay hints from the URI TLV for claim-expansion.
        // Assigned unconditionally by the Event/Address match arms below;
        // the Profile arm returns early so no initializer is needed here.
        let uri_author: Option<String>;
        let uri_relay_hints: Vec<String>;
        // F-TTL — only naddr URIs address a replaceable (addressable) identity
        // (kind, author-pubkey, d-tag). Captured here so it is in scope at the
        // cached-event branch below, where the TTL gate decides whether a
        // freshness re-verification REQ is due. nevent/note URIs are immutable
        // events with no TTL record, so they leave this `None` and `force`
        // is a silent no-op for them.
        let mut replaceable_coord: Option<(u32, String, String)> = None;

        // `filter` is the wire-level [`InterestShape`] (the REQ filter); distinct
        // from the resolver-level `shape: EventShape` (which bytes to project).
        let (primary_id, filter) = match parsed {
            NostrUri::Profile { .. } => {
                self.log(format!(
                    "claim_event: refusing Profile URI (use claim_profile) {}",
                    truncate(&uri, 80)
                ));
                return Vec::new();
            }
            NostrUri::Event {
                event_id,
                author,
                relays,
                kind: _,
            } => {
                // §7.3: capture author TLV (seeds Phase-1 warm filter) and
                // relay hints (fed to Phase-2 candidate queue via W7).
                uri_author = author;
                uri_relay_hints = relays;
                let filter = InterestShape {
                    event_ids: std::iter::once(event_id.clone()).collect(),
                    limit: Some(1),
                    ..Default::default()
                };
                (event_id, filter)
            }
            NostrUri::Address {
                identifier,
                pubkey,
                kind,
                relays,
            } => {
                // Per NIP-01 §3.7 (addressable events), the canonical filter
                // for "fetch the event at coordinate (kind, pubkey, d_tag)" is
                //   {kinds:[K], authors:[A], "#d":[D], limit:1}
                //
                // We MUST NOT populate `InterestShape.addresses` here: that
                // field serializes as `#a` (events that REFERENCE the
                // coordinate via an `a` tag — bookmark lists, reposts).
                // Addressable events do NOT carry their own coordinate as an
                // `a` tag, so combining `#a` with `kinds`/`authors`/`#d`
                // produces an empty set on the relay. We use `authors` for
                // outbox routing (the planner's NIP-65 mailbox lookup keys
                // off `authors` just as well as `NaddrCoord::pubkey`).
                // W5: naddr author is the pubkey field (single-author by construction).
                uri_author = Some(pubkey.clone());
                uri_relay_hints = relays;
                // F-TTL — capture the addressable coordinate so the cached
                // branch can run the freshness gate (kind, author-pubkey, d-tag).
                replaceable_coord = Some((kind, pubkey.clone(), identifier.clone()));
                let mut tags: std::collections::BTreeMap<
                    String,
                    std::collections::BTreeSet<String>,
                > = std::collections::BTreeMap::new();
                tags.insert(
                    "d".to_string(),
                    std::iter::once(identifier.clone()).collect(),
                );
                let filter = InterestShape {
                    kinds: std::iter::once(kind).collect(),
                    authors: std::iter::once(pubkey.clone()).collect(),
                    tags,
                    limit: Some(1),
                    ..Default::default()
                };
                // Stable coordinate form — must match the renderer-side
                // `WireUri.primary_id`.
                let primary_id = format!("{kind}:{pubkey}:{identifier}");
                let _ = NaddrCoord {
                    pubkey: pubkey.clone(),
                    kind,
                    d_tag: identifier.clone(),
                };
                (primary_id, filter)
            }
        };

        // Refcount + bound check (mirror of `claim_profile`). Drop-newest
        // on overflow bumps the diagnostic counter and silently no-ops
        // (D6: never an FFI error).
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
            self.projection_rev_tracker.source_versions.bump_claimed_event_content();
            // ADR-0063 Lane B (D6a) — per-key rev (resolve site 1 of 3).
            self.projection_rev_tracker.source_versions.bump_event_row(&primary_id);
        }

        // ADR-0063 — `Live` on an ADDRESSABLE coordinate registers a tailing
        // interest (the event twin of a `Live` profile claim) so kind:3xxxx
        // replacements arrive reactively. Immutable nevent/note ids
        // (`replaceable_coord == None`) can never change, so `Live` falls through
        // to the one-shot path. The tailing slot dedups on a stable
        // `event-claim:<primary_id>` SubKey; marking `event_claim_requested`
        // makes a later `CacheOk` claim for the same coord a no-op (Live wins).
        if liveness == RefLiveness::Live {
            if let Some((_, _, _)) = replaceable_coord.as_ref() {
                // BLOCKING 3 — Live wins: retire any `CacheOk` one-shot already
                // registered for this key (CacheOk-then-Live) so exactly ONE
                // interest / wire REQ survives. No-op for Live-first.
                self.cancel_event_oneshot(&primary_id);
                let hints: Vec<RelayHint> = uri_relay_hints
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
            // addressable (naddr) coordinate, run the freshness gate: a lazy
            // re-verification REQ fires only if the TTL has elapsed
            // (`force == false`), or unconditionally when the user explicitly
            // navigated to / refreshed this entity (`force == true`).
            // nevent/note URIs are immutable (`replaceable_coord == None`) and
            // skip this entirely — `force` is a silent no-op for them.
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

        // Fix B (universal latent-bug fix): a cold claim (`!can_send`) parks
        // ONLY when it has no usable relay hint. When the URI carries NIP-19
        // relay TLVs, the claim has a concrete publisher-provided relay to leave
        // on right now — so it must fall through to the registration path
        // below, which seeds the OneshotApi interest with those hints. The
        // planner then compiles a REQ targeting the hint relay (empirically
        // confirmed even with zero bootstrap relays connected and no cached
        // mailbox — see `event_claim_tests::
        // claim_event_parked_with_uri_hint_registers_and_targets_hint_relay`),
        // and `send_outbound` dials that URL on demand (relay_mgmt.rs:358-389).
        // This lets an nevent with a working hint resolve even if NO bootstrap
        // relay is up.
        if !can_send && uri_relay_hints.is_empty() {
            // Cold-start parking: the claim has already been refcounted into
            // `event_claims` (so the renderer sees the claim row immediately)
            // but no OneshotApi interest is registered yet — no relay is
            // reachable, so there is nowhere to send a REQ.
            //
            // `pending_event_claim_requests` drains this queue from
            // `pending_view_requests` once `can_send` flips, replaying
            // each pair as a warm `claim_event(uri, consumer_id, true)`.
            // `claim_event` is idempotent on the refcount side
            // (`BTreeSet::insert` returns `false` for the duplicate
            // consumer) so the replay only registers the OneshotApi
            // interest that this cold path skipped.
            self.log(format!(
                "event claim parked until relay connects: {}",
                truncate(&uri, 80)
            ));
            self.pending_event_claims.push((uri, consumer_id));
            return Vec::new();
        }

        // W5/§7.3 — seed the INITIAL OneshotApi REQ with the URI's NIP-19
        // relay TLVs so the first request fans out to publisher-provided
        // content relays ∪ the planner's bootstrap lanes, instead of
        // bootstrap-only. Previously these hints only fed the Phase-2
        // retarget queue via `register_claim_expansion`; the cold REQ went
        // bootstrap-only and the publisher's own relays were not consulted
        // until Phase 2. The same hints still flow to the tracker below for
        // Phase-2 candidate scoring. `UserConfigured` mirrors the variant
        // `advance_to_phase2` already uses for URI-sourced hints.
        let initial_hints: Vec<RelayHint> = uri_relay_hints
            .iter()
            .map(|url| RelayHint {
                url: url.clone(),
                source: HintSource::UserConfigured,
            })
            .collect();

        // Unified front-door path: prepare mints the token and derives
        // identity+interest; register_interest installs via EnsureAbsent and
        // fires the store-serve + recompile trigger (replaces the bare
        // ensure_sub + manual ViewOpened enqueue pattern).
        let (token, interest_id, identity, interest) =
            self.oneshot.prepare(InterestScope::Global, filter, initial_hints);
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
        // W5 — register claim-expansion tracker. Must be called AFTER
        // prepare so `interest_id` is resolved. The tracker stores the
        // interest_id, author, and URI relay hints for the Phase 1/2/3
        // state machine (§7.3 retarget).
        self.register_claim_expansion(
            primary_id,
            Some(interest_id),
            uri_author,
            uri_relay_hints,
            Instant::now(),
        );
        // register_interest already enqueued InvalidateCompile on install.

        Vec::new()
    }

    // integration-scaffold(#1671 Lane H): delete before final master cut.
    pub(crate) fn release_event(&mut self, uri: &str, consumer_id: &str) -> Vec<OutboundMessage> {
        self.release_ref(RefNamespace::Event, uri, consumer_id)
    }

    /// The `event` reference release (ADR-0063). Generalizes the former
    /// `release_event`: drop the consumer's refcount, tear the slot down on the
    /// last owner (incl. the `Live` tailing registry owner, if any), and bump the
    /// per-key rev. The OneshotApi row is NOT released here — the existing
    /// `complete_unknown_oneshot` path releases it on EOSE.
    pub(in crate::kernel) fn release_event_ref(
        &mut self,
        uri: &str,
        consumer_id: &str,
    ) -> Vec<OutboundMessage> {
        // Resolve the URI to the same `primary_id` `claim_event`
        // computed. A re-parse is cheap and keeps the FFI surface
        // URI-string-symmetric — callers never have to remember a
        // computed key.
        let primary_id = match parse_nostr_uri(uri) {
            Ok(NostrUri::Event { event_id, .. }) => event_id,
            Ok(NostrUri::Address {
                identifier,
                pubkey,
                kind,
                ..
            }) => format!("{kind}:{pubkey}:{identifier}"),
            Ok(NostrUri::Profile { .. }) => {
                self.log(format!(
                    "release_event: refusing Profile URI {}",
                    truncate(uri, 80)
                ));
                return Vec::new();
            }
            Err(e) => {
                self.log(format!(
                    "release_event: ignoring unparseable URI {}: {}",
                    truncate(uri, 80),
                    e
                ));
                return Vec::new();
            }
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
            self.teardown_event_claim_key(&primary_id);
        } else if actually_removed || shape_narrowed {
            // Surviving-row update on a real change only (BLOCKING 3).
            self.changed_since_emit = true;
            self.projection_rev_tracker
                .source_versions
                .bump_claimed_event_content();
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
