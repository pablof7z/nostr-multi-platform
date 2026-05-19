//! NIP-57 zap kinds.

/// Zap request (built by the client, embedded in the LN-paid receipt).
pub const KIND_ZAP_REQUEST: u32 = 9734;

/// Zap receipt (minted by the LN provider after payment). Decode-only —
/// clients never construct kind:9735 directly.
pub const KIND_ZAP_RECEIPT: u32 = 9735;
