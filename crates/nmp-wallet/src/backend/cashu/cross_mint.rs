//! `CrossMintTransfer` — cross-mint nutzap funding via a NUT-05 melt -> mint
//! bridge (#3003, tracking issue).
//!
//! # The gap this closes
//!
//! `nutzap.send` (`send.rs`, #2997) can only pay from a mint the sender
//! already holds balance at AND the recipient accepts. When the sender has
//! funds only at mints the recipient does NOT accept, it fails
//! `INSUFFICIENT_BALANCE`/`NO_TRUSTED_MINT` even though it CAN pay by moving
//! value over Lightning: get a mint-quote (bolt11) from a recipient-accepted
//! TARGET mint, MELT proofs at a SOURCE mint where this wallet has balance
//! to pay that invoice, then MINT the tokens at the target.
//!
//! # Architecture — reuse, don't duplicate, the P2PK send path
//!
//! This mints SELF-owned (normal, NOT P2PK) proofs at the target mint and
//! writes them to kind:7375 — exactly like a deposit. `send.rs`'s EXISTING
//! balance-aware mutual-mint selection then transparently picks the
//! now-funded target mint and does its own P2PK-lock swap + kind:9321
//! publish. All new money-critical surface lives in this ONE journaled
//! saga; the P2PK send logic in `send.rs`/`send_worker.rs` is untouched.
//!
//! # The saga (money-safety — the defining requirement)
//!
//! Reuses the existing `WalletOperationState` set unchanged — no new state
//! variant was needed:
//!
//! - **Draft -> Prepared**: the operation exists; no funds moved yet.
//! - *(worker thread, `cross_mint_worker::run_cross_mint_transfer_worker`)*:
//!   target `create_mint_quote(amount_sats)` (TARGET mint) -> source
//!   `create_melt_quote(bolt11)` (SOURCE mint, sized to
//!   `melt_quote.amount + melt_quote.fee_reserve`) -> reserve + durably
//!   journal the consumed SOURCE inputs + `melt_quote_id` (the pre-melt
//!   money-safety write every melt in this codebase must make) -> transition
//!   to **MintPending**.
//! - **MintPending -> MintSettled**: `melt()` — the irreversible leg (the
//!   Lightning payment leaves the source mint). `PAID` -> fold the source
//!   change, advance. Any other outcome (transport failure, or the mint
//!   itself reports non-`PAID`) -> **Unknown**, reconciled ONLY via a later
//!   cold-restart resume's `get_melt_quote_status` (never assumed, never a
//!   live retry loop — mirrors every other Cashu saga in this crate).
//! - **MintSettled -> PublishPending -> Settled**: mint the target's tokens
//!   (retryable while the target quote is PAID-not-yet-ISSUED,
//!   write-if-absent fenced via `PendingCrossMintTransfer::minted_proofs` —
//!   mirrors `PendingDeposit::minted_proofs`, #2946) and publish the
//!   resulting kind:7375. On settle, the target mint is added to this
//!   wallet's accepted-mint list (`state.mints`) so `send.rs`'s mutual-mint
//!   check finds it, and (only for `send.rs`'s auto-fallback) the original
//!   `SendNutzap` is re-dispatched.
//!
//! See `cross_mint_worker.rs` for the mint HTTP round-trips and
//! `cross_mint_resume.rs` for the durable WAL write-through + cold-restart
//! reconciliation (the crash-after-melt-before-mint case #3003 exists to
//! close without ever double-spending or double-minting), and
//! `start_intents.rs`'s `start_cross_mint_transfer` for the pre-dispatch
//! validation + source-mint auto-selection + journal pre-record.
//!
//! # Source/target mint selection
//!
//! Target: the first recipient-accepted mint this wallet can fund (mirrors
//! `send.rs`'s own mutual-mint ordering) — resolved by the caller
//! (`send.rs`'s fallback, or the explicit action's `target_mint` argument).
//! Source: NOT a single pre-committed mint but an ORDERED list of SETTLEABLE
//! candidates (#3010) — every mint (NOT the target, NOT a known valueless
//! test mint) that can cover `amount_sats` (a lower-bound proxy for the real,
//! fee-inclusive total — see
//! `state_cross_mint::spendable_source_candidates_excluding`'s doc comment),
//! largest spendable balance first. The worker walks the list and, on any
//! PRE-melt failure at a candidate (melt-quote/keyset/reserve — none move
//! funds), falls through to the next; the moment a candidate reserves and
//! melts is the commit point (past which it never advances — an ambiguous
//! melt is reconciled via resume, never retried against a fresh source). No
//! splitting across source mints in v1 — fails closed
//! (`NO_FUNDABLE_SOURCE_MINT`/`INSUFFICIENT_BALANCE`) if no single settleable
//! source mint covers it. A known valueless test mint (e.g. testnut) is never
//! a candidate: it hands out free ecash but cannot settle a real bolt11, so
//! its melt hangs `PENDING` — see `is_known_valueless_mint`.

use std::fmt;
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_nip60::KIND_NIP60_TOKEN;

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};

use crate::fail_closed::fail_closed;
use crate::journal::{WalletOperationId, WalletOperationKind};

