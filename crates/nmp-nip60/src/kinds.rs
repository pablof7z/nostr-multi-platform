//! Kind constants for NIP-60 (Cashu wallet) and related NIPs.

/// NIP-60: Cashu wallet event — encrypted wallet config (privkey + mints).
pub const KIND_WALLET: u16 = 17375;

/// NIP-60: Cashu wallet unspent proof — encrypted token event.
pub const KIND_TOKEN: u16 = 7375;

/// NIP-60: Cashu spending history event.
pub const KIND_HISTORY: u16 = 7376;

/// NIP-60: Cashu wallet redeeming a quote (deposit in-progress).
pub const KIND_QUOTE: u16 = 7374;

/// NIP-61: Cashu nutzap informational event — advertises accepted mints + pubkey.
pub const KIND_NUTZAP_INFO: u16 = 10019;

/// NIP-61: Cashu nutzap event — sends ecash proofs to a recipient.
pub const KIND_NUTZAP: u16 = 9321;

/// NIP-88: Mint announcement — mint publishes its metadata to Nostr.
pub const KIND_MINT_ANNOUNCE: u16 = 38172;
