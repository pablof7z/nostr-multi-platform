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

/// `CompleteDepositCashu` named a `quote_id` this backend has no pending record
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

/// #2910/#2923 — a `CompleteDepositCashu` attempt for this `quote_id` is
/// already chaining toward a signature (see `PendingDeposit::chain_started_at`'s
/// doc comment); retryable once that attempt finishes or its lease expires,
/// never a hard failure.
pub const DEPOSIT_IN_PROGRESS: &str = "wallet_cashu_deposit_in_progress";

// ─── #2917 (epic #2864 W8/W9/W13) — nutzap loop ────────────────────────────

/// `PublishNutzapInfo` has no relay set to publish kind:10019 to (neither the
/// active account's own cached kind:10019 nor a NIP-65 fallback resolved).
pub const NO_NUTZAP_RELAYS: &str = "wallet_cashu_no_nutzap_relays";

/// `SendNutzap`'s recipient has no cached kind:10019 — see
/// `CachedEventLookup`'s doc comment: this is a point-in-time cache read, not
/// a fetch, so a recipient never previously observed fails closed here.
pub const NO_RECIPIENT_NUTZAP_INFO: &str = "wallet_cashu_no_recipient_nutzap_info";

/// The recipient's kind:10019 lists no mint this wallet also accepts, or (for
/// `RedeemNutzap`) the nutzap's `u` mint is not in the active account's own
/// accepted-mint list.
pub const NO_TRUSTED_MINT: &str = "wallet_cashu_no_trusted_mint";

/// The recipient's kind:10019 carries no usable P2PK pubkey and no fallback
/// (NIP-61 allows falling back to the recipient's Nostr pubkey, but this
/// wallet requires an explicit Cashu P2PK pubkey to lock to).
pub const NO_RECIPIENT_P2PK: &str = "wallet_cashu_no_recipient_p2pk";

/// The recipient's kind:10019 lists no relay to publish the kind:9321 to.
pub const NO_RECIPIENT_RELAYS: &str = "wallet_cashu_no_recipient_relays";

/// This wallet's held proofs at the chosen mint do not cover the requested
/// send amount.
pub const INSUFFICIENT_BALANCE: &str = "wallet_cashu_insufficient_balance";

/// The NUT-03 swap (P2PK-locking outgoing proofs, or unlinking incoming
/// proofs on redeem) failed at the mint.
pub const SWAP_FAILED: &str = "wallet_cashu_swap_failed";

/// `RedeemNutzap` named an `event_id` this backend has never observed (or
/// the observed kind:9321 failed a required verification: wrong `p` tag,
/// untrusted mint, wrong P2PK lock, or bad/missing DLEQ).
pub const INVALID_NUTZAP: &str = "wallet_cashu_invalid_nutzap";

/// `RedeemNutzap` named an `event_id` already redeemed — never double-count.
pub const ALREADY_REDEEMED: &str = "wallet_cashu_already_redeemed";

/// No Cashu wallet is active (no `cashu_pubkey_hex`/`cashu_privkey`) — every
/// nutzap operation requires a created wallet first.
pub const NO_CASHU_WALLET: &str = "wallet_cashu_no_wallet";

// ─── #2965 (epic #2864) — wallet recovery ──────────────────────────────────

/// `RecoverCashuWallet` found no cached kind:17375 wallet event for this
/// account (`ctx.latest_author_kind` — see `recover.rs`): either this account
/// genuinely has no existing NIP-60 wallet on relays (the caller should use
/// `cashu.create` instead), or `wallet_self_authored_shape`'s cold-start
/// replay simply has not delivered it into this session's cache yet.
pub const NO_EXISTING_WALLET: &str = "wallet_cashu_no_existing_wallet";
