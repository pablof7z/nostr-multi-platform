//! NIP-17 user-facing error codes (issue #1682).
//!
//! Stable machine keys carried by [`nmp_core::ui_token::UiToken::code`] so the
//! shells render localized prose instead of the English fallback. The set is
//! CLOSED — adding a key is a deliberate FFI-contract change. The shells map
//! every key here to localized copy; an unknown key falls back to the token's
//! English `fallback_prose`.

/// A DM could not be sent (pre-publish failure: missing signer, no kind:10050
/// DM-relay list, encode/seal error). `raw_detail` carries the reason.
pub const DM_SEND_FAILED: &str = "nip17_dm_send_failed";

/// A DM gift-wrap envelope failed during the publish continuation. `subject`
/// carries the envelope label (`recipient` / `self-copy`); `raw_detail` the
/// reason.
pub const DM_GIFTWRAP_FAILED: &str = "nip17_dm_giftwrap_failed";
