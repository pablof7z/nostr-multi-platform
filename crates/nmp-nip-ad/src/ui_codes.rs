//! NIP-AD user-facing error codes (#2927).
//!
//! Stable machine keys carried by [`nmp_core::ui_token::UiToken::code`] so the
//! shells render localized prose instead of the English fallback. The set is
//! CLOSED — adding a key is a deliberate FFI-contract change. `subject` carries
//! the AD URL for shell interpolation.

/// The supplied AD URL failed shape validation (not an http(s) URL, no host).
/// `subject` carries the raw URL as supplied.
pub const RESOLVE_INVALID: &str = "nip_ad_resolve_invalid";

/// The `.well-known/nostr.json?ad=<path>` fetch or parse failed (SSRF reject,
/// DNS, network, no matching path entry, malformed `{filter, relays}`).
/// `subject` carries the AD URL; `raw_detail` carries the failure reason.
pub const RESOLVE_FAILED: &str = "nip_ad_resolve_failed";

/// An AD resolution was requested but the native HTTP fetcher is not compiled
/// into this build (wasm32 / no-IO target). `subject` carries the AD URL.
pub const RESOLVE_NATIVE_UNAVAILABLE: &str = "nip_ad_resolve_native_unavailable";
