//! Kernel request coordination — `req` / `req_for_relay` / `defer_outbound` /
//! `record_tx` primitives plus the per-tick view-request dispatcher.
//!
//! Logical groupings are split across sibling files:
//! - `relay_lifecycle.rs` — connecting/connected/failed/closed transitions
//! - `startup.rs`         — cold-start REQ emission (seed bootstrap + self profile)
//! - `auth_gate.rs`       — NIP-42 AUTH paused/failed predicates + outbound partition
//! - `profile.rs`         — profile/author open/close/claim/release

mod auth_gate;
mod event;
// ADR-0063 (#1671 Lane B): the Live/Tailing addressable-event slot + event-claim
// lookup/parking helpers, split out to keep `event.rs` under the 500-LOC ceiling.
mod event_live;
// ADR-0063 (#1671 Lane D): the canonical raw event-key parser (lowercase-hex id
// or `kind:pubkey:d` coordinate), split into its own module so `event.rs` stays
// under the 500-LOC ceiling.
mod event_key;
mod profile;
mod relay_lifecycle;
mod startup;

pub use profile::ProfileLiveness;
// ADR-0063 (#1671 Lane D): the canonical cold-start-parked event target.
pub(in crate::kernel) use event_key::parse_event_key;
pub(in crate::kernel) use event_key::PendingEventClaim;

use super::{
    discovery, json, wire_log, CanonicalRelayUrl, Kernel, OutboundMessage, RelayRole, Value,
};

impl Kernel {
    #[allow(dead_code)] // Per-lane snapshot retained for diagnostic surface (M11).
    pub(crate) fn active_subscriptions(&self, role: RelayRole) -> Vec<String> {
        self.wire
            .subs
            .values()
            .filter(|sub| {
                sub.role == role && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
            })
            .map(|sub| sub.id.clone())
            .collect()
    }

    /// Snapshot every active wire-sub as `(sub_id, relay_url)`. T105: the
    /// actor's lane-by-lane close path needs the URL each sub was opened on
    /// so the CLOSE can be routed to the right socket in the URL-keyed
    /// transport pool (the role alone is not enough — many sockets share
    /// one lane).
    pub(crate) fn snapshot_active_wire_subs(&self) -> Vec<(String, String)> {
        self.wire
            .subs
            .values()
            .filter(|sub| !matches!(sub.state.as_str(), "closed" | "closed_by_relay"))
            .map(|sub| (sub.id.clone(), sub.relay_url.to_string()))
            .collect()
    }

    /// Mark a protocol-rewritten planner sub complete without waiting for an
    /// `EOSE`. Used by sync protocols that determine no follow-up NIP-01 `REQ`
    /// is necessary.
    pub fn complete_rewritten_wire_sub(&mut self, relay_url: &str, sub_id: &str) {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.unregister_persistent_sub(key.as_str(), sub_id);
        self.wire.subs.remove(&(key, sub_id.to_string()));
        self.changed_since_emit = true;
    }

    pub(crate) fn pending_view_requests(&mut self) -> Vec<OutboundMessage> {
        let mut requests = Vec::new();
        while let Some(message) = self.deferred_outbound.pop_front() {
            requests.push(message);
        }
        // Check time-gated timeline open (contacts_deadline may have elapsed).
        requests.extend(self.maybe_open_timeline());
        // V-68 / V-112 (ADR-0042): author_view.request_pending / author_requests(),
        // thread_view.request_pending / prepare_thread_requests(), and
        // maybe_open_thread_hydration() deleted — per-app FlatFeed handles these.
        // M2 migration: profile (kind:0) claims are registry interests now
        // (`claim_profile` → `InterestRegistry`); there is no bespoke
        // `pending_profile_claim_requests` drain — the planner compiles them.
        requests.extend(self.pending_event_claim_requests());
        // F-TTL — register pending replaceable re-verification interests through
        // the registry chokepoint (no bespoke REQ build). The planner compiles
        // them on the next drain, so a reverify of a stale replaceable for an
        // author whose kind:10002 is uncached inherits the D3 probe + outbox
        // re-route. Returns no `OutboundMessage`s (the planner emits frames).
        self.drain_pending_reverify();
        // T82: turn referenced-but-missing ids collected during ingest into
        // oneshot fetches (idempotent — no-op when the set is empty).
        requests.extend(self.drain_unknown_oneshots());
        requests
    }

