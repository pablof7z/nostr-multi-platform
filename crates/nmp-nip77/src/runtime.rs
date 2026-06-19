//! Runtime that wires NIP-77 into NMP's substrate hooks.

use std::collections::HashMap;
use std::sync::Mutex;

use nmp_planner::InterestLifecycle;
use nmp_core::substrate::{RelayTextInterceptor, ReqFrameContext, ReqFrameInterceptor};
use nmp_core::{Kernel, OutboundMessage};
use nmp_coverage_gate::{CoverageGate, FilterFanout};
use nostr::{Filter, JsonUtil as _, RelayMessage};

use crate::codec::{hex_decode_size_limited, notice_mentions_negentropy};
use crate::filter::EligibleFilter;
use crate::messages;
use crate::reconciler::{Reconciler, ReconcilerOutcome};

/// Cached NIP-77 support state for one relay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayNegentropyState {
    /// No response has been observed yet.
    Unknown,
    /// A `NEG-OPEN` was sent and no terminal response has arrived yet.
    Probing,
    /// Relay responded with `NEG-MSG`.
    Supported,
    /// Relay rejected the verb or announced negentropy is disabled.
    Unsupported,
}

struct Session {
    sub_id: String,
    role: nmp_core::RelayRole,
    relay_url: String,
    filter_json: String,
    reconciler: Reconciler,
    mode: SessionMode,
    /// Number of NEG-MSG round-trips seen in this session so far.
    rounds: u64,
    /// Local item count at session open (before items moved into reconciler).
    local_item_count: u64,
    /// Kernel wall-clock seconds at NEG-OPEN (K3 Stage B2 liveness deadline).
    /// A relay that silently ignores NEG-OPEN (no NEG-MSG / NEG-ERR / NOTICE)
    /// would otherwise leave this session stuck in `Probing` forever; the
    /// `on_idle_tick` sweep falls back to a plain REQ once
    /// `now − opened_at ≥ NEG_OPEN_LIVENESS_DEADLINE_SECS`.
    opened_at_secs: u64,
}

/// K3 Stage B2 — how long a `Probing` NEG session may sit with no terminal
/// response (NEG-MSG / NEG-ERR / NOTICE) before the liveness sweep falls back
/// to a plain REQ. Generous (30 s) so a slow-but-live relay mid-reconciliation
/// is not torn down; a NEG-MSG resets the wait by ending the `Probing` state.
const NEG_OPEN_LIVENESS_DEADLINE_SECS: u64 = 30;

#[derive(Clone, Debug, Eq, PartialEq)]
enum SessionMode {
    ReplaceOneShot,
    BackfillTailing { ids_sub_id: String },
}

/// Client-side NIP-77 runtime.
pub struct NegentropySyncRuntime {
    gate: CoverageGate,
    sessions: Mutex<HashMap<(String, String), Session>>,
    relay_states: Mutex<HashMap<String, RelayNegentropyState>>,
}

impl NegentropySyncRuntime {
    /// Build a runtime using the supplied large-filter gate.
    #[must_use]
    pub fn new(gate: CoverageGate) -> Self {
        Self {
            gate,
            sessions: Mutex::new(HashMap::new()),
            relay_states: Mutex::new(HashMap::new()),
        }
    }

    /// K3 (ADR-0056 §3.D2) — is this `(filter_hash, relay)` uncovered by the
    /// coverage ledger?
    ///
    /// "Uncovered" means there is NO completed-coverage row for
    /// `(canonical_filter_hash, relay)`. Such a shape has never been fully synced
    /// against this relay, so a full-window (Stage A un-floored) negentropy
    /// reconciliation should be preferred over a plain REQ regardless of fanout —
    /// it is the one mechanism that self-heals a below-floor gap.
    ///
    /// The `filter_hash` is the `sub-<hash>` wire-id suffix — the SAME key the
    /// write path records under and the floor read reads by. Non-planner ids
    /// (no `sub-` prefix) have no canonical hash and so no ledger row; they are
    /// treated as covered (not uncovered) — they fall through to the fanout
    /// gate, never force negentropy on a key the ledger cannot track.
    fn relay_filter_is_uncovered(&self, kernel: &Kernel, sub_id: &str, relay_url: &str) -> bool {
        let Some(filter_hash) = sub_id.strip_prefix("sub-") else {
            return false;
        };
        kernel
            .event_store_handle()
            .get_coverage(filter_hash, relay_url)
            .is_none()
    }

