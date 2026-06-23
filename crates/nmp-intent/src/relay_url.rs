//! Relay-URL recognition + normalization (issue #1804).
//!
//! Detects a `ws://` / `wss://` relay URL in the raw input and normalizes it
//! (lowercase scheme/host, trailing-slash policy) so the classifier can emit a
//! canonical `InputIntentTarget::RelayUrl`. SHAPE/parse only — never connects.
//!
//! S1 fills the body; the module exists now to freeze the path.

// S1: ws/wss detection + normalization (mirrors
// `nmp_core::substrate::canonicalize_relay_url`).