    // V-112 (ADR-0042): `close_subscriptions_with_prefixes` deleted — its only
    // callers were the retired close_author / close_thread view-close paths.
    // T133 wire-sub eviction on view close is carried by the planner CLOSE
    // diff (`drain_lifecycle_tick`) behind the generic close_interest seam,
    // plus the oneshot-EOSE / CLOSED-frame eviction paths.

    /// Build a single REQ frame addressed to `relay_url` on transport lane `role`.
    ///
    /// T105: the resolved per-author write relay (content/profile/thread) or
    /// recipient read relay (inbox notifications) is threaded straight onto
    /// the wire — the `RelayRole` only labels the diagnostic lane the frame
    /// belongs to. The recorded `WireSub` remembers `relay_url` so the EOSE
    /// CLOSE re-routes to the same socket the REQ went out on.
    ///
    /// T-relay-url-normalize: the relay URL is canonicalized before it is used
    /// as the `wire_subs` key and the stored `WireSub.relay_url` field. This is
    /// the other wire-sub registration path beside
    /// `register_planner_wire_frames`; both must write the same canonical key
    /// so the EOSE / CLOSED handler's canonicalized lookup hits the row.
    ///
    /// The emitted `OutboundMessage.relay_url` keeps the **raw** input form:
    /// it is purely a routing target, and the transport pool (`relay_mgmt.rs`)
    /// canonicalizes its own pool key, so a raw vs canonical `relay_url` dials
    /// the identical socket. Leaving it raw also keeps the routing assertions
    /// in the outbox/replay/profile-claim tests stable — they assert on the
    /// URL form the NIP-65 resolver produced, which is an orthogonal concern
    /// to the wire-sub map key.
    pub(crate) fn req_for_relay(
        &mut self,
        role: RelayRole,
        relay_url: String,
        sub_id: &str,
        summary: &str,
        filter: Value,
    ) -> OutboundMessage {
        // Canonical key for the `wire_subs` map; the raw `relay_url` is the
        // routing target on the emitted frame. Falls back to wrapping the raw
        // string for non-ws/wss inputs (`parse_or_raw`).
        let wire_key_url = CanonicalRelayUrl::parse_or_raw(&relay_url);
        self.log(format!(
            "REQ {sub_id}@{} ({}): {summary}",
            role.key(),
            relay_url
        ));
        let paused = self.relay_auth_paused(role);
        // K3 Stage D1: capture the REQ's `since` floor so the EOSE handler can
        // record coverage honestly (un-floored ⇒ `[0, now]`; floored ⇒ no row).
        let since_floor = filter.get("since").and_then(serde_json::Value::as_u64);
        // PD-033-C Stage 0: route through the single-writer helper. Stage 6
        // retires this M1 caller entirely; until then the helper preserves
        // M1's `auth_paused` initial state (M2 hardcodes `"opening"`, which
        // is a known asymmetry — see pd033c-plan.md §4.1).
        self.insert_wire_sub(
            role,
            wire_key_url,
            sub_id.to_string(),
            summary.to_string(),
            if paused { "auth_paused" } else { "opening" },
            since_floor,
        );
        OutboundMessage {
            role,
            relay_url,
            text: json!(["REQ", sub_id, filter]).to_string(),
        }
    }

    pub(crate) fn defer_outbound(&mut self, message: OutboundMessage) {
        self.log(format!(
            "defer {} outbound until relay reconnects",
            message.role.key()
        ));
        self.deferred_outbound.push_back(message);
        while self.deferred_outbound.len() > 64 {
            self.deferred_outbound.pop_front();
        }
        self.changed_since_emit = true;
    }

    pub(crate) fn record_tx(&mut self, role: RelayRole, bytes: usize) {
        let relay = self.relay_mut(role);
        relay.counters.bytes_tx = relay.counters.bytes_tx.saturating_add(bytes as u64);
    }