    /// Read cached relay support state.
    #[must_use]
    pub fn relay_state(&self, relay_url: &str) -> RelayNegentropyState {
        self.relay_states
            .lock()
            .ok()
            .and_then(|states| states.get(relay_url).copied())
            .unwrap_or(RelayNegentropyState::Unknown)
    }

    fn set_relay_state(
        &self,
        kernel: &mut Kernel,
        role: nmp_core::RelayRole,
        relay_url: &str,
        state: RelayNegentropyState,
    ) {
        if let Ok(mut states) = self.relay_states.lock() {
            states.insert(relay_url.to_string(), state);
        }
        let key = match state {
            RelayNegentropyState::Unknown => "unknown",
            RelayNegentropyState::Probing => "probing",
            RelayNegentropyState::Supported => "supported",
            RelayNegentropyState::Unsupported => "unsupported",
        };
        kernel.set_negentropy_probe_state(role, key);
    }

    fn fallback_req(session: &Session) -> OutboundMessage {
        OutboundMessage::new(
            session.role,
            session.relay_url.clone(),
            messages::req_text(&session.sub_id, &session.filter_json),
        )
    }

    fn close_msg(session: &Session) -> OutboundMessage {
        OutboundMessage::new(
            session.role,
            session.relay_url.clone(),
            messages::neg_close_text(&session.sub_id),
        )
    }

    fn ids_req(session: &Session, ids: &[[u8; 32]]) -> OutboundMessage {
        let sub_id = match &session.mode {
            SessionMode::ReplaceOneShot => session.sub_id.as_str(),
            SessionMode::BackfillTailing { ids_sub_id } => ids_sub_id.as_str(),
        };
        OutboundMessage::new(
            session.role,
            session.relay_url.clone(),
            messages::ids_req_text(sub_id, ids),
        )
    }

    fn fallback_all_for_relay(
        &self,
        kernel: &mut Kernel,
        relay_url: &str,
        _reason: &str,
    ) -> Vec<OutboundMessage> {
        let mut out = Vec::new();
        let Ok(mut sessions) = self.sessions.lock() else {
            return out;
        };
        let keys: Vec<_> = sessions
            .keys()
            .filter(|(url, _)| url == relay_url)
            .cloned()
            .collect();
        for key in keys {
            if let Some(session) = sessions.remove(&key) {
                self.set_relay_state(
                    kernel,
                    session.role,
                    relay_url,
                    RelayNegentropyState::Unsupported,
                );
                out.push(Self::fallback_req(&session));
            }
        }
        out
    }
}

