//! NIP-59 gift-wrap kinds.

/// Gift-wrap envelope (kind:1059). The opaque outer event minted by the
/// `gift_wrap` builder and tapped by every protocol that delivers content
/// through NIP-59 (NIP-17 DMs, MLS Welcome, etc.). Higher-layer crates
/// import this constant rather than redefining `1059` locally — the integer
/// is owned by NIP-59 because the envelope is NIP-59's protocol.
pub const KIND_GIFT_WRAP: u32 = 1059;
