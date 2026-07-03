//! `nmp-wallet` Cashu-backend user-facing error codes (issue #1682 pattern).
//!
//! Stable machine keys carried by [`nmp_core::ui_token::UiToken::code`] so
//! shells render localized prose instead of the English fallback. Mirrors
//! `nmp-nip47::ui_codes`'s convention, namespaced `wallet_cashu_*` (this is
//! `nmp-wallet`'s own product surface, not a NIP crate).

/// No account is active; `CreateCashuWallet` needs an identity to
/// self-encrypt/sign for.
pub const NO_ACCOUNT: &str = "wallet_cashu_no_account";

/// `CreateCashuWallet` was dispatched again after this wallet already
/// completed creation — refuse rather than silently overwrite `mints`/
/// `cashu_pubkey_hex` for a wallet that may already hold ledger balance.
pub const ALREADY_CREATED: &str = "wallet_cashu_already_created";

/// The requested mint URL is malformed or (for a deposit) not one this
/// wallet was created with.
pub const UNSUPPORTED_MINT: &str = "wallet_cashu_unsupported_mint";

/// `CompleteDeposit` named a `quote_id` this backend has no pending record
/// for (unknown or already completed).
pub const UNKNOWN_QUOTE: &str = "wallet_cashu_unknown_quote";

/// The signer-transparent NIP-44 self-encrypt or sign port failed (including
/// "signer can't NIP-44" — fail closed rather than fall back to raw keys).
pub const OPERATION_FAILED: &str = "wallet_cashu_operation_failed";

/// The NUT-04 mint-quote request itself failed (network/protocol error).
pub const MINT_QUOTE_FAILED: &str = "wallet_cashu_mint_quote_failed";

/// The mint quote has not been paid yet — retryable, not a hard failure.
pub const QUOTE_NOT_PAID: &str = "wallet_cashu_quote_not_paid";

/// The value-moving NUT-04 mint-tokens call failed after the quote was paid.
pub const MINT_TOKENS_FAILED: &str = "wallet_cashu_mint_tokens_failed";

/// A durable journal operation could not be recorded/transitioned (should be
/// unreachable in normal operation; surfaced rather than silently dropped).
pub const JOURNAL_ERROR: &str = "wallet_cashu_journal_error";
