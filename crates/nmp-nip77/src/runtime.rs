//! Runtime that wires NIP-77 into NMP's substrate hooks.

use std::collections::HashMap;
use std::sync::Mutex;

use nmp_core::planner::InterestLifecycle;
use nmp_core::substrate::{RelayTextInterceptor, ReqFrameContext, ReqFrameInterceptor};
use nmp_core::{Kernel, OutboundMessage};
use nmp_coverage_gate::{CoverageGate, FilterFanout};
use nostr::{Filter, JsonUtil as _, RelayMessage};

use crate::codec::{hex_decode, notice_mentions_negentropy};
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
        OutboundMessage::new(
            session.role,
            session.relay_url.clone(),
            messages::ids_req_text(&session.sub_id, ids),
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
        if !matches!(ctx.lifecycle, InterestLifecycle::OneShot) {
            return None;
        }
        if self.relay_state(&ctx.relay_url) == RelayNegentropyState::Unsupported {
            return None;
        }
        let filter = EligibleFilter::parse(&ctx.filter_json).ok()?;
        let fanout = FilterFanout::new(filter.authors.len(), filter.kinds.len());
        if !self.gate.should_use_negentropy_for_filter(fanout, true) {
            return None;
        }
        let store = kernel.event_store_handle();
        let items = filter.local_items(store.as_ref()).ok()?;
        let nostr_filter: Filter = serde_json::from_value(filter.value.clone()).ok()?;
        let mut reconciler = Reconciler::client(items).ok()?;
        let initial_msg = reconciler.initiate().ok()?;
        let session = Session {
            sub_id: ctx.sub_id.clone(),
            role: ctx.role,
            relay_url: ctx.relay_url.clone(),
            filter_json: ctx.filter_json.clone(),
            reconciler,
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
        Some(vec![OutboundMessage::new(
            ctx.role,
            ctx.relay_url.clone(),
            text,
        )])
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
                let Ok(msg) = hex_decode(&message) else {
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
                    Ok(ReconcilerOutcome::Done { need, .. }) => {
                        let mut out = vec![Self::close_msg(&session)];
                        if need.is_empty() {
                            kernel.complete_rewritten_wire_sub(relay_url, &session.sub_id);
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
}
