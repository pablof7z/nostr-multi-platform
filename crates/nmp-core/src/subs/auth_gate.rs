//! Auth-pause gate — partitions wire frames so REQs targeting a paused relay
//! are held in a pending buffer until `Authenticated` arrives. CLOSE frames
//! always pass through (we must be able to close stale subscriptions even on
//! paused relays — e.g. when the user logs out mid-connection).
//!
//! This is the M5 (NIP-42) coordination seam: T40 emits
//! `RelayAuthStateChanged` triggers into the inbox; the lifecycle records
//! state into [`AuthGate`]; new REQs check the gate before being returned;
//! pending REQs flush on `Authenticated`.

use std::collections::{BTreeMap, HashMap};

use super::trigger::RelayAuthState;
use super::wire::WireFrame;
use crate::planner::RelayUrl;

/// Per-relay auth state + buffer for REQs withheld until auth completes.
#[derive(Default)]
pub(super) struct AuthGate {
    state: HashMap<RelayUrl, RelayAuthState>,
    pending: BTreeMap<RelayUrl, Vec<WireFrame>>,
}

impl AuthGate {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// True when REQs to `relay_url` must be diverted to the pending buffer.
    /// `Authenticated` and `NotRequired` are pass-through; `Failed` is also
    /// pass-through because the actor / operator owns the resolution path
    /// (D7) and the buffer would otherwise grow without bound.
    pub(super) fn is_paused(&self, relay_url: &str) -> bool {
        matches!(
            self.state.get(relay_url),
            Some(RelayAuthState::ChallengeReceived) | Some(RelayAuthState::Authenticating)
        )
    }

    /// Record an auth-state transition. Returns the drained pending buffer
    /// when the new state is `Authenticated`; empty vec otherwise.
    pub(super) fn record_transition(
        &mut self,
        relay_url: RelayUrl,
        state: RelayAuthState,
    ) -> Vec<WireFrame> {
        let now_authenticated = matches!(state, RelayAuthState::Authenticated);
        self.state.insert(relay_url.clone(), state);
        if now_authenticated {
            self.pending.remove(&relay_url).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Partition a wire-frame batch: REQs targeting paused relays are
    /// diverted to the pending buffer; CLOSEs and REQs to live relays pass
    /// through. Returns the pass-through frames.
    pub(super) fn partition(&mut self, frames: Vec<WireFrame>) -> Vec<WireFrame> {
        let mut out = Vec::with_capacity(frames.len());
        for frame in frames {
            match &frame {
                WireFrame::Req { relay_url, .. } if self.is_paused(relay_url) => {
                    self.pending
                        .entry(relay_url.clone())
                        .or_default()
                        .push(frame);
                }
                _ => out.push(frame),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{InterestId, InterestLifecycle};

    fn req(relay: &str) -> WireFrame {
        WireFrame::Req {
            relay_url: relay.to_string(),
            sub_id: "x".to_string(),
            filter_json: "{}".to_string(),
            interest_id: InterestId(0),
            lifecycle: InterestLifecycle::Tailing,
        }
    }

    fn close(relay: &str) -> WireFrame {
        WireFrame::Close {
            relay_url: relay.to_string(),
            sub_id: "x".to_string(),
        }
    }

    #[test]
    fn challenge_received_pauses_reqs() {
        let mut g = AuthGate::new();
        g.record_transition("wss://r".to_string(), RelayAuthState::ChallengeReceived);
        let frames = g.partition(vec![req("wss://r"), req("wss://other")]);
        assert_eq!(frames.len(), 1, "only 'other' passes through");
    }

    #[test]
    fn close_always_passes_through() {
        let mut g = AuthGate::new();
        g.record_transition("wss://r".to_string(), RelayAuthState::ChallengeReceived);
        let frames = g.partition(vec![close("wss://r")]);
        assert_eq!(frames.len(), 1, "CLOSE passes despite pause");
    }

    #[test]
    fn authenticated_flushes_pending() {
        let mut g = AuthGate::new();
        g.record_transition("wss://r".to_string(), RelayAuthState::ChallengeReceived);
        g.partition(vec![req("wss://r")]);
        let flushed = g.record_transition("wss://r".to_string(), RelayAuthState::Authenticated);
        assert_eq!(flushed.len(), 1, "pending REQ flushed on Authenticated");
    }

    #[test]
    fn not_required_is_pass_through() {
        let mut g = AuthGate::new();
        g.record_transition("wss://r".to_string(), RelayAuthState::NotRequired);
        let frames = g.partition(vec![req("wss://r")]);
        assert_eq!(frames.len(), 1);
    }
}