impl ReqFrameInterceptor for NegentropySyncRuntime {
    fn intercept_req(
        &self,
        kernel: &mut Kernel,
        ctx: &ReqFrameContext,
    ) -> Option<Vec<OutboundMessage>> {
        if self.relay_state(&ctx.relay_url) == RelayNegentropyState::Unsupported {
            return None;
        }
        let mode = match ctx.lifecycle {
            InterestLifecycle::OneShot => SessionMode::ReplaceOneShot,
            InterestLifecycle::Tailing => SessionMode::BackfillTailing {
                ids_sub_id: format!("{}-neg-ids", ctx.sub_id),
            },
        };
        let filter = EligibleFilter::parse(&ctx.filter_json).ok()?;
        let fanout = FilterFanout::new(filter.authors.len(), filter.kinds.len());
        // K3 Stage D2 (ADR-0056 §3.D2): the gate consults coverage-ledger
        // STALENESS, not fanout alone. When the ledger is enabled and this
        // `(filter_hash, relay)` has NO completed-coverage row, the shape has
        // never been fully synced against this relay — exactly the case the
        // full-window (un-floored, Stage A) negentropy reconciliation is built
        // to self-heal — so we prefer negentropy regardless of fanout. When a
        // coverage row DOES exist (already synced) or the flag is OFF, the
        // fanout heuristic governs as before (the fallback the ADR keeps).
        let uncovered = self.relay_filter_is_uncovered(kernel, &ctx.sub_id, &ctx.relay_url);
        if !(uncovered || self.gate.should_use_negentropy_for_filter(fanout, true)) {
            return None;
        }
        // K3 Stage A: reconcile over the FULL window, not the watermark-floored
        // `[floor, ∞)`. The `since` floor is a presence heuristic (avoid
        // re-fetching cached events on a plain REQ); NIP-77 transfers exactly the
        // id-set symmetric difference, so an un-floored reconciliation is
        // self-healing for below-floor gaps. The floored filter is retained only
        // for the fallback / live-only plain REQs below, which legitimately keep
        // the floor.
        let neg_filter = filter.unfloored();
        let store = kernel.event_store_handle();
        let items = neg_filter.local_items(store.as_ref()).ok()?;
        let local_item_count = items.len() as u64;
        let nostr_filter: Filter = serde_json::from_value(neg_filter.value.clone()).ok()?;
        let mut reconciler = Reconciler::client(items).ok()?;
        let initial_msg = reconciler.initiate().ok()?;
        let session = Session {
            sub_id: ctx.sub_id.clone(),
            role: ctx.role,
            relay_url: ctx.relay_url.clone(),
            filter_json: ctx.filter_json.clone(),
            reconciler,
            mode: mode.clone(),
            rounds: 0,
            local_item_count,
            // K3 Stage B2: stamp the kernel wall-clock at open so the liveness
            // sweep can detect a relay that never responds to NEG-OPEN.
            opened_at_secs: kernel.now_secs(),
        };
        let text = messages::neg_open_text(&ctx.sub_id, nostr_filter, &initial_msg);
        self.set_relay_state(
            kernel,
            ctx.role,
            &ctx.relay_url,
            RelayNegentropyState::Probing,
        );
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert((ctx.relay_url.clone(), ctx.sub_id.clone()), session);
        }
        let neg_open = OutboundMessage::new(ctx.role, ctx.relay_url.clone(), text);
        match mode {
            SessionMode::ReplaceOneShot => Some(vec![neg_open]),
            SessionMode::BackfillTailing { .. } => Some(vec![
                OutboundMessage::new(
                    ctx.role,
                    ctx.relay_url.clone(),
                    messages::req_text(&ctx.sub_id, &filter.live_only_filter_json()),
                ),
                neg_open,
            ]),
        }
    }
}

