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
use crate::nip21::{parse_nostr_uri, NostrUri};
use crate::planner::{HintSource, InterestScope, InterestShape, NaddrCoord, RelayHint};
use crate::subs::CompileTrigger;

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
    pub(crate) fn claim_event(
        &mut self,
        uri: String,
        consumer_id: String,
        can_send: bool,
        force: bool,
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
        let mut uri_author: Option<String> = None;
        let mut uri_relay_hints: Vec<String> = Vec::new();
        // F-TTL — only naddr URIs address a replaceable (addressable) identity
        // (kind, author-pubkey, d-tag). Captured here so it is in scope at the
        // cached-event branch below, where the TTL gate decides whether a
        // freshness re-verification REQ is due. nevent/note URIs are immutable
        // events with no TTL record, so they leave this `None` and `force`
        // is a silent no-op for them.
        let mut replaceable_coord: Option<(u32, String, String)> = None;

        let (primary_id, shape) = match parsed {
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
                let shape = InterestShape {
                    event_ids: std::iter::once(event_id.clone()).collect(),
                    limit: Some(1),
                    ..Default::default()
                };
                (event_id, shape)
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
                let shape = InterestShape {
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
                (primary_id, shape)
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
        // Must run BEFORE the already-resolved short-circuit so the
        // projection re-emits on the next tick even when no REQ goes
        // out (the host needs the `claimed_events[primary_id]` entry
        // to render the embed card).
        self.changed_since_emit = true;
        // ADR-0055 Rung 1: bump claimed_event_content_ver (codex #1 condition 1).
        self.projection_rev_tracker.source_versions.bump_claimed_event_content();

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
            // Cold-start parking. Mirrors `ProfileRequestState.pending`:
            // the claim has already been refcounted into `event_claims`
            // (so the renderer sees the claim row immediately) but no
            // OneshotApi interest is registered yet — no relay is
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

        // D4 — single registration path. The wire frame is emitted by
        // the planner's `drain_tick` (triggered by the `ViewOpened`
        // enqueue below), NOT by this function.
        let (token, interest_id) = {
            let registry = self.lifecycle.registry_mut();
            self.oneshot
                .request(registry, InterestScope::Global, shape, initial_hints)
        };
        self.pending_discovery_oneshots
            .insert(interest_id.clone(), token);
        self.event_claim_requested.insert(primary_id.clone());
        // W5 — register claim-expansion tracker. Must be called AFTER
        // OneshotApi::request so `interest_id` is resolved. The tracker
        // stores the interest_id, author, and URI relay hints for the
        // Phase 1/2/3 state machine (§7.3 retarget).
        self.register_claim_expansion(
            primary_id,
            Some(interest_id),
            uri_author,
            uri_relay_hints,
            Instant::now(),
        );
        // A2 — view-equivalent registered an interest. Empty
        // `interest_ids` is correct (the compiler walks the full
        // registry; this Vec is diagnostic provenance only).
        self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened {
            interest_ids: Vec::new(),
        });

        Vec::new()
    }

    /// Drop a consumer's claim on the event identified by `uri`. On the
    /// last consumer's release the row is removed from `event_claims`
    /// and from `event_claim_requested`; the OneshotApi row is NOT
    /// released here — the existing `complete_unknown_oneshot` path
    /// releases on EOSE (symmetric with `release_profile`).
    pub(crate) fn release_event(&mut self, uri: &str, consumer_id: &str) -> Vec<OutboundMessage> {
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

        let mut remove_claim = false;
        let mut remaining = 0;
        if let Some(consumers) = self.event_claims.get_mut(&primary_id) {
            consumers.remove(consumer_id);
            remaining = consumers.len();
            remove_claim = consumers.is_empty();
        }
        if remove_claim {
            self.event_claims.remove(&primary_id);
            // Allow a re-claim to re-register interest with the
            // OneshotApi (a stale `event_claim_requested` entry would
            // otherwise short-circuit the next cold-claim).
            self.event_claim_requested.remove(&primary_id);
            // codex M3 — the last consumer released, so cancel the W5
            // claim-expansion retargeting tracker for this id. Without
            // this, the `PendingClaim` would survive until its own
            // wall-clock budget elapses (`poll_claim_expansion`), keeping
            // Phase 1/2 hint-retargeting work alive for an event nobody
            // wants anymore.
            self.release_claim_expansion(&primary_id);
        }
        self.changed_since_emit = true;
        // ADR-0055 Rung 1: bump claimed_event_content_ver (codex #1 condition 1).
        self.projection_rev_tracker.source_versions.bump_claimed_event_content();
        self.log(format!(
            "release event {} consumer {} ref {}",
            truncate(&primary_id, 80),
            truncate(consumer_id, 80),
            remaining
        ));
        Vec::new()
    }

    /// Is the event identified by `primary_id` already in the kernel's
    /// read-cache? Hex64 keys look up `events` directly; coordinate
    /// keys (`kind:pubkey:d_tag`) scan `events.values()` for the
    /// matching addressable triple.
    ///
    /// Used by `claim_event` to short-circuit the OneshotApi
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
