//! Outbound pre-connect buffer drop surfacing (#2765).
//!
//! Mirrors [`super::inbound::drain_inbound`]'s "never a silent loss" pattern
//! for the outbound side: [`nmp_network::browser_send_buffer::PreConnectSendBuffer`]
//! records every frame it evicts on overflow; [`surface_outbound_drops`] turns
//! those records into an explicit [`BrowserRuntimeEvent::RelayOutboundDropped`]
//! event per drop and, for evicted publish `EVENT` frames, un-sticks the
//! kernel's publish engine so the event re-dispatches rather than staying
//! forever `InFlight` against a relay it never actually reached.
//!
//! This module (unlike [`super::spawn`]) is NOT `cfg(target_arch = "wasm32")`
//! gated — `DroppedOutboundFrame` and `KernelReducer` are both plain Rust
//! types, so the classify/emit/terminal-wire logic is unit-testable on native
//! targets even though the driver that produces the drops
//! (`nmp_network::browser_driver::BrowserRelayDriver`) is wasm32-only.

use nmp_core::KernelReducer;
use nmp_network::browser_send_buffer::DroppedOutboundFrame;

use crate::BrowserRuntimeEvent;

/// Classify an outbound NIP-01 frame's leading array tag for diagnostics.
///
/// Only `"EVENT"` drives the terminal-failure path below; the rest are purely
/// informational so the host can tell what kind of traffic was lost.
fn classify_frame_kind(text: &str) -> &'static str {
    if text.starts_with("[\"EVENT\"") {
        "EVENT"
    } else if text.starts_with("[\"REQ\"") {
        "REQ"
    } else if text.starts_with("[\"CLOSE\"") {
        "CLOSE"
    } else if text.starts_with("[\"AUTH\"") {
        "AUTH"
    } else if text.starts_with("[\"COUNT\"") {
        "COUNT"
    } else {
        "other"
    }
}

/// Turn drained [`DroppedOutboundFrame`]s into host events, un-sticking the
/// publish engine for every evicted `EVENT` frame.
///
/// For `kind == "EVENT"` this calls
/// `reducer.handle_relay_outbound_dropped`, which moves affected `InFlight`
/// publish attempts back to `Pending` without marking the still-healthy,
/// still-connecting relay as failed. The frame that overflowed the pre-connect
/// buffer never reached the socket, so retry is correct; relay-health churn is
/// not.
pub(crate) fn surface_outbound_drops(
    drops: &[DroppedOutboundFrame],
    reducer: &mut KernelReducer,
) -> Vec<BrowserRuntimeEvent> {
    let mut events = Vec::with_capacity(drops.len());
    for drop in drops {
        let kind = classify_frame_kind(&drop.text);
        events.push(BrowserRuntimeEvent::RelayOutboundDropped {
            url: drop.url.clone(),
            kind: kind.to_string(),
        });
        if kind == "EVENT" {
            reducer.handle_relay_outbound_dropped(drop.role, &drop.url);
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_network::role::RelayRole;

    fn drop_frame(text: &str) -> DroppedOutboundFrame {
        DroppedOutboundFrame {
            url: "wss://relay.example".to_string(),
            role: RelayRole::Content,
            text: text.to_string(),
        }
    }

    #[test]
    fn event_kind_drop_surfaces_one_event_and_does_not_panic_the_reducer() {
        let mut reducer = KernelReducer::new();
        let drops = vec![drop_frame("[\"EVENT\",{\"id\":\"abc\"}]")];

        let events = surface_outbound_drops(&drops, &mut reducer);

        assert_eq!(events.len(), 1, "exactly one drop event surfaced");
        match &events[0] {
            BrowserRuntimeEvent::RelayOutboundDropped { url, kind } => {
                assert_eq!(url, "wss://relay.example");
                assert_eq!(kind, "EVENT");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn req_kind_drop_is_classified_distinctly_from_event() {
        let mut reducer = KernelReducer::new();
        let drops = vec![drop_frame("[\"REQ\",\"sub1\",{}]")];

        let events = surface_outbound_drops(&drops, &mut reducer);

        assert_eq!(events.len(), 1);
        match &events[0] {
            BrowserRuntimeEvent::RelayOutboundDropped { kind, .. } => {
                assert_eq!(kind, "REQ", "REQ frames must not classify as EVENT");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn multiple_drops_surface_one_event_each_in_order() {
        let mut reducer = KernelReducer::new();
        let drops = vec![
            drop_frame("[\"EVENT\",{}]"),
            drop_frame("[\"REQ\",\"s\",{}]"),
            drop_frame("[\"CLOSE\",\"s\"]"),
        ];

        let events = surface_outbound_drops(&drops, &mut reducer);

        let kinds: Vec<&str> = events
            .iter()
            .map(|e| match e {
                BrowserRuntimeEvent::RelayOutboundDropped { kind, .. } => kind.as_str(),
                other => panic!("unexpected event: {other:?}"),
            })
            .collect();
        assert_eq!(kinds, vec!["EVENT", "REQ", "CLOSE"]);
    }

    #[test]
    fn empty_drops_surface_nothing() {
        let mut reducer = KernelReducer::new();
        let events = surface_outbound_drops(&[], &mut reducer);
        assert!(events.is_empty());
    }

    #[test]
    fn unrecognized_frame_classifies_as_other() {
        let mut reducer = KernelReducer::new();
        let drops = vec![drop_frame("not-a-nip01-frame")];
        let events = surface_outbound_drops(&drops, &mut reducer);
        match &events[0] {
            BrowserRuntimeEvent::RelayOutboundDropped { kind, .. } => assert_eq!(kind, "other"),
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
