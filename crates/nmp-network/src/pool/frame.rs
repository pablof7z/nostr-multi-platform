//! Wire-frame ⇄ pool-frame conversion for the [`super::Pool`] translator.
//!
//! Split out of `pool::inner` (file-size ownership): the
//! `tungstenite::Message → RelayFrame` direction (inbound, incl. the NIP-42
//! AUTH pre-classification) and the `WireFrame → RelayCommand` direction
//! (outbound) form one cohesive unit — the only place this crate maps between
//! the raw WebSocket message type and the pool's typed frame surface.
//!
//! These functions are deliberately lock-free: the translator runs
//! [`tungstenite_to_relay_frame`] *before* taking the `PoolInner` lock so the
//! per-frame JSON parse never blocks concurrent `Pool::send` calls.

use tungstenite::Message;

use crate::relay_worker::RelayCommand;

use super::types::{RelayFrame, WireFrame};

/// Convert one `tungstenite::Message` into a [`RelayFrame`]. Returns
/// `None` for the raw `Frame(_)` variant which the kernel has never
/// observed.
///
/// ## Step 8 phase E — AUTH pre-classification
///
/// Text frames are peeked for the `["AUTH", <challenge>]` NIP-42 shape
/// (using the dependency-free parser in `nmp-nip42-types`). A match
/// surfaces as [`RelayFrame::Auth`]; everything else (including
/// malformed AUTH frames with empty challenges) stays as
/// [`RelayFrame::Text`] so the kernel's existing ingest parser handles
/// them uniformly. This is the **only** AUTH-aware behaviour in this
/// crate — building the kind:22242 reply event lives in `nmp-nip42`,
/// and the per-relay pause/replay FSM lives in
/// `nmp-core::subs::AuthGate`.
pub(super) fn tungstenite_to_relay_frame(message: Message) -> Option<RelayFrame> {
    match message {
        Message::Text(text) => Some(classify_text_frame(text)),
        Message::Binary(bytes) => Some(RelayFrame::Binary(bytes)),
        Message::Ping(_) => Some(RelayFrame::Ping),
        Message::Pong(_) => Some(RelayFrame::Pong),
        Message::Close(frame) => Some(RelayFrame::Close(frame.map(|f| f.reason.into_owned()))),
        Message::Frame(_) => None,
    }
}

/// Peek a text frame for the NIP-42 `["AUTH", <challenge>]` shape and
/// pre-classify it as [`RelayFrame::Auth`]; fall through to
/// [`RelayFrame::Text`] on anything else (non-AUTH frame, malformed
/// JSON, empty challenge).
///
/// `pub(crate)` so the phase-E pool tests can exercise the classifier
/// directly without spinning up a real socket.
pub(crate) fn classify_text_frame(text: String) -> RelayFrame {
    // Cheap fast-path: only parse JSON if the frame looks like it might
    // be an AUTH frame (NIP-42 frames are `["AUTH", ...]` — case-sensitive).
    if !text.contains("\"AUTH\"") {
        return RelayFrame::Text(text);
    }
    let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return RelayFrame::Text(text);
    };
    // `relay_url` is empty here — the wire layer doesn't know its own
    // URL (the worker is keyed by URL but the value isn't threaded into
    // the translator), and `AuthChallenge.relay_url` is only used by
    // the kernel for the `["relay", <url>]` tag on the kind:22242
    // response. The kernel stamps the delivering URL from its own
    // context (see `kernel/ingest/auth_handlers.rs` and ADR-T125).
    // We only need `parse_auth_frame`'s shape + non-empty-challenge
    // validation here.
    match nmp_nip42_types::parse_auth_frame(&parsed, "") {
        Some(challenge) => RelayFrame::Auth(challenge.challenge),
        None => RelayFrame::Text(text),
    }
}