    /// Test-only: number of frames currently sitting in the deferred-outbound queue.
    /// Used by actor-level tests that cannot access the private field directly.
    #[cfg(test)]
    pub(crate) fn deferred_outbound_len(&self) -> usize {
        self.deferred_outbound.len()
    }

    /// T140 — register planner-emitted `WireFrame`s into the kernel's wire-sub
    /// bookkeeping so the EOSE handler treats them at parity with the retired
    /// M1 `seed-timeline-*` path.
    ///
    /// For every `WireFrame::Req`:
    ///   - a `WireSub` row is inserted (the EOSE handler at
    ///     `ingest/mod.rs` does `wire_subs.get_mut(sub_id)` to flip the sub to
    ///     `live`; without a row that is a silent no-op and the diagnostic
    ///     surface never shows the M2 follow feed);
    ///   - if the originating interest is `Tailing` (the follow-feed
    ///     lifecycle), the sub-id is registered persistent so the existing
    ///     `is_persistent_sub(sub_id)` branch of the EOSE keep-live predicate
    ///     keeps it open after the first EOSE — instead of inventing a new
    ///     `sub-*` prefix rule that would also (wrongly) keep `OneShot`
    ///     planner output alive. Lifecycle is the correct discriminator; it is
    ///     already carried on the frame.
    ///
    /// For every `WireFrame::Close`: drop the persistent registration and the
    /// wire-sub row so a re-routed/withdrawn follow no longer keeps a sub live.
    ///
    /// Called from the actor `wire_frames_to_outbound` bridge (the single
    /// point where planner frames cross into the transport layer).
    ///
    /// T-relay-url-normalize: planner-emitted `relay_url`s originate from
    /// kind:10002 NIP-65 relay lists — arbitrary, user-typed strings that may
    /// carry a non-canonical form (mixed-case scheme/host, empty-path trailing
    /// slash). The transport pool (`relay_mgmt.rs`) keys every socket — and
    /// every `RelayEvent` a worker emits — on the *canonical* URL. The EOSE
    /// handler in `ingest/mod.rs` therefore looks up `wire_subs` and
    /// `persistent_subs` under the canonical delivering URL. This boundary is
    /// the single point where planner URLs cross into the kernel's wire-sub
    /// bookkeeping, so every key written here is canonicalized to match.
    /// Without this, a `Tailing` follow-feed sub registered under a raw URL
    /// would never satisfy `is_persistent_sub(<canonical>, sub_id)` — the EOSE
    /// handler would wrongly auto-CLOSE the follow feed and leak its stale
    /// `wire_subs` row forever.
    pub(crate) fn register_planner_wire_frames(&mut self, frames: &[crate::subs::WireFrame]) {
        use crate::planner::InterestLifecycle;
        use crate::subs::WireFrame;
        for frame in frames {
            match frame {
                WireFrame::Req {
                    relay_url,
                    sub_id,
                    filter_json,
                    lifecycle,
                    interest_id,
                    ..
                } => {
                    // Canonical key so the EOSE handler's lookup (which uses
                    // the transport-stamped canonical delivering URL) hits the
                    // same `wire_subs` / `persistent_subs` entry. The
                    // `CanonicalRelayUrl` newtype makes that invariant
                    // compiler-enforced; `parse_or_raw` keeps the prior
                    // fail-open behavior for URLs that do not parse as ws/wss.
                    let key = CanonicalRelayUrl::parse_or_raw(relay_url);
                    let role = self
                        .role_for_relay_url(key.as_str())
                        .unwrap_or(RelayRole::Content);
                    if matches!(lifecycle, InterestLifecycle::Tailing) {
                        self.register_persistent_sub(key.as_str(), sub_id.clone());
                    }
                    // PD-033-C Stage 1 discovery-oneshot bridge: if this frame
                    // originated from a pending discovery oneshot registered by
                    // `drain_unknown_oneshots`, move the `OneshotToken` into
                    // `oneshot_subs` keyed by the **planner-assigned `sub_id`**
                    // so the EOSE handler (`complete_unknown_oneshot`) and the
                    // store-gate (`is_discovery_oneshot`) key on the actual
                    // wire sub-id. Pre-Stage 1, the kernel-side
                    // `oneshot-disc-{token}` sub_id was inserted by
                    // `drain_unknown_oneshots` AND emitted by the M1 dual-write
                    // so the two sides matched. With M1 retired the planner's
                    // `sub-<hash>` is the only sub-id that ever lands on the
                    // wire — `oneshot_subs` must be keyed on that.
                    if let Some(token) = self.pending_discovery_oneshots.remove(interest_id) {
                        self.oneshot_subs
                            .insert(sub_id.clone(), (token, discovery::OneshotKind::Discovery));
                    }
                    // F-TTL reverify-oneshot bridge (mirrors the discovery bridge
                    // above): move the reverify keys onto the planner-assigned
                    // `sub_id` so the EOSE handler re-stamps `check_again_after`
                    // against the actual wire sub-id. The planner reuses one
                    // `sub_id` across the relay URLs of a single interest (#170),
                    // so `remove` on the first frame transfers the keys once; the
                    // sibling relay frames find the entry already gone (their EOSE
                    // hits the same `reverify_subs[sub_id]` row).
                    if let Some(keys) = self.pending_reverify_oneshots.remove(interest_id) {
                        self.reverify_subs
                            .entry(sub_id.clone())
                            .or_default()
                            .extend(keys);
                    }
                    // Claim-expansion reverse-index bridge.
                    // If this frame's `interest_id` belongs to a pending claim,
                    // map the planner-assigned `sub_id` → `interest_id` so the
                    // ingest seam can look up the originating claim in O(log N).
                    // B4: also record (canonical_relay, sub_id) in in_flight_attempts
                    // so EOSE attribution is per-relay, not per-sub_id alone.
                    if self.pending_claims.contains_key(interest_id) {
                        self.claim_sub_index
                            .insert(sub_id.clone(), interest_id.clone());
                        if let Some(claim) = self.pending_claims.get_mut(interest_id) {
                            let canonical_relay = key.as_str().to_string();
                            claim
                                .in_flight_attempts
                                .insert((canonical_relay.clone(), sub_id.clone()));
                            // Emit ReqEmit at the wire-frame emission seam.
                            // Keep instrumentation in nmp-core, not nmp-planner.
                            // Phase discriminant:
                            // Phase1 → "phase1", Phase2InFlight → "phase2".
                            // D0: "phase" is a string discriminant, not a protocol noun.
                            let phase = match &claim.phase {
                                super::claim_expansion::Phase::Phase1 => "phase1",
                                super::claim_expansion::Phase::Phase2InFlight => "phase2",
                                super::claim_expansion::Phase::Terminal(_) => "terminal",
                            };
                            let author = claim.author.as_deref().unwrap_or("");
                            let has_hint = !claim.candidate_queue.is_empty();
                            wire_log::log_wire(wire_log::WireLogEvent::ReqEmit {
                                sub_id,
                                relay_url: &canonical_relay,
                                phase,
                                author,
                                has_hint,
                            });
                        }
                    }
                    // K3 Stage D1: parse the planner filter's `since` floor so
                    // the EOSE handler records coverage honestly (un-floored ⇒
                    // `[0, now]`; `since`-floored ⇒ no row, never over-claiming
                    // `[0, floor)`).
                    let since_floor = serde_json::from_str::<serde_json::Value>(filter_json)
                        .ok()
                        .and_then(|v| v.get("since").and_then(serde_json::Value::as_u64));
                    // PD-033-C Stage 0: route through the single-writer helper.
                    // After Stage 6 this is the SOLE caller of `insert_wire_sub`.
                    // M2 keeps its `"opening"` initial state (M1 has an extra
                    // `auth_paused` branch — see pd033c-plan.md §4.1 for the
                    // gap and the AuthGate consolidation that closes it).
                    self.insert_wire_sub(
                        role,
                        key,
                        sub_id.clone(),
                        filter_json.clone(),
                        "opening",
                        since_floor,
                    );
                }
                WireFrame::Close { relay_url, sub_id } => {
                    // Same canonicalization as the Req arm: a Close emitted
                    // with a non-canonical URL must still un-pin the sub and
                    // evict the row registered under the canonical key.
                    let key = CanonicalRelayUrl::parse_or_raw(relay_url);
                    self.unregister_persistent_sub(key.as_str(), sub_id);
                    self.wire.subs.remove(&(key, sub_id.clone()));
                }
            }
        }
        self.changed_since_emit = true;
    }

