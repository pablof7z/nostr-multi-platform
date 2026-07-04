//! `DepositQuoteCashu` / `CompleteDepositCashu` — the two-phase Cashu deposit flow
//! (#2895 W2), split because the two NUT-04 mint HTTP calls happen at
//! different times and only one of them moves value:
//!
//! - [`CashuDepositQuoteCommand`] requests a mint quote (a bolt11 invoice).
//!   Never moves value — no proofs are consumed or minted — so it can
//!   complete before any invoice is paid. Persists the quote to the journal
//!   (`MintPending` -> `MintSettled`) and surfaces `{quote_id, bolt11, mint,
//!   amount_sats}` through the action's `RecordActionSuccess` result JSON —
//!   the one-shot channel a caller keeps to pay the invoice and later name
//!   `quote_id` back to `CompleteDepositCashu`. Never the bounded
//!   `WalletProjection` or a log line (quote ids are secret-adjacent).
//! - [`CashuCompleteDepositCommand`] checks the quote's paid state (testnut:
//!   already `Paid`; a real mint: `Paid` once the invoice above is settled
//!   externally), then mints tokens — THE value-moving call — and writes the
//!   resulting proofs as a NIP-44 self-encrypted kind:7375 token event via the
//!   same signer-transparent chain `create_wallet.rs` uses.
//!
//! Both HTTP round-trips run on a spawned worker thread (D8 — the actor
//! thread never blocks on mint HTTP); the worker writes results back into the
//! shared [`CashuWalletState`](super::state::CashuWalletState) directly,
//! mirroring how `NwcWalletBackend`'s runtime is the sole writer of its
//! `WalletStatusSlot`.
//!
//! This module is split into cohesive submodules (AGENTS.md LOC discipline):
//! [`quote`] owns `DepositQuoteCashu`, [`complete`] owns the
//! `CompleteDepositCashu` resume/lease/fencing state machine, and
//! [`token_event`] owns the kind:7375 build + `on_signed` ledger/proof fold.
//! This file re-exports the three cross-module entry points and holds the
//! `fail` helper both command submodules share.
//!
//! # NOT wired here (escalated, not silently skipped)
//!
//! `nmp_nip60::Nip60WalletHandle::complete_deposit` also queues a kind:7376
//! spending-history event alongside the kind:7375 token event. This backend
//! does not — the #2895 W2 design scoped `CompleteDepositCashu` to "mint tokens
//! ... store proofs -> write kind:7375" only. A wallet driven exclusively by
//! this backend will show correct balances (the ledger folds `TokenAdded`
//! facts, not history events) but an incomplete kind:7376 history stream for
//! other NIP-60 clients reading this wallet. Follow-up, not a silent gap.
//!
//! # Known limitation — a fenced-out (superseded) attempt still publishes
//!
//! #2910/#2923's `chain_started_at` lease (see
//! [`PendingDeposit`](super::state::PendingDeposit)'s doc comment) stops a
//! stale, superseded `CompleteDepositCashu` attempt from double-folding the
//! ledger/proof inventory (its `on_signed` fences on the lease and skips the
//! mutation), but `chain.rs`'s `sign_and_publish` still unconditionally
//! publishes right after `on_signed` returns — there is no hook to abort that
//! from inside the fenced-out closure. In the (narrow, requires an
//! actually-abandoned-but-not-dead attempt to still finish signing) case this
//! fires, the fenced-out attempt's real proofs still land on a relay as a
//! valid, independently-signed kind:7375 event this backend just never
//! reconciles locally today (kind:7375 cold-start reconciliation isn't
//! implemented — see `mod.rs`'s "Live wiring... vs. still-deferred behavior"
//! module doc comment). Not a fund-loss or double-count risk today; would
//! become a delayed double-fold if/when that reconciliation ships without its
//! own proof-identity dedup — worth remembering when that lands.

mod complete;
mod quote;
mod resume;
mod token_event;

pub(super) use complete::CashuCompleteDepositCommand;
pub(super) use quote::CashuDepositQuoteCommand;
pub(super) use resume::ResumeDepositCommand;
pub(super) use token_event::{dispatch_token_event, token_event_plaintext};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::build_record_action_failure;
use nmp_core::ui_token::UiToken;
use nmp_core::CommandSender;

/// Surface a failure both as a UI error token (always) and, when the caller
/// kept a one-shot action-result channel, a `RecordActionFailure` on that
/// `correlation_id`. Shared by [`quote`] and [`complete`] — the two deposit
/// commands' single fail-closed reporting path.
fn fail(
    worker_tx: &CommandSender,
    correlation_id: Option<String>,
    code: &'static str,
    reason: String,
) {
    let token = UiToken::error(code, reason.clone());
    let _ = worker_tx.send(ActorCommand::ShowErrorToken { token });
    if let Some(id) = correlation_id {
        let _ = worker_tx.send(build_record_action_failure(id, reason));
    }
}