/// Convert a [`WireFrame`] into the worker's `RelayCommand::Send(String)`.
/// Today only `Text` is wire-emittable; `Binary` is reserved.
pub(super) fn wire_frame_to_command(frame: WireFrame) -> Option<RelayCommand> {
    match frame {
        WireFrame::Text(text) => Some(RelayCommand::Send(text)),
        WireFrame::Binary(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A well-formed `["AUTH", <challenge>]` frame is pre-classified as
    /// [`RelayFrame::Auth`] carrying the challenge string.
    #[test]
    fn classify_auth_extracts_non_empty_challenge() {
        let frame = classify_text_frame(r#"["AUTH","challenge-token-123"]"#.to_string());
        match frame {
            RelayFrame::Auth(challenge) => assert_eq!(challenge, "challenge-token-123"),
            other => panic!("expected RelayFrame::Auth, got {other:?}"),
        }
    }

    /// A non-AUTH text frame passes through verbatim as [`RelayFrame::Text`].
    #[test]
    fn classify_passes_non_auth_text_through_untouched() {
        let raw = r#"["EVENT","sub",{"id":"abc"}]"#.to_string();
        match classify_text_frame(raw.clone()) {
            RelayFrame::Text(text) => assert_eq!(text, raw),
            other => panic!("expected RelayFrame::Text, got {other:?}"),
        }
    }

    /// A malformed AUTH frame (empty challenge) falls through to
    /// [`RelayFrame::Text`] so the kernel's ingest parser sees it unchanged.
    #[test]
    fn classify_malformed_auth_falls_through_to_text() {
        let raw = r#"["AUTH",""]"#.to_string();
        match classify_text_frame(raw.clone()) {
            RelayFrame::Text(text) => assert_eq!(text, raw),
            other => panic!("expected RelayFrame::Text for empty challenge, got {other:?}"),
        }
    }

    /// The cheap fast-path must not misfire on a frame that merely contains the
    /// `"AUTH"` substring inside a non-AUTH position (e.g. event content).
    #[test]
    fn classify_does_not_misfire_on_auth_substring_in_other_frames() {
        let raw = r#"["EVENT","sub",{"content":"the \"AUTH\" word"}]"#.to_string();
        match classify_text_frame(raw.clone()) {
            RelayFrame::Text(text) => assert_eq!(text, raw),
            other => panic!("expected RelayFrame::Text, got {other:?}"),
        }
    }

    /// Invalid JSON that contains the `"AUTH"` token still falls through to
    /// [`RelayFrame::Text`] (D6 — never panics on malformed input).
    #[test]
    fn classify_invalid_json_falls_through_to_text() {
        let raw = r#"["AUTH", not-valid-json"#.to_string();
        match classify_text_frame(raw.clone()) {
            RelayFrame::Text(text) => assert_eq!(text, raw),
            other => panic!("expected RelayFrame::Text for invalid JSON, got {other:?}"),
        }
    }

    /// Binary / Ping / Pong / Close map to their typed `RelayFrame` variants;
    /// the raw `Frame(_)` variant yields `None` (never observed by the kernel).
    #[test]
    fn tungstenite_message_variants_map_to_relay_frames() {
        assert!(matches!(
            tungstenite_to_relay_frame(Message::Binary(vec![1, 2, 3])),
            Some(RelayFrame::Binary(_))
        ));
        assert!(matches!(
            tungstenite_to_relay_frame(Message::Ping(Vec::new())),
            Some(RelayFrame::Ping)
        ));
        assert!(matches!(
            tungstenite_to_relay_frame(Message::Pong(Vec::new())),
            Some(RelayFrame::Pong)
        ));
    }

    /// A `WireFrame::Text` becomes a `RelayCommand::Send`; `Binary` is reserved
    /// and yields `None` (not yet wire-emittable).
    #[test]
    fn wire_frame_text_becomes_send_command_binary_is_none() {
        assert!(matches!(
            wire_frame_to_command(WireFrame::Text("x".to_string())),
            Some(RelayCommand::Send(_))
        ));
        assert!(wire_frame_to_command(WireFrame::Binary(vec![0])).is_none());
    }
}
