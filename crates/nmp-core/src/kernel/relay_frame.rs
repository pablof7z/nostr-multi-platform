//! Wire-transport-agnostic relay message frame.
//!
//! [`RelayFrame`] abstracts over `tungstenite::Message` (and any future
//! transport, e.g. `web_sys::WebSocket` for wasm32) so the kernel ingest
//! pipeline compiles without the `native` Cargo feature (V-01 Phase 1c).
//!
//! # Direction of conversion
//!
//! The native `nmp_network::relay_worker` reads `tungstenite::Message` off
//! the WebSocket and `actor::dispatch::tungstenite_message_to_relay_frame`
//! converts each frame to a [`RelayFrame`] *before* handing it to
//! [`crate::kernel::Kernel::handle_message`]. The kernel itself never names
//! `tungstenite`. A non-native transport (wasm32 fetch / WebSocket) is
//! responsible for its own equivalent conversion.
//!
//! # Variant set
//!
//! Only the variants the ingest pipeline actually distinguishes are kept.
//! `Ping`/`Pong` carry no payload here — the only ingest-side use is to bump
//! the inbound counter (and drop, in the case of pong), which the variant
//! tag itself signals.  `Close` carries the optional reason string because
//! the kernel surfaces it on `relay.last_error`. The tungstenite
//! `Message::Frame` raw-frame variant has no kernel-side observable, so it
//! has no counterpart here — the `relay_worker` drops it before conversion.

/// One inbound WebSocket frame, observed by the kernel.
///
/// V-01 Stage 3 — promoted to `pub` so the wasm32 `BrowserRelayDriver` in
/// `nmp-wasm` can construct frames from `web_sys::MessageEvent` /
/// `web_sys::CloseEvent` and hand them to
/// [`crate::KernelReducer::handle_relay_frame`]. Substrate-grade (D0): the
/// enum carries no app/protocol nouns.
#[derive(Debug)]
pub enum RelayFrame {
    /// Text payload — the only frame the kernel actually parses (NIP-01 JSON).
    Text(String),
    /// Binary payload — counted but otherwise ignored.
    Binary(Vec<u8>),
    /// Keepalive ping (server → client) — counted, no payload retained.
    Ping,
    /// Keepalive pong (server → client) — counted, no payload retained.
    Pong,
    /// Connection close — optional reason surfaced on `relay.last_error`.
    Close(Option<String>),
}
