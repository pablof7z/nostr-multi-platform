//! Stable machine codes for user-facing NIP-46/NIP-55 handshake progress labels
//! (#1711, part of #1670).
//!
//! The broker owns the prose for the handshake steps it drives, so — mirroring
//! the `UiToken` code registry pattern (`nmp_nip17::ui_codes`, etc.) — it owns
//! the stable `code` keys too. `nmp-signer-broker` has no `nmp-core` dependency
//! (D0), so these live here as protocol-neutral `&'static str` constants; the
//! broker emits one alongside the English fallback prose, and the shells localize
//! the code (falling back to the prose for any key they don't recognize).
//!
//! Only the user-facing *progress* labels carry a code. Diagnostic / `"failed"`
//! transitions (e.g. `dialing {url}`, `parse bunker uri: {e}`) carry raw upstream
//! detail, not curated copy, so they stay prose-only (`code == None`).

/// NIP-46 bunker handshake: sending the `connect` request to the bunker.
pub const SENDING_CONNECT_TO_BUNKER: &str = "signer_progress_sending_connect_to_bunker";
/// NIP-46 bunker handshake: awaiting the user's approval in the bunker app.
pub const AWAITING_BUNKER_APPROVAL: &str = "signer_progress_awaiting_bunker_approval";
/// NostrConnect handshake: waiting for the signer to scan the QR code.
pub const NOSTRCONNECT_SCAN_QR: &str = "signer_progress_nostrconnect_scan_qr";
/// NostrConnect handshake: awaiting the user's confirmation in the signer app.
pub const NOSTRCONNECT_AWAITING_CONFIRMATION: &str =
    "signer_progress_nostrconnect_awaiting_confirmation";