    /// F-TTL — register pending replaceable re-verification interests through
    /// the registry chokepoint (M2 migration).
    ///
    /// Each due [`crate::store::ReplaceableKey`] becomes a `OneShot + Global`
    /// `LogicalInterest { kinds:[k], authors:[pk], #d?, limit:None }` registered
    /// via [`crate::subs::OneshotApi::request`] — the SAME single-registration
    /// path `claim_event` / `claim_profile` use. This is what the pre-migration
    /// bespoke `req_for_relay` build could not give a reverify: by flowing
    /// through `recompile_and_diff`, a reverify of a stale replaceable for an
    /// author whose kind:10002 is uncached now triggers the D3 kind:10002 probe
    /// and re-routes onto the author's own relays when their relay-list arrives,
    /// instead of landing on the indexers only and never re-routing.
    ///
    /// The planner-assigned `sub_id` is bridged back to the reverify key set via
    /// `pending_reverify_oneshots` → `reverify_subs` in
    /// [`Kernel::register_planner_wire_frames`], so the EOSE handler re-stamps
    /// `check_again_after` against the actual wire sub-id (same bridge shape as
    /// `pending_discovery_oneshots`). Emits no `OutboundMessage`s; the planner
    /// emits the wire frames on its next drain.
    pub(crate) fn drain_pending_reverify(&mut self) {
        use crate::planner::{InterestScope, InterestShape};

        let mut registered_any = false;
        while let Some(key) = self.pending_reverify.pop_front() {
            let (kind, pubkey, d_tag_opt) = match &key {
                crate::store::ReplaceableKey::Regular { kind, pubkey } => (*kind, *pubkey, None),
                crate::store::ReplaceableKey::Parameterized {
                    kind,
                    pubkey,
                    d_tag,
                } => (*kind, *pubkey, Some(d_tag.clone())),
            };

            // Hex-encode the author pubkey (router + filter key on hex strings).
            let mut pubkey_hex = String::with_capacity(64);
            for byte in pubkey.iter() {
                pubkey_hex.push_str(&format!("{byte:02x}"));
            }

            let mut tags: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
                std::collections::BTreeMap::new();
            if let Some(d_tag) = &d_tag_opt {
                tags.insert("d".to_string(), std::iter::once(d_tag.clone()).collect());
            }
            let shape = InterestShape {
                kinds: std::iter::once(kind).collect(),
                authors: std::iter::once(pubkey_hex).collect(),
                tags,
                // `limit: None` so same-shape reverifies coalesce; the kinds
                // here are replaceable, so one event per (author, kind[, d]).
                limit: None,
                ..Default::default()
            };

            // Unified front-door path: prepare + register_interest (EnsureAbsent
            // = register-if-absent, so same-shape reverifies share one slot).
            let (_token, interest_id, identity, interest) =
                self.oneshot.prepare(InterestScope::Global, shape, Vec::new());
            self.register_interest(
                &[crate::kernel::cache_serve::InterestRegistration {
                    identity,
                    interest,
                    policy: crate::kernel::cache_serve::InterestWrite::EnsureAbsent,
                }],
                "reverify-oneshot",
            );
            // Bridge: planner emits the REQ for this interest_id; the bridge in
            // `register_planner_wire_frames` moves these keys into `reverify_subs`
            // under the planner sub_id so EOSE re-stamps the TTL.
            self.pending_reverify_oneshots
                .entry(interest_id)
                .or_default()
                .push(key.clone());
            registered_any = true;
        }

        // register_interest already enqueues InvalidateCompile on install.
        let _ = registered_any;
    }
}