impl RelayTextInterceptor for NegentropySyncRuntime {
    fn on_relay_text(
        &self,
        kernel: &mut Kernel,
        relay_url: &str,
        text: &str,
    ) -> Vec<OutboundMessage> {
        let Ok(message) = RelayMessage::from_json(text) else {
            return Vec::new();
        };
        match message {
            RelayMessage::Notice(message) if notice_mentions_negentropy(&message) => {
                self.fallback_all_for_relay(kernel, relay_url, &message)
            }
            RelayMessage::NegErr {
                subscription_id,
                message: _message,
            } => {
                let sub_id = subscription_id.to_string();
                let key = (relay_url.to_string(), sub_id);
                let Some(session) = self.sessions.lock().ok().and_then(|mut s| s.remove(&key))
                else {
                    return Vec::new();
                };
                self.set_relay_state(
                    kernel,
                    session.role,
                    relay_url,
                    RelayNegentropyState::Unsupported,
                );
                vec![Self::fallback_req(&session)]
            }
            RelayMessage::NegMsg {
                subscription_id,
                message,
            } => {
                let sub_id = subscription_id.to_string();
                let key = (relay_url.to_string(), sub_id);
                let Some(mut session) = self.sessions.lock().ok().and_then(|mut s| s.remove(&key))
                else {
                    return Vec::new();
                };
                let Ok(msg) = hex_decode_size_limited(&message) else {
                    self.set_relay_state(
                        kernel,
                        session.role,
                        relay_url,
                        RelayNegentropyState::Unsupported,
                    );
                    return vec![Self::fallback_req(&session)];
                };
                self.set_relay_state(
                    kernel,
                    session.role,
                    relay_url,
                    RelayNegentropyState::Supported,
                );
                session.rounds = session.rounds.saturating_add(1);
                // K3 Stage B2: a NEG-MSG is forward progress — re-anchor the
                // liveness deadline so a slow-but-live multi-round
                // reconciliation is measured from its LAST progress, not from
                // open, and is never torn down while it is actively advancing.
                session.opened_at_secs = kernel.now_secs();
                match session.reconciler.reconcile(&msg) {
                    Ok(ReconcilerOutcome::Send(next)) => {
                        let outbound = OutboundMessage::new(
                            session.role,
                            session.relay_url.clone(),
                            messages::neg_msg_text(&session.sub_id, &next),
                        );
                        if let Ok(mut sessions) = self.sessions.lock() {
                            sessions
                                .insert((relay_url.to_string(), session.sub_id.clone()), session);
                        }
                        vec![outbound]
                    }
                    Ok(ReconcilerOutcome::Done { have, need }) => {
                        kernel.set_negentropy_sync_stats(
                            session.rounds,
                            have.len() as u64,
                            need.len() as u64,
                            session.local_item_count,
                        );
                        // K3 Stage D1 (ADR-0056 §3) — reconciliation COMPLETED
                        // for this (filter, relay). Per Stage A the NEG window is
                        // un-floored `[0, ∞)`, so the completed sync honestly
                        // covers `[0, now]`; advance the coverage ledger. Gated
                        // on the kernel's off-by-default flag, so this is a no-op
                        // in D1's default configuration.
                        let now_secs = kernel.now_secs();
                        kernel.record_neg_done_coverage(&session.sub_id, relay_url, now_secs);
                        let mut out = vec![Self::close_msg(&session)];
                        if need.is_empty() {
                            if matches!(session.mode, SessionMode::ReplaceOneShot) {
                                kernel.complete_rewritten_wire_sub(relay_url, &session.sub_id);
                            }
                        } else {
                            out.push(Self::ids_req(&session, &need));
                        }
                        out
                    }
                    Err(_) => {
                        vec![Self::fallback_req(&session)]
                    }
                }
            }
            _ => Vec::new(),
        }
    }

    /// K3 Stage B2 — NEG-OPEN liveness sweep (D8: wall-clock-gated, no
    /// sleep/loop). The actor calls this on every idle iteration (~250 ms
    /// cadence). A relay that accepts NEG-OPEN but then goes silent — no
    /// NEG-MSG, no NEG-ERR, no NOTICE — would otherwise leave the interest's
    /// wire sub "live" with no EOSE and no retry, starving it forever. For
    /// every session that has sat with no terminal response for at least
    /// [`NEG_OPEN_LIVENESS_DEADLINE_SECS`], fall back to the plain (floored)
    /// REQ — the SAME fallback path NEG-ERR triggers — so the interest is
    /// never stuck. `opened_at_secs` is re-anchored on each NEG-MSG, so a
    /// slow-but-live reconciliation is measured from its last progress and is
    /// never torn down while actively advancing.
    fn on_idle_tick(&self, kernel: &mut Kernel) -> Vec<OutboundMessage> {
        let now = kernel.now_secs();
        let mut out = Vec::new();

        // Phase 1: collect timed-out sessions under the lock (no kernel touch
        // inside the lock; the set_relay_state below re-enters the kernel).
        let timed_out: Vec<(String, String)> = {
            let Ok(sessions) = self.sessions.lock() else {
                return out;
            };
            sessions
                .iter()
                .filter(|(_, s)| {
                    now.saturating_sub(s.opened_at_secs) >= NEG_OPEN_LIVENESS_DEADLINE_SECS
                })
                .map(|(key, _)| key.clone())
                .collect()
        };

        // Phase 2: remove each timed-out session and emit its fallback REQ.
        for key in timed_out {
            let Some(session) = self.sessions.lock().ok().and_then(|mut s| s.remove(&key)) else {
                continue;
            };
            // Mark the relay Unsupported for this session's lifetime — the same
            // terminal state NEG-ERR records — so subsequent interests on this
            // relay skip the NEG probe and go straight to a plain REQ rather
            // than re-incurring the deadline.
            self.set_relay_state(
                kernel,
                session.role,
                &session.relay_url,
                RelayNegentropyState::Unsupported,
            );
            out.push(Self::fallback_req(&session));
        }
        out
    }
}
