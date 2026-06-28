//! NIP-05 user-facing error codes (issue #2285 / #1682).
//!
//! Stable machine keys carried by [`nmp_core::ui_token::UiToken::code`] so the
//! shells render localized prose instead of the English fallback. The set is
//! CLOSED — adding a key is a deliberate FFI-contract change. The shells map
//! every key here to localized copy; an unknown key falls back to the token's
//! English `fallback_prose`. `subject` carries the `name@domain` identifier
//! string for shell interpolation.

/// The supplied `name@domain` NIP-05 identifier failed shape validation
/// (invalid local-part charset, malformed domain). `subject` carries the
/// raw identifier as supplied.
pub const LOOKUP_INVALID: &str = "nip05_lookup_invalid";

/// The `.well-known/nostr.json` HTTP fetch failed (DNS, network, or the
/// `names` map did not contain the requested local-part). `subject` carries
/// the `name@domain` identifier; `raw_detail` carries the failure reason.
pub const LOOKUP_FAILED: &str = "nip05_lookup_failed";

/// A NIP-05 lookup was requested but the native HTTP fetcher is not compiled
/// into this build (wasm32 / no-IO target). `subject` carries the identifier.
pub const LOOKUP_NATIVE_UNAVAILABLE: &str = "nip05_lookup_native_unavailable";
