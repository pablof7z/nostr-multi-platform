//! Auth-pause gate — partitions wire frames so REQs targeting a paused relay
//! are held in a pending buffer until `Authenticated` arrives. CLOSE frames
//! always pass through (we must be able to close stale subscriptions even on
//! paused relays — e.g. when the user logs out mid-connection).
//!
//! This is the M5 (NIP-42) coordination seam: T40 emits
//! `RelayAuthStateChanged` triggers into the inbox; the lifecycle records
//! state into [`AuthGate`]; new REQs check the gate before being returned;
//! pending REQs flush on `Authenticated`.
//!
//! ## Fail-closed semantics (T76, ADR-0019)
//!
//! A relay that demanded AUTH and then rejected/refused/timed-out it is in
//! `Failed`. Per doctrine D3 (outbox) and the NIP-42 spec, that relay MUST
//! NOT silently downgrade to unauthenticated reads — its AUTH-gated REQs
//! are **withheld**, not leaked onto the wire. `Failed` is therefore a
//! *paused* state here, exactly like `ChallengeReceived` / `Authenticating`.
//!
//! To keep the buffer bounded (the original fail-open rationale), the
//! transition INTO `Failed` **drops** that relay's pending buffer and
//! subsequent REQs are dropped (not buffered). Recovery is reconnect-only:
//! a relay reconnect resets the driver to `NotRequired`, the relay re-sends
//! a fresh challenge, and the handshake restarts cleanly. Other relays are
//! never affected — the gate is keyed per relay URL.

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

    /// True when REQs to `relay_url` must be withheld from the wire.
    /// `NotRequired` / `Authenticated` are pass-through. `ChallengeReceived`
    /// / `Authenticating` buffer (transient — flushed on `Authenticated`).
    /// `Failed` is **fail-closed**: REQs are withheld (and dropped, not
    /// buffered — see [`Self::is_failed`]) so an AUTH-gated relay never
    /// silently downgrades to unauthenticated reads (T76 / ADR-0019).
    pub(super) fn is_paused(&self, relay_url: &str) -> bool {
        matches!(
            self.state.get(relay_url),
            Some(
                RelayAuthState::ChallengeReceived
                    | RelayAuthState::Authenticating
                    | RelayAuthState::Failed
            )
        )
    }

    /// True when `relay_url` is fail-closed (`Failed`). Distinct from the
    /// transient pause states: REQs to a failed relay are dropped, never
    /// buffered, so the pending buffer stays bounded.
    pub(super) fn is_failed(&self, relay_url: &str) -> bool {
        matches!(self.state.get(relay_url), Some(RelayAuthState::Failed))
    }

    /// Record an auth-state transition. Returns the drained pending buffer
    /// when the new state is `Authenticated`; empty vec otherwise.
    ///
    /// Transition INTO `Failed` drops that relay's pending buffer: the REQs
    /// are not flushed (would leak unauthenticated) and not retained (would
    /// grow unbounded). They are reissued by the post-reconnect recompile.
    pub(super) fn record_transition(
        &mut self,
        relay_url: RelayUrl,
        state: RelayAuthState,
    ) -> Vec<WireFrame> {
        let now_authenticated = matches!(state, RelayAuthState::Authenticated);
        let now_failed = matches!(state, RelayAuthState::Failed);
        self.state.insert(relay_url.clone(), state);
        if now_authenticated {
            self.pending.remove(&relay_url).unwrap_or_default()
        } else {
            if now_failed {
                // Fail-closed: discard withheld REQs rather than leak them
                // to an unauthenticated relay or grow the buffer unbounded.
                self.pending.remove(&relay_url);
            }
            Vec::new()
        }
    }

    /// Partition a wire-frame batch: REQs targeting a failed relay are
    /// dropped (fail-closed); REQs targeting a transiently paused relay are
    /// diverted to the pending buffer; CLOSEs and REQs to live relays pass
    /// through. Returns the pass-through frames.
    pub(super) fn partition(&mut self, frames: Vec<WireFrame>) -> Vec<WireFrame> {
        let mut out = Vec::with_capacity(frames.len());
        for frame in frames {
            match &frame {
                WireFrame::Req { relay_url, .. } if self.is_failed(relay_url) => {
                    // Fail-closed drop — no buffer, no wire. Recovery is the
                    // post-reconnect recompile re-walking this interest.
                }
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

    // ─── T76 fail-closed regressions ────────────────────────────────────

    #[test]
    fn failed_relay_withholds_reqs_fail_closed() {
        // An AUTH-required relay that rejected AUTH must NOT receive its
        // gated REQs — fail-closed, not silent unauthenticated downgrade.
        let mut g = AuthGate::new();
        g.record_transition("wss://r".to_string(), RelayAuthState::Failed);
        let frames = g.partition(vec![req("wss://r")]);
        assert!(
            frames.is_empty(),
            "Failed relay must withhold REQs (fail-closed)"
        );
    }

    #[test]
    fn failed_relay_does_not_affect_other_relays() {
        let mut g = AuthGate::new();
        g.record_transition("wss://failed".to_string(), RelayAuthState::Failed);
        let frames = g.partition(vec![req("wss://failed"), req("wss://healthy")]);
        assert_eq!(frames.len(), 1, "only healthy relay's REQ passes");
        match &frames[0] {
            WireFrame::Req { relay_url, .. } => assert_eq!(relay_url, "wss://healthy"),
            other => panic!("expected healthy REQ, got {other:?}"),
        }
    }

    #[test]
    fn failed_relay_drops_reqs_not_buffered() {
        // The buffer must stay bounded: REQs to a failed relay are dropped,
        // never queued, so they cannot flush later (no late leak) and the
        // pending map does not grow.
        let mut g = AuthGate::new();
        g.record_transition("wss://r".to_string(), RelayAuthState::Failed);
        g.partition(vec![req("wss://r"), req("wss://r"), req("wss://r")]);
        // A subsequent (hypothetical) Authenticated transition must NOT
        // resurrect dropped REQs — recovery is the post-reconnect recompile.
        let flushed = g.record_transition("wss://r".to_string(), RelayAuthState::Authenticated);
        assert!(
            flushed.is_empty(),
            "dropped REQs must not resurrect on a later Authenticated"
        );
    }

    #[test]
    fn transition_into_failed_drops_existing_pending_buffer() {
        // REQs buffered during ChallengeReceived must be discarded when the
        // handshake fails — not flushed (would leak unauthenticated), not
        // retained (would grow unbounded).
        let mut g = AuthGate::new();
        g.record_transition("wss://r".to_string(), RelayAuthState::ChallengeReceived);
        g.partition(vec![req("wss://r"), req("wss://r")]);
        let drained_on_fail = g.record_transition("wss://r".to_string(), RelayAuthState::Failed);
        assert!(
            drained_on_fail.is_empty(),
            "Failed transition returns nothing (drop, not flush)"
        );
        let flushed_later =
            g.record_transition("wss://r".to_string(), RelayAuthState::Authenticated);
        assert!(
            flushed_later.is_empty(),
            "buffer was dropped on Failed; nothing to flush later"
        );
    }
}
