//! Kernel request coordination — `req` / `req_for_relay` / `defer_outbound` /
//! `record_tx` primitives plus the per-tick view-request dispatcher.
//!
//! Logical groupings are split across sibling files:
//! - `relay_lifecycle.rs` — connecting/connected/failed/closed transitions
//! - `startup.rs`         — cold-start REQ emission (seed bootstrap + self profile)
//! - `auth_gate.rs`       — NIP-42 AUTH paused/failed predicates + outbound partition
//! - `profile.rs`         — profile/author open/close/claim/release
//! - `thread.rs`          — thread open/close/hydration

mod auth_gate;
mod profile;
mod relay_lifecycle;
mod startup;
mod thread;

use super::{discovery, json, Kernel, RelayRole, OutboundMessage, CanonicalRelayUrl, Value};

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

    pub(crate) fn pending_view_requests(&mut self) -> Vec<OutboundMessage> {
        let mut requests = Vec::new();
        while let Some(message) = self.deferred_outbound.pop_front() {
            requests.push(message);
        }
        // Check time-gated timeline open (contacts_deadline may have elapsed).
        requests.extend(self.maybe_open_timeline());
        if self.author_view.request_pending {
            requests.extend(self.author_requests());
        }
        if self.thread_view.request_pending {
            requests.extend(self.prepare_thread_requests());
        }
        if self.diagnostic_firehose.interest.is_some()
            && !self
                .wire
                .subs
                .keys()
                .any(|(_relay_url, sub_id)| sub_id.starts_with("diag-firehose-"))
        {
            requests.extend(self.firehose_requests());
        }
        requests.extend(self.pending_profile_claim_requests());
        requests.extend(self.maybe_open_thread_hydration());
        // T82: turn referenced-but-missing ids collected during ingest into
        // oneshot fetches (idempotent — no-op when the set is empty).
        requests.extend(self.drain_unknown_oneshots());
        requests
    }

    /// Close every wire-sub whose id matches one of `prefixes`, returning the
    /// CLOSE frames to dispatch.
    ///
    /// T133: rows are evicted from `wire_subs` (`HashMap::remove`) once the
    /// CLOSE outbound is constructed. Pre-T133 the row stayed with
    /// `state="closed"` for diagnostic surfacing — under long-running sessions
    /// this let the row table grow unbounded (every profile-claim, thread, or
    /// author view adds rows; close cycles never reclaimed them). Eviction is
    /// O(1) per row (`HashMap::remove`); no per-event alloc on the hot path
    /// (D8 invariant — the close path is cold relative to EVENT ingest).
    pub(crate) fn close_subscriptions_with_prefixes(
        &mut self,
        prefixes: &[&str],
    ) -> Vec<OutboundMessage> {
        // Two-pass: can't `remove` while holding a `&mut` iterator on the map.
        let mut closes = Vec::new();
        // #170: evict by the full `(relay_url, sub_id)` key — the same sub_id
        // may be live on multiple relays; a sub_id-only evict would drop a
        // sibling relay's row that no prefix targeted.
        let mut to_evict: Vec<(CanonicalRelayUrl, String)> = Vec::new();
        for sub in self.wire.subs.values() {
            if prefixes.iter().any(|prefix| sub.id.starts_with(prefix))
                && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
            {
                closes.push(OutboundMessage {
                    role: sub.role,
                    relay_url: sub.relay_url.to_string(),
                    text: json!(["CLOSE", sub.id]).to_string(),
                });
                to_evict.push((sub.relay_url.clone(), sub.id.clone()));
            }
        }
        for key in to_evict {
            self.wire.subs.remove(&key);
        }
        if !closes.is_empty() {
            self.changed_since_emit = true;
        }
        closes
    }

    /// Build REQ frames on every configured bootstrap socket for `role`.
    ///
    /// T105 transition shim: kept for diagnostic / one-off REQs (NIP-65
    /// discovery, indexer-only fetches) that legitimately leave on the
    /// bootstrap lanes.  Emits one frame per configured bootstrap URL. Per-author/recipient view emitters use
    /// [`Self::req_for_relay`] to route to the planner-resolved URL instead.
    ///
    /// V-04 Stage 2: the last in-tree caller (`active_account_bootstrap_requests`)
    /// migrated to `InterestRegistry::ensure_sub` + planner-driven wire-frame
    /// emission. The helper is kept under `#[allow(dead_code)]` because the
    /// PD-033-C plan retires `Kernel::req` entirely in a later stage; deleting
    /// it now would touch the doc-comment / `ONESHOT_SUB_PREFIX` retirement
    /// gates in `kernel/discovery.rs` that still reference the M1 helper name.
    #[allow(dead_code)]
    pub(crate) fn req(
        &mut self,
        role: RelayRole,
        sub_id: &str,
        summary: &str,
        filter: Value,
    ) -> Vec<OutboundMessage> {
        let mut out = Vec::new();
        for url in self.bootstrap_urls_for_role(role) {
            out.push(self.req_for_relay(role, url, sub_id, summary, filter.clone()));
        }
        out
    }

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
                    if let Some(token) =
                        self.pending_discovery_oneshots.remove(interest_id)
                    {
                        self.oneshot_subs.insert(
                            sub_id.clone(),
                            (token, discovery::OneshotKind::Discovery),
                        );
                    }
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
}
