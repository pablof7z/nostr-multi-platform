//! NIP-47 event kinds.

/// NWC client → wallet service request (encrypted with NIP-44).
pub const KIND_NWC_REQUEST: u32 = 23194;

/// Wallet service → client response (encrypted with NIP-44).
pub const KIND_NWC_RESPONSE: u32 = 23195;