pub(super) use super::cross_mint_worker::SendRetry;
use super::cross_mint_worker::{run_cross_mint_transfer_worker, CrossMintWorkerArgs};
use super::state::{lock_state, CashuWalletState};
use super::{is_well_formed_mint_url, operation_id_for, ui_codes};

pub(super) struct CrossMintTransferCommand {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) operation_id: WalletOperationId,
    pub(super) account_pubkey: String,
    pub(super) target_mint: String,
    /// Ordered (largest-balance-first) settleable SOURCE candidates the worker
    /// walks until one yields a melt quote it can reserve+melt against (#3010)
    /// — never a single pre-committed source. Each entry is
    /// `(canonical_mint_url, spendable_total)`.
    pub(super) source_candidates: Vec<(String, u64)>,
    pub(super) amount_sats: u64,
    pub(super) correlation_id: Option<String>,
    pub(super) on_settled: Option<SendRetry>,
}

impl fmt::Debug for CrossMintTransferCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CrossMintTransferCommand")
            .field("operation_id", &self.operation_id.as_str())
            .field("amount_sats", &self.amount_sats)
            .finish_non_exhaustive()
    }
}

impl ProtocolCommand for CrossMintTransferCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            state,
            operation_id,
            account_pubkey,
            target_mint,
            source_candidates,
            amount_sats,
            correlation_id,
            on_settled,
        } = *self;
        // The target-mint self-encrypted kind:7375 publishes to this
        // account's OWN relays (a deposit-shaped event, not a recipient-
        // addressed one) — same seam `deposit.rs`/`create_wallet.rs` use.
        let relays = ctx.recipient_publish_relays(&account_pubkey, KIND_NIP60_TOKEN);
        let created_at = ctx.now_secs();
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            run_cross_mint_transfer_worker(CrossMintWorkerArgs {
                worker_tx,
                state,
                operation_id,
                account_pubkey,
                target_mint,
                source_candidates,
                amount_sats,
                relays,
                created_at,
                correlation_id,
                on_settled,
            });
        });
        Ok(())
    }
}

/// Build the `ActorCommand`s for a cross-mint transfer, given an ALREADY
/// KNOWN `account_pubkey` (no `WalletBackendContext`/`&CashuWalletBackend`
/// needed) — the shared validation + source-mint auto-selection + journal
/// pre-record both callers go through:
///
/// - `start_intents.rs`'s `start_cross_mint_transfer` (the standalone
///   `nmp.wallet.cashu.cross_mint_transfer` action) resolves `account_pubkey`
///   from `ctx.account_pubkey` and passes `on_settled: None`.
/// - `send.rs`'s `nutzap.send` auto-fallback already resolved
///   `account_pubkey` for its OWN dispatch and passes `on_settled: Some(_)`
///   to re-drive the original send once the transfer settles.
///
/// Fails closed with `UNSUPPORTED_MINT`/`NO_FUNDABLE_SOURCE_MINT`/
/// `JOURNAL_ERROR` — never panics on a bad `target_mint`/`amount_sats` (D6).
#[allow(clippy::too_many_arguments)]
pub(super) fn build_cross_mint_transfer(
    state: Arc<Mutex<CashuWalletState>>,
    account_pubkey: String,
    now_secs: u64,
    target_mint: String,
    amount_sats: u64,
    correlation_id: Option<String>,
    on_settled: Option<SendRetry>,
) -> Vec<ActorCommand> {
    if amount_sats == 0 {
        return fail_closed(
            ui_codes::UNSUPPORTED_MINT,
            correlation_id,
            "cross_mint_transfer requires amount_sats > 0".to_string(),
        );
    }
    if !is_well_formed_mint_url(&target_mint) {
        return fail_closed(
            ui_codes::UNSUPPORTED_MINT,
            correlation_id,
            format!("unsupported target mint: {target_mint}"),
        );
    }
    // #3010 — resolve the FULL ordered list of settleable source candidates
    // (largest balance first, excluding the target and any known valueless
    // test mint) rather than one pre-committed source. The worker walks this
    // list and, on any pre-melt failure (melt-quote/keyset/reserve — all
    // move no funds), falls through to the next candidate; only a candidate
    // that actually reserves+melts commits. Fail closed here only if NO
    // settleable mint could fund it at all.
    let source_candidates =
        lock_state(&state).spendable_source_candidates_excluding(&target_mint, amount_sats);
    if source_candidates.is_empty() {
        return fail_closed(
            ui_codes::NO_FUNDABLE_SOURCE_MINT,
            correlation_id,
            "no settleable mint holds enough spendable balance to fund this transfer".to_string(),
        );
    }
    let operation_id = operation_id_for(&correlation_id, now_secs, "cross-mint");
    {
        let mut s = lock_state(&state);
        if let Err(e) = s.begin_operation_at(
            operation_id.clone(),
            WalletOperationKind::CrossMintTransfer,
            now_secs,
        ) {
            return fail_closed(ui_codes::JOURNAL_ERROR, correlation_id, format!("{e:?}"));
        }
    }
    vec![ActorCommand::Protocol(Box::new(CrossMintTransferCommand {
        state,
        operation_id,
        account_pubkey,
        target_mint,
        source_candidates,
        amount_sats,
        correlation_id,
        on_settled,
    }))]
}
